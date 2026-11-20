//! Меню ядерного пульта: свой экран мода с карточками оружия.
//!
//! ```text
//!  ┌──────────────────┐   ┌──────────────────────────┐
//!  │ ☢ ЯДЕРНЫЙ ПУЛЬТ  │   │ [icon] NK-01 Фугасная    │
//!  │                  │   ├──────────────────────────┤
//!  │     [пульт]      │   │ [icon] NK-02 Тактический │ ← выбрано
//!  │                  │   │  Радиус  ▓▓▓▓░░░░░░      │
//!  │  Заряжено: NK-02 │   │  Скорость ▓▓▓▓▓▓░░░      │
//!  └──────────────────┘   └──────────────────────────┘
//! ```

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use mod_api::*;

use crate::weapons::{WEAPONS, Weapon};

/// Под этим id игра знает экран нашего мода.
const SCREEN: &str = crate::MOD_ID;

const CARD_W: f32 = 200.0;
const CARD_H: f32 = 24.0;
const CARD_GAP: f32 = 4.0;
const PANEL_W: f32 = 140.0;
const PANEL_H: f32 = 176.0;

/// Состояние меню на клиенте.
#[derive(Default)]
pub struct Menu {
    /// Где на экране лежат карточки: (прямоугольник, номер оружия).
    cards: Mutex<Vec<([f32; 4], usize)>>,
    /// Попросили закрыть: закроем в следующем тике (во входных хуках хост только читает).
    close: AtomicBool,
}

/// Открыть меню.
pub fn open(host: &mut dyn ClientHost) {
    host.mod_screen_open(SCREEN);
}

fn inside(r: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}

/// Рамка в 1 пиксель.
fn frame(draw: &mut dyn DrawContext, r: [f32; 4], argb: u32) {
    draw.fill([r[0], r[1], r[2], 1.0], argb);
    draw.fill([r[0], r[1] + r[3] - 1.0, r[2], 1.0], argb);
    draw.fill([r[0], r[1], 1.0, r[3]], argb);
    draw.fill([r[0] + r[2] - 1.0, r[1], 1.0, r[3]], argb);
}

/// Полоска характеристики: подпись и шкала 0..1.
fn stat(draw: &mut dyn DrawContext, at: [f32; 2], label: &str, value: f32, argb: u32) {
    draw.text(label, at, 0xFF_BDBDBD, false);
    let bar = [at[0] + 52.0, at[1] + 1.0, 70.0, 6.0];
    draw.fill(bar, 0xFF_303030);
    draw.fill(
        [bar[0], bar[1], bar[2] * value.clamp(0.0, 1.0), bar[3]],
        argb,
    );
}

impl Menu {
    pub fn tick(&self, host: &mut dyn ClientHost) {
        if self.close.swap(false, Ordering::Relaxed) {
            host.mod_screen_close(SCREEN);
        }
    }

    pub fn draw(&self, host: &dyn ClientHost, event: &mut ScreenRenderPostEvent<'_>) {
        if host.mod_screen() != Some(SCREEN) {
            return;
        }
        let draw = &mut *event.draw;
        let mouse = event.screen.mouse;
        let [w, h] = draw.size();
        let selected = crate::selected();
        let chosen: &Weapon = crate::weapons::weapon(selected);

        // Затемнение фона.
        draw.fill([0.0, 0.0, w, h], 0xA0_000000);

        // Заголовок.
        let title = "T H E   N U K E   R E M O T E";
        let tw = draw.text_width(title);
        draw.text(title, [(w - tw) / 2.0, 14.0], 0xFF_FFFFFF, true);

        let total_w = PANEL_W + 12.0 + CARD_W;
        let left = ((w - total_w) / 2.0).floor();
        let top = ((h - PANEL_H) / 2.0).floor().max(30.0);

        // ── левая панель: пульт ──
        let panel = [left, top, PANEL_W, PANEL_H];
        draw.fill(panel, 0xF0_F5F5F5);
        draw.fill([left, top, PANEL_W, 18.0], 0xFF_212121);
        draw.text(
            "☢ ЯДЕРНЫЙ ПУЛЬТ",
            [left + 8.0, top + 5.0],
            0xFF_FF5252,
            false,
        );
        draw.fill([left + 40.0, top + 26.0, 60.0, 60.0], 0xFF_E0E0E0);
        draw.item(crate::keys().remote, [left + 46.0, top + 32.0], 48.0);
        draw.text("Заряжено:", [left + 8.0, top + 94.0], 0xFF_616161, false);
        draw.text(chosen.code, [left + 8.0, top + 106.0], chosen.color, true);
        draw.text(chosen.name, [left + 8.0, top + 118.0], 0xFF_212121, false);
        let radius = (chosen.radius / 55.0) as f32;
        let speed = (chosen.wave_speed / 2.0) as f32;
        stat(
            draw,
            [left + 8.0, top + 136.0],
            "Радиус",
            radius,
            chosen.color,
        );
        stat(
            draw,
            [left + 8.0, top + 148.0],
            "Волна",
            speed,
            chosen.color,
        );
        draw.text(
            "Shift+ПКМ — меню",
            [left + 8.0, top + 162.0],
            0xFF_9E9E9E,
            false,
        );

        // ── правая колонка: карточки оружия ──
        let x = left + PANEL_W + 12.0;
        let mut cards = self.cards.lock().unwrap();
        cards.clear();
        for (i, weapon) in WEAPONS.iter().enumerate() {
            let y = top + i as f32 * (CARD_H + CARD_GAP);
            let r = [x, y, CARD_W, CARD_H];
            let hovered = inside(r, mouse);
            let is_selected = i == selected;
            let bg = if is_selected {
                0xF0_2B2B2B
            } else if hovered {
                0xE0_3A3A3A
            } else {
                0xC0_1E1E1E
            };
            draw.fill(r, bg);
            draw.fill([x, y, 3.0, CARD_H], weapon.color);
            if is_selected {
                frame(draw, r, weapon.color);
            }
            if let Some(icon) = crate::keys().icons.get(i) {
                draw.item(*icon, [x + 6.0, y + 4.0], 16.0);
            }
            draw.text(weapon.code, [x + 26.0, y + 4.0], weapon.color, false);
            draw.text(weapon.name, [x + 62.0, y + 4.0], 0xFF_FFFFFF, false);
            draw.text(weapon.about, [x + 26.0, y + 14.0], 0xFF_9E9E9E, false);
            cards.push((r, i));
        }

        let hint = "ЛКМ — выбрать   •   Esc — закрыть   •   ПКМ пультом — пуск";
        let hw = draw.text_width(hint);
        draw.text(hint, [(w - hw) / 2.0, h - 16.0], 0xFF_BDBDBD, false);
    }

    pub fn click(
        &self,
        host: &dyn ClientHost,
        event: &mut ScreenMouseButtonPressedPreEvent<'_>,
    ) -> Flow {
        if host.mod_screen() != Some(SCREEN) {
            return Flow::Continue;
        }
        let at = event.screen.mouse;
        let hit = self
            .cards
            .lock()
            .unwrap()
            .iter()
            .find(|(r, _)| inside(*r, at))
            .map(|(_, i)| *i);
        if let Some(index) = hit {
            crate::select(index);
            self.close.store(true, Ordering::Relaxed);
        }
        Flow::Cancel
    }
}
