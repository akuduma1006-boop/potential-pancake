//! ☢ NukeCraft: ядерный пульт для Horizon (Minecraft Java 26.3 на Rust).
//!
//! * **ПКМ** пультом: удар выбранным оружием в точку прицела.
//! * **Shift + ПКМ**: меню выбора оружия.
//!
//! Как устроено:
//! * клиент рисует меню (`menu.rs`) и по ПКМ шлёт серверу запрос `Fire` (`net.rs`);
//! * сервер ищет точку прицела, ведёт ракету и взрывает (`strike.rs`);
//! * сервер шлёт клиентам `Boom`, и у тех, кто рядом, трясётся камера.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use mod_api::*;

mod menu;
mod net;
mod strike;
mod weapons;

/// Id мода: он же пространство имён его предметов.
pub const MOD_ID: &str = "nukecraft";

/// Ключи, полученные при регистрации.
pub struct Keys {
    pub remote: ItemKey,
    pub icons: Vec<ItemKey>,
    pub particles: strike::Particles,
    pub blocks: strike::BlockKeys,
}

static KEYS: OnceLock<Keys> = OnceLock::new();

pub fn keys() -> &'static Keys {
    KEYS.get()
        .expect("NukeCraft: ключи регистрируются до первого хука")
}

/// Выбранное на этом клиенте оружие.
static SELECTED: AtomicUsize = AtomicUsize::new(0);

pub fn selected() -> usize {
    SELECTED.load(Ordering::Relaxed)
}

pub fn select(index: usize) {
    SELECTED.store(index.min(weapons::WEAPONS.len() - 1), Ordering::Relaxed);
}

#[derive(Default)]
pub struct NukeCraft {
    menu: menu::Menu,
}

/// Поведение предмета-пульта.
struct Remote;

#[mod_api::horizon_part]
impl ItemPart for Remote {
    fn use_item(
        &self,
        host: HostSide<'_>,
        _event: &mut UseItemEvent<'_>,
    ) -> Option<InteractionResult> {
        match host {
            HostSide::Client(host) => {
                let sneaking = host
                    .mapping("key.sneak")
                    .ok()
                    .flatten()
                    .is_some_and(|m| m.down);
                if sneaking {
                    menu::open(host);
                } else {
                    net::send_fire(host, selected());
                }
                Some(InteractionResult::Success)
            }
            // Сервер ждёт запрос `Fire`: его шлёт клиент, а здесь только «съедаем» клик.
            HostSide::Server(_) => Some(InteractionResult::Consume),
        }
    }
}

#[mod_api::horizon_mod]
impl Hooks for NukeCraft {
    fn register(&self, registrar: &mut dyn Registrar, _cx: &RegistrationContext) {
        let remote = registrar.item_key("nukecraft:nuke_remote");
        registrar.item_part(remote, Remote);
        let icons = weapons::WEAPONS
            .iter()
            .map(|w| registrar.item_key(w.icon))
            .collect();
        let particles = strike::Particles::register(registrar);
        let blocks = strike::BlockKeys::register(registrar);
        let _ = KEYS.set(Keys {
            remote,
            icons,
            particles,
            blocks,
        });
        net::register(registrar);
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        if matches!(event.phase, ServerTickPhase::ServerTickEnd { .. }) {
            strike::tick(host);
        }
    }

    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        if let Some(player) = host.player(event.player) {
            let _ = player.tell(
                host,
                Text::literal("☢ NukeCraft: ПКМ пультом — удар, Shift + ПКМ — выбор оружия"),
            );
        }
    }

    fn camera_shake(&self, host: &dyn ClientHost, event: &mut CameraShakeEvent<'_>) {
        net::apply_shake(host, event);
    }

    // ── меню ──
    fn client_tick_pre(&self, host: &mut dyn ClientHost, _: &mut ClientTickPreEvent) {
        self.menu.tick(host);
    }

    fn screen_render_post(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        self.menu.draw(host, event);
    }

    fn screen_mouse_button_pressed_pre(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        self.menu.click(host, event)
    }
}
