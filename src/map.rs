use bevy::prelude::*;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Resource)]
pub struct MapConfig {
    pub name: String,
    pub map: MapSettings,
    pub ground: GroundSettings,
    pub walls: WallSettings,
    pub trees: TreeSettings,
    pub lighting: LightingSettings,
    pub player: PlayerSettings,
    pub zombies: ZombieSettings,
}

#[derive(Debug, Deserialize)]
pub struct MapSettings {
    pub size: f32,
}

#[derive(Debug, Deserialize)]
pub struct GroundSettings {
    pub color: [f32; 3],
}

#[derive(Debug, Deserialize)]
pub struct WallSettings {
    pub height: f32,
    pub thickness: f32,
    pub color: [f32; 3],
}

#[derive(Debug, Deserialize)]
pub struct TreeSettings {
    pub placement: TreePlacement,
    pub trunk: TrunkSettings,
    pub canopy: CanopySettings,
    pub collision_radius: f32,
}

#[derive(Debug, Deserialize)]
pub enum TreePlacement {
    Random {
        count: usize,
        min_spacing: f32,
        clear_radius: f32,
    },
    Fixed(Vec<[f32; 2]>),
}

#[derive(Debug, Deserialize)]
pub struct TrunkSettings {
    pub size: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Debug, Deserialize)]
pub struct CanopySettings {
    pub size: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Debug, Deserialize)]
pub struct LightingSettings {
    pub sun_illuminance: f32,
    pub sun_angle: [f32; 3],
    pub ambient_brightness: f32,
}

#[derive(Debug, Deserialize)]
pub struct PlayerSettings {
    pub spawn: [f32; 3],
    pub health: f32,
    pub ammo: u32,
    pub speed: f32,
    pub sprint_speed: f32,
}

#[derive(Debug, Deserialize)]
pub struct ZombieSettings {
    pub spawn_interval: f32,
    pub base_speed: f32,
    pub speed_per_kill: f32,
    pub max_speed_bonus: f32,
    pub base_move_chance: f32,
    pub move_chance_per_kill: f32,
    pub max_move_chance_bonus: f32,
}

impl MapConfig {
    pub fn map_half(&self) -> f32 {
        self.map.size / 2.0
    }
}

pub fn load_map_config(path: &str) -> MapConfig {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read map file '{}': {}", path, e));
    ron::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse map file '{}': {}", path, e))
}

pub fn default_map_config() -> MapConfig {
    MapConfig {
        name: "Forest".to_string(),
        map: MapSettings { size: 80.0 },
        ground: GroundSettings { color: [0.2, 0.6, 0.15] },
        walls: WallSettings { height: 4.0, thickness: 1.0, color: [0.35, 0.35, 0.35] },
        trees: TreeSettings {
            placement: TreePlacement::Random {
                count: 50,
                min_spacing: 3.5,
                clear_radius: 6.0,
            },
            trunk: TrunkSettings { size: [0.8, 4.0, 0.8], color: [0.45, 0.25, 0.1] },
            canopy: CanopySettings { size: [2.5, 2.5, 2.5], color: [0.1, 0.45, 0.1] },
            collision_radius: 1.2,
        },
        lighting: LightingSettings {
            sun_illuminance: 10000.0,
            sun_angle: [-0.8, 0.4, 0.0],
            ambient_brightness: 300.0,
        },
        player: PlayerSettings {
            spawn: [0.0, 0.9, 0.0],
            health: 100.0,
            ammo: 100,
            speed: 5.0,
            sprint_speed: 9.0,
        },
        zombies: ZombieSettings {
            spawn_interval: 3.0,
            base_speed: 4.5,
            speed_per_kill: 0.08,
            max_speed_bonus: 5.0,
            base_move_chance: 0.6,
            move_chance_per_kill: 0.005,
            max_move_chance_bonus: 0.35,
        },
    }
}
