//! Каталог оружия ядерного пульта.
//!
//! Новое оружие = новая запись в [`WEAPONS`]. Меню, сеть и взрывы подхватят её сами.

/// Как оружие добирается до цели.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// Ракета падает с неба вертикально.
    Rocket,
    /// Метеорит летит под углом, с огненным хвостом.
    Meteor,
    /// Луч с орбиты: бьёт сразу и прожигает мир до самого низа.
    OrbitalBeam,
}

/// Одно оружие.
#[derive(Clone, Copy, Debug)]
pub struct Weapon {
    /// Код, как на табло пульта.
    pub code: &'static str,
    /// Название в меню.
    pub name: &'static str,
    /// Короткое описание в меню.
    pub about: &'static str,
    /// Предмет-иконка в меню.
    pub icon: &'static str,
    /// Цвет акцента в меню (ARGB).
    pub color: u32,
    /// Радиус воронки в блоках.
    pub radius: f64,
    /// На сколько блоков за тик растёт ударная волна.
    pub wave_speed: f64,
    /// Способ доставки.
    pub delivery: Delivery,
    /// Скорость полёта, блоков за тик.
    pub flight_speed: f64,
    /// Рисовать ли ядерный гриб.
    pub mushroom: bool,
    /// Сила тряски камеры (0 = нет).
    pub shake: f32,
}

pub const WEAPONS: &[Weapon] = &[
    Weapon {
        code: "NK-01",
        name: "Фугасная ракета",
        about: "Обычная ракета для разминки",
        icon: "minecraft:tnt",
        color: 0xFF_F2C94C,
        radius: 6.0,
        wave_speed: 2.0,
        delivery: Delivery::Rocket,
        flight_speed: 3.0,
        mushroom: false,
        shake: 0.6,
    },
    Weapon {
        code: "NK-02",
        name: "Тактический заряд",
        about: "Маленькая ядерная боеголовка",
        icon: "minecraft:fire_charge",
        color: 0xFF_F2994A,
        radius: 14.0,
        wave_speed: 1.5,
        delivery: Delivery::Rocket,
        flight_speed: 2.5,
        mushroom: true,
        shake: 1.5,
    },
    Weapon {
        code: "NK-03",
        name: "Ядерная бомба",
        about: "Классика: вспышка, волна и гриб",
        icon: "minecraft:nether_star",
        color: 0xFF_EB5757,
        radius: 30.0,
        wave_speed: 1.2,
        delivery: Delivery::Rocket,
        flight_speed: 2.0,
        mushroom: true,
        shake: 3.0,
    },
    Weapon {
        code: "NK-04",
        name: "Царь-бомба",
        about: "Самое мощное, что есть в пульте",
        icon: "minecraft:dragon_egg",
        color: 0xFF_B71C1C,
        radius: 55.0,
        wave_speed: 1.0,
        delivery: Delivery::Rocket,
        flight_speed: 1.5,
        mushroom: true,
        shake: 5.0,
    },
    Weapon {
        code: "NK-05",
        name: "Метеорит",
        about: "Камень из космоса, раскалённая воронка",
        icon: "minecraft:magma_block",
        color: 0xFF_BB6BD9,
        radius: 20.0,
        wave_speed: 2.0,
        delivery: Delivery::Meteor,
        flight_speed: 4.0,
        mushroom: false,
        shake: 2.5,
    },
    Weapon {
        code: "NK-06",
        name: "Орбитальный луч",
        about: "Прожигает мир до бедрока",
        icon: "minecraft:end_rod",
        color: 0xFF_56CCF2,
        radius: 7.0,
        wave_speed: 1.0,
        delivery: Delivery::OrbitalBeam,
        flight_speed: 0.0,
        mushroom: false,
        shake: 1.0,
    },
];

/// Оружие по номеру; неверный номер даёт первое.
pub fn weapon(index: usize) -> &'static Weapon {
    WEAPONS.get(index).unwrap_or(&WEAPONS[0])
}
