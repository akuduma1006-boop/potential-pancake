//! Сервер: прицел, полёт ракеты, взрыв волной, обугливание и ядерный гриб.
//!
//! Блоки вырезаются через `edit_blocks` колоннами (`BlockRun`): за тик удаляется слой шара
//! между прошлым и новым радиусом волны. Так даже Царь-бомба (R = 55) не вешает сервер.

use std::f64::consts::TAU;
use std::sync::Mutex;

use mod_api::*;

use crate::net;
use crate::weapons::{Delivery, Weapon, weapon};

/// Дальность прицела в блоках.
const AIM_DISTANCE: f64 = 400.0;
/// Высота, с которой падает ракета (над точкой удара).
const DROP_HEIGHT: f64 = 140.0;
/// Сколько тиков висит ядерный гриб.
const MUSHROOM_TICKS: u32 = 260;
/// Перезарядка пульта у игрока, в тиках.
const COOLDOWN_TICKS: u64 = 10;

// ───────────────────────────── ключи реестров ─────────────────────────────

/// Частицы, которые мы рисуем.
pub struct Particles {
    flame: ParticleKey,
    smoke: ParticleKey,
    large_smoke: ParticleKey,
    campfire: ParticleKey,
    cloud: ParticleKey,
    lava: ParticleKey,
    flash: ParticleKey,
    explosion: ParticleKey,
    end_rod: ParticleKey,
    dust: ParticleKey,
}

impl Particles {
    pub fn register(r: &mut dyn Registrar) -> Self {
        Self {
            flame: r.particle_key("minecraft:flame"),
            smoke: r.particle_key("minecraft:smoke"),
            large_smoke: r.particle_key("minecraft:large_smoke"),
            campfire: r.particle_key("minecraft:campfire_signal_smoke"),
            cloud: r.particle_key("minecraft:cloud"),
            lava: r.particle_key("minecraft:lava"),
            flash: r.particle_key("minecraft:flash"),
            explosion: r.particle_key("minecraft:explosion_emitter"),
            end_rod: r.particle_key("minecraft:end_rod"),
            dust: r.particle_key("minecraft:dust"),
        }
    }
}

/// Блоки для воронок.
pub struct BlockKeys {
    air: BlockKey,
    coarse_dirt: BlockKey,
    blackstone: BlockKey,
    basalt: BlockKey,
    magma: BlockKey,
    obsidian: BlockKey,
    netherrack: BlockKey,
    fire: BlockKey,
}

impl BlockKeys {
    pub fn register(r: &mut dyn Registrar) -> Self {
        Self {
            air: r.block_key("minecraft:air"),
            coarse_dirt: r.block_key("minecraft:coarse_dirt"),
            blackstone: r.block_key("minecraft:blackstone"),
            basalt: r.block_key("minecraft:basalt"),
            magma: r.block_key("minecraft:magma_block"),
            obsidian: r.block_key("minecraft:obsidian"),
            netherrack: r.block_key("minecraft:netherrack"),
            fire: r.block_key("minecraft:fire"),
        }
    }
}

// ───────────────────────────── состояние ─────────────────────────────

enum Phase {
    /// Ракета летит: текущая точка и направление.
    Flying { pos: [f64; 3], dir: [f64; 3] },
    /// Волна растёт: текущий радиус и номер тика.
    Exploding { radius: f64, age: u32 },
    /// Висит гриб.
    Mushroom { age: u32 },
}

struct Strike {
    level: LevelRef,
    weapon: &'static Weapon,
    target: [f64; 3],
    phase: Phase,
}

struct State {
    strikes: Vec<Strike>,
    /// Последний выстрел каждого игрока (номер тика).
    last_fire: Vec<(PlayerRef, u64)>,
    tick: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    strikes: Vec::new(),
    last_fire: Vec::new(),
    tick: 0,
});

// ───────────────────────────── маленькая математика ─────────────────────────────

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn length(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let l = length(a);
    if l < 1e-9 {
        [0.0, -1.0, 0.0]
    } else {
        scale(a, 1.0 / l)
    }
}

/// Направление взгляда из углов Minecraft (yaw, pitch в градусах).
fn look_dir(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    [
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    ]
}

/// Детерминированный шум 0..1 для неровных краёв воронки.
fn hash01(x: i32, z: i32) -> f64 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (z as u32).wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 16;
    f64::from(h) / f64::from(u32::MAX)
}

/// Множитель радиуса для колонки: делает край рваным.
fn column_factor(x: i32, z: i32) -> f64 {
    0.88 + 0.17 * hash01(x.div_euclid(2), z.div_euclid(2))
}

// ───────────────────────────── вывод: частицы, звуки ─────────────────────────────

fn particles(
    host: &mut dyn ServerHost,
    level: LevelRef,
    options: &ParticleOptions,
    at: [f64; 3],
    spread: [f64; 3],
    speed: f64,
    count: i32,
) {
    let _ = level.send_particles(host, options, None, at, spread, speed, count);
}

fn sound(
    host: &mut dyn ServerHost,
    level: LevelRef,
    name: &str,
    at: [f64; 3],
    volume: f32,
    pitch: f32,
) {
    let _ = level.play_sound(host, name, at, SoundSource::Master, volume, pitch);
}

fn state_of(host: &mut dyn ServerHost, key: BlockKey) -> Option<BlockState> {
    host.block_state_default(key)
}

// ───────────────────────────── тик сервера ─────────────────────────────

pub fn tick(host: &mut dyn ServerHost) {
    let mut state = STATE.lock().unwrap();
    state.tick += 1;
    let now = state.tick;

    for (player, weapon_index) in net::take_pending() {
        let ready = state
            .last_fire
            .iter()
            .find(|(p, _)| *p == player)
            .is_none_or(|(_, t)| now >= t + COOLDOWN_TICKS);
        if !ready {
            continue;
        }
        state.last_fire.retain(|(p, _)| *p != player);
        state.last_fire.push((player, now));
        if let Some(strike) = launch(host, player, weapon(weapon_index)) {
            state.strikes.push(strike);
        }
    }

    let mut i = 0;
    while i < state.strikes.len() {
        if advance(host, &mut state.strikes[i]) {
            i += 1;
        } else {
            state.strikes.swap_remove(i);
        }
    }
}

/// Найти точку прицела и создать удар.
fn launch(host: &mut dyn ServerHost, player: PlayerRef, w: &'static Weapon) -> Option<Strike> {
    let entity = player.entity();
    let level = entity.level(host)?;
    let eye = entity.eye_position(host)?;
    let [yaw, pitch] = entity.rotation(host)?;
    let dir = look_dir(yaw, pitch);
    let far = add(eye, scale(dir, AIM_DISTANCE));
    let hit = level.clip(host, eye, far, ClipBlock::Outline, ClipFluid::None);

    let Some(hit) = hit.filter(|h| !h.miss) else {
        let _ = player.action_bar(
            host,
            Text::literal("✖ Цель не найдена: наведи прицел на землю"),
        );
        return None;
    };
    let target = hit.location;

    let _ = player.action_bar(
        host,
        Text::literal(format!(
            "☢ {} {} → {:.0} {:.0} {:.0}",
            w.code, w.name, target[0], target[1], target[2]
        )),
    );

    let phase = match w.delivery {
        Delivery::Rocket => {
            sound(
                host,
                level,
                "minecraft:entity.firework_rocket.launch",
                eye,
                2.0,
                0.5,
            );
            Phase::Flying {
                pos: add(target, [0.0, DROP_HEIGHT, 0.0]),
                dir: [0.0, -1.0, 0.0],
            }
        }
        Delivery::Meteor => {
            sound(
                host,
                level,
                "minecraft:entity.ender_dragon.growl",
                eye,
                2.0,
                0.6,
            );
            let angle = hash01(target[0] as i32, target[2] as i32) * TAU;
            let start = add(
                target,
                [angle.cos() * 160.0, DROP_HEIGHT, angle.sin() * 160.0],
            );
            Phase::Flying {
                pos: start,
                dir: normalize(sub(target, start)),
            }
        }
        Delivery::OrbitalBeam => {
            sound(
                host,
                level,
                "minecraft:block.beacon.activate",
                target,
                8.0,
                0.5,
            );
            Phase::Exploding {
                radius: 0.0,
                age: 0,
            }
        }
    };
    Some(Strike {
        level,
        weapon: w,
        target,
        phase,
    })
}

/// Один тик удара. `false` — удар закончился.
fn advance(host: &mut dyn ServerHost, s: &mut Strike) -> bool {
    let p = &crate::keys().particles;
    match &mut s.phase {
        Phase::Flying { pos, dir } => {
            let step = s.weapon.flight_speed;
            let next = add(*pos, scale(*dir, step));
            let tail = sub(*pos, scale(*dir, 1.5));

            // Сама ракета: яркое ядро и хвост огня и дыма.
            particles(
                host,
                s.level,
                &ParticleOptions::simple(p.end_rod),
                *pos,
                [0.1; 3],
                0.0,
                4,
            );
            particles(
                host,
                s.level,
                &ParticleOptions::simple(p.flame),
                tail,
                [0.25; 3],
                0.04,
                12,
            );
            particles(
                host,
                s.level,
                &ParticleOptions::simple(p.large_smoke),
                tail,
                [0.4; 3],
                0.02,
                6,
            );
            if s.weapon.delivery == Delivery::Meteor {
                particles(
                    host,
                    s.level,
                    &ParticleOptions::simple(p.lava),
                    tail,
                    [0.8; 3],
                    0.0,
                    4,
                );
            }

            if length(sub(*pos, s.target)) <= step || next[1] <= s.target[1] {
                s.phase = Phase::Exploding {
                    radius: 0.0,
                    age: 0,
                };
            } else {
                *pos = next;
            }
            true
        }
        Phase::Exploding { radius, age } => {
            let first = *age == 0;
            *age += 1;
            let prev = *radius;
            let cur = (prev + s.weapon.wave_speed).min(s.weapon.radius);
            *radius = cur;
            if first {
                detonate(host, s);
            }
            carve(host, s, prev, cur);
            shock_ring(host, s, cur);

            if cur >= s.weapon.radius {
                scorch(host, s);
                if s.weapon.mushroom {
                    s.phase = Phase::Mushroom { age: 0 };
                    return true;
                }
                return false;
            }
            true
        }
        Phase::Mushroom { age } => {
            *age += 1;
            let age = *age;
            mushroom(host, s, age);
            age < MUSHROOM_TICKS
        }
    }
}

/// Миг взрыва: ванильный взрыв в центре (урон, звук, искры), вспышка, тряска у игроков.
fn detonate(host: &mut dyn ServerHost, s: &Strike) {
    let p = &crate::keys().particles;
    let w = s.weapon;
    let c = s.target;

    // Ванильный взрыв делает урон и отбрасывание; блоки режем сами.
    let settings = ExplosionSettings::default();
    let power = (w.radius as f32 * 0.5).clamp(3.0, 16.0);
    let _ = s.level.explode(
        host,
        None,
        c,
        power,
        w.mushroom,
        ExplosionInteraction::None,
        settings,
    );

    particles(
        host,
        s.level,
        &ParticleOptions::simple(p.flash),
        add(c, [0.0, 2.0, 0.0]),
        [0.0; 3],
        0.0,
        1,
    );
    particles(
        host,
        s.level,
        &ParticleOptions::simple(p.explosion),
        c,
        [w.radius * 0.3; 3],
        0.0,
        8,
    );
    let volume = (w.radius / 2.0) as f32;
    let pitch = (1.2 - w.radius / 60.0).max(0.3) as f32;
    sound(
        host,
        s.level,
        "minecraft:entity.generic.explode",
        c,
        volume,
        pitch,
    );
    if w.mushroom || w.radius >= 20.0 {
        sound(
            host,
            s.level,
            "minecraft:entity.lightning_bolt.thunder",
            c,
            volume,
            0.5,
        );
        let _ = s
            .level
            .strike_lightning(host, add(c, [0.0, 1.0, 0.0]), true);
    }

    // Тряска камеры у всех, кто достаточно близко.
    if w.shake > 0.0 {
        let reach = w.radius * 8.0;
        for player in s.level.online_players(host).unwrap_or_default() {
            let Some(pos) = player.entity().position(host) else {
                continue;
            };
            let d = length(sub(pos, c));
            if d < reach {
                let k = (1.0 - d / reach) as f32;
                net::send_boom(host, player, w.shake * k, (20.0 + w.radius) as i32);
            }
        }
    }
}

/// Колонны, которые надо вырезать, когда волна растёт с радиуса `prev` до `cur`.
///
/// Блок колонны (x, z) на высоте y входит в шар радиуса `r`, если
/// `dx² + dz² ≤ (r·f)²` и `|dy| ≤ ⌊√((r·f)² − dx² − dz²)⌋`, где `f` — шум края колонны.
/// Для каждого шага отдаём только новые блоки: вместе шаги дают весь шар ровно один раз.
/// `beam`: вместо шара — столб от `floor` до 60 блоков над центром.
fn shell_runs(
    center: [i32; 3],
    prev: f64,
    cur: f64,
    floor: i32,
    max_y: i32,
    beam: bool,
) -> Vec<BlockRun> {
    let [cx, cy, cz] = center;
    let reach = (cur * 1.06).ceil() as i32;
    let mut runs = Vec::new();

    for dx in -reach..=reach {
        for dz in -reach..=reach {
            let (x, z) = (cx + dx, cz + dz);
            let f = column_factor(x, z);
            let (rp, rc) = (prev * f, cur * f);
            let d2 = f64::from(dx * dx + dz * dz);
            if d2 > rc * rc {
                continue;
            }
            // Колонна попала в шар впервые на этом шаге?
            let new_column = prev == 0.0 || d2 > rp * rp;
            let mut run = |from: i32, to: i32| {
                let (from, to) = (from.max(floor), to.min(max_y));
                if to >= from {
                    runs.push(BlockRun {
                        x,
                        z,
                        y: from,
                        height: (to - from + 1) as u32,
                        surface: None,
                        block: 0,
                        only: None,
                    });
                }
            };

            if beam {
                if new_column {
                    run(floor, cy + 60);
                }
                continue;
            }

            let hi = (rc * rc - d2).sqrt().floor() as i32;
            if new_column {
                run(cy - hi, cy + hi);
            } else {
                // Середина уже вырезана прошлыми шагами: добавляем сверху и снизу.
                let lo = (rp * rp - d2).sqrt().floor() as i32;
                if hi > lo {
                    run(cy + lo + 1, cy + hi);
                    run(cy - hi, cy - lo - 1);
                }
            }
        }
    }
    runs
}

/// Удалить слой шара (или цилиндра для луча) между радиусами `prev` и `cur`.
fn carve(host: &mut dyn ServerHost, s: &Strike, prev: f64, cur: f64) {
    let Some(air) = state_of(host, crate::keys().blocks.air) else {
        return;
    };
    let Some((min_y, height)) = s.level.height(host) else {
        return;
    };
    let max_y = min_y + height - 1;
    let floor = min_y + 1; // самый нижний слой не трогаем

    let center = [
        s.target[0].floor() as i32,
        s.target[1].floor() as i32,
        s.target[2].floor() as i32,
    ];
    let beam = s.weapon.delivery == Delivery::OrbitalBeam;
    let runs = shell_runs(center, prev, cur, floor, max_y, beam);

    if runs.is_empty() {
        return;
    }
    let edit = BlockEdit {
        palette: vec![air],
        filters: Vec::new(),
        runs,
    };
    let bulk = match s.level.dimension().id(host) {
        Some(dimension) => host.edit_blocks(&dimension, &edit),
        None => Err("нет имени измерения".into()),
    };
    if let Err(why) = bulk {
        // Запасной путь: по одному блоку (медленнее, но работает в любом уровне).
        log_once(&why);
        for run in &edit.runs {
            for y in run.y..run.y + run.height as i32 {
                let _ = s
                    .level
                    .set_block(host, [run.x, y, run.z], air, BlockUpdateFlags::CLIENTS);
            }
        }
    }
}

/// Пишет в лог причину отказа `edit_blocks`, но только один раз.
fn log_once(why: &str) {
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!("NukeCraft: edit_blocks отказал ({why}), ставлю блоки по одному");
    }
}

/// Видимое кольцо ударной волны по краю.
fn shock_ring(host: &mut dyn ServerHost, s: &Strike, r: f64) {
    let p = &crate::keys().particles;
    let points = (r * 2.0) as i32 + 8;
    for i in 0..points {
        let a = f64::from(i) / f64::from(points) * TAU;
        let at = add(s.target, [a.cos() * r, 0.5, a.sin() * r]);
        particles(
            host,
            s.level,
            &ParticleOptions::simple(p.cloud),
            at,
            [0.3; 3],
            0.05,
            2,
        );
    }
}

/// Обуглить поверхность в воронке и вокруг неё.
fn scorch(host: &mut dyn ServerHost, s: &Strike) {
    let k = &crate::keys().blocks;
    let w = s.weapon;
    let pick: &[BlockKey] = match w.delivery {
        Delivery::Meteor => &[k.magma, k.magma, k.obsidian, k.blackstone],
        Delivery::OrbitalBeam => &[k.magma, k.basalt, k.basalt],
        Delivery::Rocket => &[k.coarse_dirt, k.blackstone, k.basalt, k.netherrack],
    };
    let mut palette: Vec<BlockState> = pick.iter().filter_map(|b| state_of(host, *b)).collect();
    let Some(fire) = state_of(host, k.fire) else {
        return;
    };
    if palette.is_empty() {
        return;
    }
    let fire_index = palette.len() as u32;
    palette.push(fire);

    let cx = s.target[0].floor() as i32;
    let cz = s.target[2].floor() as i32;
    let r = (w.radius * 1.3).ceil() as i32 + 2;
    let mut runs = Vec::new();
    for dx in -r..=r {
        for dz in -r..=r {
            let d = f64::from(dx * dx + dz * dz).sqrt();
            let (x, z) = (cx + dx, cz + dz);
            let roll = hash01(x * 7 + 3, z * 13 + 1);
            // Чем дальше от центра, тем реже обугливание.
            if d > f64::from(r) || roll < d / f64::from(r) * 0.8 {
                continue;
            }
            let block = (hash01(x, z * 3) * (fire_index as f64)) as u32;
            // y = -1 от поверхности: верхний твёрдый блок колонны.
            runs.push(BlockRun {
                x,
                z,
                y: -1,
                height: 1,
                surface: Some(Heightmap::WorldSurface),
                block,
                only: None,
            });
            if w.mushroom && roll > 0.93 {
                runs.push(BlockRun {
                    x,
                    z,
                    y: 0,
                    height: 1,
                    surface: Some(Heightmap::WorldSurface),
                    block: fire_index,
                    only: None,
                });
            }
        }
    }
    let edit = BlockEdit {
        palette,
        filters: Vec::new(),
        runs,
    };
    if let Some(dimension) = s.level.dimension().id(host)
        && let Err(why) = host.edit_blocks(&dimension, &edit)
    {
        // Обугливание — только украшение: без массового редактирования просто пропускаем.
        log_once(&why);
    }
}

/// Ядерный гриб: ножка из дыма и огня, шляпка-огненный шар, кольцо у земли.
fn mushroom(host: &mut dyn ServerHost, s: &Strike, age: u32) {
    let p = &crate::keys().particles;
    let r = s.weapon.radius;
    let t = f64::from(age) / f64::from(MUSHROOM_TICKS);
    let grow = (t * 3.0).min(1.0);
    let height = r * 2.6 * grow + 6.0;
    let cap = r * (0.5 + 0.6 * grow);
    let base = s.target;
    let top = add(base, [0.0, height, 0.0]);

    // Ножка.
    for i in 0..10 {
        let at = add(base, [0.0, height * f64::from(i) / 10.0, 0.0]);
        let wdt = r * 0.18;
        particles(
            host,
            s.level,
            &ParticleOptions::simple(p.large_smoke),
            at,
            [wdt, height / 20.0, wdt],
            0.01,
            6,
        );
        if age < 80 {
            particles(
                host,
                s.level,
                &ParticleOptions::simple(p.flame),
                at,
                [wdt * 0.6, 1.0, wdt * 0.6],
                0.02,
                3,
            );
        }
    }

    // Шляпка: огненная, потом остывает до пепла.
    let spread = [cap * 0.7, cap * 0.25, cap * 0.7];
    let heat = (1.0 - t * 1.6).max(0.0) as f32;
    let ash = ParticleOptions::dust(p.dust, [0.25, 0.22, 0.2], 4.0);
    particles(host, s.level, &ash, top, spread, 0.0, 30);
    if heat > 0.0 {
        let fire = ParticleOptions::dust(p.dust, [1.0, 0.25 + 0.4 * heat, 0.05], 4.0);
        let under = sub(top, [0.0, cap * 0.2, 0.0]);
        particles(
            host,
            s.level,
            &fire,
            under,
            scale(spread, 0.8),
            0.0,
            (40.0 * heat) as i32 + 5,
        );
    }
    particles(
        host,
        s.level,
        &ParticleOptions::simple(p.campfire),
        top,
        spread,
        0.01,
        8,
    );

    // Кольцо у основания.
    for i in 0..24 {
        let a = f64::from(i) / 24.0 * TAU + t * 2.0;
        let rr = r * (0.8 + t);
        let at = add(base, [a.cos() * rr, 1.0, a.sin() * rr]);
        particles(
            host,
            s.level,
            &ParticleOptions::simple(p.smoke),
            at,
            [1.0; 3],
            0.01,
            2,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Все блоки из колонн; паника, если блок встретился дважды.
    fn blocks(runs: &[BlockRun], into: &mut HashSet<[i32; 3]>) {
        for r in runs {
            for y in r.y..r.y + r.height as i32 {
                assert!(
                    into.insert([r.x, y, r.z]),
                    "блок {:?} вырезан дважды",
                    [r.x, y, r.z]
                );
            }
        }
    }

    /// Шар радиуса `r` по тому же правилу, что в `shell_runs`, но напрямую.
    fn ball(center: [i32; 3], r: f64) -> HashSet<[i32; 3]> {
        let mut set = HashSet::new();
        let reach = (r * 1.06).ceil() as i32;
        for dx in -reach..=reach {
            for dz in -reach..=reach {
                let (x, z) = (center[0] + dx, center[2] + dz);
                let rr = r * column_factor(x, z);
                let d2 = f64::from(dx * dx + dz * dz);
                if d2 > rr * rr {
                    continue;
                }
                let hi = (rr * rr - d2).sqrt().floor() as i32;
                for dy in -hi..=hi {
                    set.insert([x, center[1] + dy, z]);
                }
            }
        }
        set
    }

    #[test]
    fn wave_steps_add_up_to_the_whole_ball_once() {
        for (radius, speed) in [(6.0, 2.0), (14.0, 1.5), (30.0, 1.2), (20.0, 2.0)] {
            let center = [10, 64, -7];
            let mut cut = HashSet::new();
            let mut r = 0.0;
            while r < radius {
                let next = f64::min(r + speed, radius);
                blocks(&shell_runs(center, r, next, -63, 319, false), &mut cut);
                r = next;
            }
            assert_eq!(cut, ball(center, radius), "радиус {radius}");
        }
    }

    #[test]
    fn floor_and_ceiling_are_respected() {
        let runs = shell_runs([0, -60, 0], 0.0, 10.0, -63, 319, false);
        assert!(runs.iter().all(|r| r.y >= -63));
        let runs = shell_runs([0, 315, 0], 0.0, 10.0, -63, 319, false);
        assert!(runs.iter().all(|r| r.y + r.height as i32 - 1 <= 319));
    }

    #[test]
    fn beam_cuts_each_column_once_down_to_the_floor() {
        let mut cut = HashSet::new();
        let mut r = 0.0;
        while r < 7.0 {
            let next = f64::min(r + 1.0, 7.0);
            blocks(&shell_runs([0, 70, 0], r, next, -63, 319, true), &mut cut);
            r = next;
        }
        assert!(cut.contains(&[0, -63, 0]) && cut.contains(&[0, 130, 0]));
        assert!(!cut.contains(&[0, -64, 0]));
    }

    #[test]
    fn look_direction_matches_minecraft_axes() {
        let close = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9);
        assert!(close(look_dir(0.0, 0.0), [0.0, 0.0, 1.0])); // юг: +Z
        assert!(close(look_dir(90.0, 0.0), [-1.0, 0.0, 0.0])); // запад: −X
        assert!(close(look_dir(-90.0, 0.0), [1.0, 0.0, 0.0])); // восток: +X
        assert!(close(look_dir(0.0, 90.0), [0.0, -1.0, 0.0])); // вниз
    }
}
