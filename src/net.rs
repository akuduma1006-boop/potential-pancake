//! Сеть: клиент → сервер `Fire`, сервер → клиенты `Boom`.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use mod_api::*;

/// Клиент просит выстрелить оружием `weapon` туда, куда смотрит игрок.
#[derive(PayloadCodec)]
pub struct Fire {
    pub weapon: VarInt,
}

/// Сервер сообщает клиенту о взрыве: для тряски камеры.
#[derive(PayloadCodec)]
pub struct Boom {
    /// Сила тряски, уже с учётом расстояния до игрока.
    pub strength: f32,
    /// Длительность в тиках.
    pub ticks: VarInt,
}

static FIRE: OnceLock<ToServer<Fire>> = OnceLock::new();
static BOOM: OnceLock<ToClient<Boom>> = OnceLock::new();

/// Запросы на выстрел, пришедшие из сети; сервер разбирает их в своём тике.
static PENDING: Mutex<Vec<(PlayerRef, usize)>> = Mutex::new(Vec::new());

pub fn register(registrar: &mut dyn Registrar) {
    let mut payloads = registrar.payload_handlers();
    let mut view = payloads.registrar("1");
    let fire =
        view.play_to_server_typed::<Fire>(PayloadId("nukecraft:fire".into()), |value, cx| {
            let player = cx.player().map_err(|e| format!("{e:?}"))?;
            let weapon = usize::try_from(value.weapon.0).map_err(|_| "bad weapon".to_string())?;
            PENDING.lock().unwrap().push((player, weapon));
            Ok(())
        });
    let boom = view.play_to_client_typed::<Boom>(PayloadId("nukecraft:boom".into()), |value, _| {
        start_shake(value.strength, value.ticks.0);
        Ok(())
    });
    let _ = FIRE.set(fire);
    let _ = BOOM.set(boom);
}

/// Клиент: отправить запрос на выстрел.
pub fn send_fire(host: &mut dyn ClientHost, weapon: usize) {
    if let Some(token) = FIRE.get() {
        let _ = token.send(
            host,
            &Fire {
                weapon: VarInt(weapon as i32),
            },
        );
    }
}

/// Сервер: забрать накопившиеся запросы.
pub fn take_pending() -> Vec<(PlayerRef, usize)> {
    std::mem::take(&mut *PENDING.lock().unwrap())
}

/// Сервер: сказать игроку, что рядом бахнуло.
pub fn send_boom(host: &mut dyn ServerHost, player: PlayerRef, strength: f32, ticks: i32) {
    if let Some(token) = BOOM.get() {
        let _ = token.send(
            host,
            player,
            &Boom {
                strength,
                ticks: VarInt(ticks),
            },
        );
    }
}

// ───────────────────────────── тряска камеры ─────────────────────────────

struct Shake {
    strength: f32,
    started: Instant,
    seconds: f32,
}

static SHAKE: Mutex<Option<Shake>> = Mutex::new(None);

fn start_shake(strength: f32, ticks: i32) {
    let mut shake = SHAKE.lock().unwrap();
    // Более сильный взрыв перебивает слабый.
    let current = shake.as_ref().map_or(0.0, |s| s.strength * remaining(s));
    if strength >= current {
        *shake = Some(Shake {
            strength,
            started: Instant::now(),
            seconds: ticks.max(1) as f32 / 20.0,
        });
    }
}

/// Сколько осталось от тряски: 1.0 в начале, 0.0 в конце.
fn remaining(s: &Shake) -> f32 {
    (1.0 - s.started.elapsed().as_secs_f32() / s.seconds).max(0.0)
}

pub fn apply_shake(_host: &dyn ClientHost, event: &mut CameraShakeEvent<'_>) {
    let shake = SHAKE.lock().unwrap();
    let Some(s) = shake.as_ref() else { return };
    let left = remaining(s);
    if left <= 0.0 {
        return;
    }
    let t = s.started.elapsed().as_secs_f32();
    let power = s.strength * left * left;
    event.shake.pitch += (t * 47.0).sin() * power;
    event.shake.yaw += (t * 39.0).cos() * power * 0.7;
    event.shake.roll += (t * 31.0).sin() * power * 0.5;
}
