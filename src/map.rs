use bevy::prelude::*;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize, Resource)]
pub struct MapConfig {
    pub name: String,
    pub map: MapSettings,
    pub ground: GroundSettings,
    pub walls: WallSettings,
    pub trees: TreeSettings,
    pub structures: Vec<StructurePlacement>,
    pub lighting: LightingSettings,
    pub player: PlayerSettings,
    pub zombies: ZombieSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MapSettings {
    pub size: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroundSettings {
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub struct WallSettings {
    pub height: f32,
    pub thickness: f32,
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub struct TreeSettings {
    pub placement: TreePlacement,
    pub trunk: TrunkSettings,
    pub canopy: CanopySettings,
    pub collision_radius: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub enum TreePlacement {
    Random {
        count: usize,
        min_spacing: f32,
        clear_radius: f32,
    },
    Fixed(Vec<[f32; 2]>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrunkSettings {
    pub size: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub struct CanopySettings {
    pub size: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Debug, Clone, Deserialize)]
pub enum StructureType {
    House,
    Hut,
    Castle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StructurePlacement {
    pub kind: StructureType,
    pub position: [f32; 2],
}

#[derive(Debug, Clone, Deserialize)]
pub struct LightingSettings {
    pub sun_illuminance: f32,
    pub sun_angle: [f32; 3],
    pub ambient_brightness: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlayerSettings {
    pub spawn: [f32; 3],
    pub health: f32,
    pub ammo: u32,
    pub speed: f32,
    pub sprint_speed: f32,
}

#[derive(Debug, Clone, Deserialize)]
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
                clear_radius: 10.0,
            },
            trunk: TrunkSettings { size: [0.8, 4.0, 0.8], color: [0.45, 0.25, 0.1] },
            canopy: CanopySettings { size: [2.5, 2.5, 2.5], color: [0.1, 0.45, 0.1] },
            collision_radius: 1.2,
        },
        structures: vec![StructurePlacement { kind: StructureType::House, position: [0.0, 0.0] }],
        lighting: LightingSettings {
            sun_illuminance: 10000.0,
            sun_angle: [-0.8, 0.4, 0.0],
            ambient_brightness: 300.0,
        },
        player: PlayerSettings {
            spawn: [0.0, 0.9, 7.0],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_half() {
        let config = default_map_config();
        assert_eq!(config.map_half(), 40.0);
    }

    #[test]
    fn test_map_half_custom_size() {
        let mut config = default_map_config();
        config.map.size = 100.0;
        assert_eq!(config.map_half(), 50.0);
    }

    #[test]
    fn test_default_map_config_name() {
        let config = default_map_config();
        assert_eq!(config.name, "Forest");
    }

    #[test]
    fn test_default_map_config_player_settings() {
        let config = default_map_config();
        assert_eq!(config.player.health, 100.0);
        assert_eq!(config.player.ammo, 100);
        assert_eq!(config.player.speed, 5.0);
        assert_eq!(config.player.sprint_speed, 9.0);
    }

    #[test]
    fn test_default_map_config_zombie_settings() {
        let config = default_map_config();
        assert_eq!(config.zombies.spawn_interval, 3.0);
        assert_eq!(config.zombies.base_speed, 4.5);
        assert_eq!(config.zombies.base_move_chance, 0.6);
    }

    #[test]
    fn test_default_map_config_has_structures() {
        let config = default_map_config();
        assert_eq!(config.structures.len(), 1);
        assert!(matches!(config.structures[0].kind, StructureType::House));
    }

    #[test]
    fn test_default_map_config_tree_placement_is_random() {
        let config = default_map_config();
        assert!(matches!(config.trees.placement, TreePlacement::Random { count: 50, .. }));
    }

    #[test]
    fn test_load_map_config_from_ron() {
        let ron_str = r#"(
            name: "Test",
            map: (size: 60.0),
            ground: (color: (0.1, 0.2, 0.3)),
            walls: (height: 3.0, thickness: 0.5, color: (0.4, 0.4, 0.4)),
            trees: (
                placement: Fixed([]),
                trunk: (size: (0.5, 3.0, 0.5), color: (0.3, 0.2, 0.1)),
                canopy: (size: (2.0, 2.0, 2.0), color: (0.1, 0.4, 0.1)),
                collision_radius: 1.0,
            ),
            structures: [],
            lighting: (sun_illuminance: 8000.0, sun_angle: (-0.5, 0.3, 0.0), ambient_brightness: 200.0),
            player: (spawn: (0.0, 0.9, 5.0), health: 80.0, ammo: 50, speed: 4.0, sprint_speed: 8.0),
            zombies: (spawn_interval: 2.0, base_speed: 3.0, speed_per_kill: 0.1, max_speed_bonus: 4.0, base_move_chance: 0.5, move_chance_per_kill: 0.01, max_move_chance_bonus: 0.3),
        )"#;
        let config: MapConfig = ron::from_str(ron_str).unwrap();
        assert_eq!(config.name, "Test");
        assert_eq!(config.map.size, 60.0);
        assert_eq!(config.map_half(), 30.0);
        assert_eq!(config.player.health, 80.0);
        assert_eq!(config.player.ammo, 50);
    }

    #[test]
    fn test_load_map_config_missing_file_panics() {
        let result = std::panic::catch_unwind(|| {
            load_map_config("nonexistent_file.ron");
        });
        assert!(result.is_err());
    }
}
