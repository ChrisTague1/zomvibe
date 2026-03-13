use bevy::{
    input::mouse::MouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};
use rand::Rng;
use std::collections::BinaryHeap;
use std::cmp::Ordering;

mod map;
use map::{MapConfig, TreePlacement, StructureType, load_map_config, default_map_config};

// ── App State ────────────────────────────────────────────────────────────────

#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
enum AppState {
    #[default]
    MapSelect,
    Playing,
}

struct MapOption {
    name: String,
    path: Option<String>,
}

#[derive(Resource)]
struct AvailableMaps(Vec<MapOption>);

#[derive(Component)]
struct MapSelectUI;

#[derive(Component)]
struct MapButton(usize);

#[derive(Component)]
struct MapExitButton;

const GRID_CELL: f32 = 1.0;
const PLAYER_RADIUS: f32 = 0.4;

fn scan_maps() -> Vec<MapOption> {
    let mut maps = Vec::new();
    if let Ok(entries) = std::fs::read_dir("maps") {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ron") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(config) = ron::from_str::<MapConfig>(&content) {
                        maps.push(MapOption {
                            name: config.name,
                            path: Some(path.to_string_lossy().to_string()),
                        });
                    }
                }
            }
        }
    }
    maps.sort_by(|a, b| a.name.cmp(&b.name));
    if maps.is_empty() {
        maps.push(MapOption {
            name: "Forest (Default)".to_string(),
            path: None,
        });
    }
    maps
}

fn main() {
    let maps = scan_maps();

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ZomVibe".to_string(),
                ..default()
            }),
            ..default()
        }))
        .init_state::<AppState>()
        .insert_resource(AvailableMaps(maps))
        .insert_resource(GameState::default())
        .insert_resource(TreePositions::default())
        .add_systems(OnEnter(AppState::MapSelect), setup_map_select)
        .add_systems(OnEnter(AppState::Playing), (snapshot_entities, setup_scene, setup_ui, grab_cursor).chain())
        .add_systems(OnExit(AppState::MapSelect), cleanup_map_select)
        .add_systems(OnExit(AppState::Playing), cleanup_playing)
        .add_systems(
            Update,
            (map_select_interaction, map_exit_interaction)
                .run_if(in_state(AppState::MapSelect)),
        )
        .add_systems(
            Update,
            (toggle_pause, pause_button_interaction)
                .run_if(in_state(AppState::Playing)),
        )
        .add_systems(
            Update,
            (
                player_look,
                player_move,
                reload,
                shoot,
                spawn_zombies,
                zombie_ai,
                bullet_movement,
                check_bullet_zombie_collision,
                melee_attack,
                melee_swing_animation,
                zombie_attack_player,
                update_health_ui,
                health_regen,
                damage_flash,
            )
                .run_if(in_state(AppState::Playing).and(game_active)),
        )
        .run();
}

// ── Components ──────────────────────────────────────────────────────────────

#[derive(Component)]
struct Player;

#[derive(Component)]
struct CameraAnchor;

#[derive(Component)]
struct Zombie {
    path: Vec<Vec2>,
    path_index: usize,
    repath_timer: Timer,
    idle: bool,
    idle_timer: Timer,
}

#[derive(Component)]
struct Wall;

#[derive(Component)]
struct Bullet {
    velocity: Vec3,
    lifetime: f32,
}

#[derive(Component)]
struct Gun;

#[derive(Component)]
struct HealthText;

#[derive(Component)]
struct KillText;

#[derive(Component)]
struct AmmoText;

#[derive(Component)]
struct ScoreText;

#[derive(Component)]
struct MeleeWeapon;

#[derive(Component)]
struct MeleeSwing {
    timer: Timer,
}

#[derive(Component)]
struct DamageFlash {
    timer: Timer,
}

#[derive(Component)]
struct DamageOverlay;

#[derive(Component)]
struct PauseMenu;

#[derive(Component)]
enum PauseButton {
    Resume,
    Exit,
}

// ── Resources ───────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
struct GameState {
    kills: u32,
    score: u32,
    ammo: u32,
    magazine: u32,
    reload_timer: Option<Timer>,
    health: f32,
    last_hit_time: f32,
    hit_count_in_window: u32,
    is_dead: bool,
    is_paused: bool,
    vertical_velocity: f32,
    is_grounded: bool,
    melee_cooldown: f32,
}

impl GameState {
    fn zombie_speed(&self, config: &MapConfig) -> f32 {
        config.zombies.base_speed + (self.kills as f32 * config.zombies.speed_per_kill).min(config.zombies.max_speed_bonus)
    }

    fn zombie_move_chance(&self, config: &MapConfig) -> f32 {
        config.zombies.base_move_chance + (self.kills as f32 * config.zombies.move_chance_per_kill).min(config.zombies.max_move_chance_bonus)
    }
}

fn game_active(game: Res<GameState>) -> bool {
    !game.is_paused
}

#[derive(Resource)]
struct ZombieSpawnTimer(Timer);

#[derive(Resource, Default)]
struct TreePositions(Vec<Vec2>);

struct FloorRect {
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
    y: f32,
}

struct WallRect {
    min_x: f32,
    max_x: f32,
    min_z: f32,
    max_z: f32,
    min_y: f32,
    max_y: f32,
}

#[derive(Resource, Default)]
struct FloorSurfaces(Vec<FloorRect>);

#[derive(Resource, Default)]
struct HouseWalls(Vec<WallRect>);

// ── A* Navigation Grid ──────────────────────────────────────────────────────

#[derive(Resource)]
struct NavGrid {
    blocked: Vec<bool>,
    map_half: f32,
    grid_size: usize,
}

impl NavGrid {
    fn new(map_half: f32, grid_size: usize) -> Self {
        Self {
            blocked: vec![false; grid_size * grid_size],
            map_half,
            grid_size,
        }
    }

    fn world_to_grid(&self, world_x: f32, world_z: f32) -> Option<(usize, usize)> {
        let gx = ((world_x + self.map_half) / GRID_CELL) as i32;
        let gz = ((world_z + self.map_half) / GRID_CELL) as i32;
        if gx >= 0 && gx < self.grid_size as i32 && gz >= 0 && gz < self.grid_size as i32 {
            Some((gx as usize, gz as usize))
        } else {
            None
        }
    }

    fn grid_to_world(&self, gx: usize, gz: usize) -> Vec2 {
        Vec2::new(
            gx as f32 * GRID_CELL - self.map_half + GRID_CELL * 0.5,
            gz as f32 * GRID_CELL - self.map_half + GRID_CELL * 0.5,
        )
    }

    fn idx(&self, gx: usize, gz: usize) -> usize {
        gz * self.grid_size + gx
    }

    fn is_blocked(&self, gx: usize, gz: usize) -> bool {
        self.blocked[self.idx(gx, gz)]
    }

    fn find_path(&self, start: Vec2, end: Vec2) -> Vec<Vec2> {
        let Some((sx, sz)) = self.world_to_grid(start.x, start.y) else {
            return vec![];
        };
        let Some((ex, ez)) = self.world_to_grid(end.x, end.y) else {
            return vec![];
        };

        // If goal is blocked, just go direct
        if self.is_blocked(ex, ez) {
            return vec![end];
        }

        let mut open = BinaryHeap::new();
        let total = self.grid_size * self.grid_size;
        let mut g_score = vec![f32::INFINITY; total];
        let mut came_from = vec![(usize::MAX, usize::MAX); total];
        let mut closed = vec![false; total];

        g_score[self.idx(sx, sz)] = 0.0;
        let h = heuristic(sx, sz, ex, ez);
        open.push(AStarNode { gx: sx, gz: sz, f: h });

        let neighbors: [(i32, i32, f32); 8] = [
            (-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
            (-1, -1, 1.414), (-1, 1, 1.414), (1, -1, 1.414), (1, 1, 1.414),
        ];

        let mut found = false;
        let mut iterations = 0;
        let max_iterations = 2000;

        while let Some(current) = open.pop() {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }

            let cx = current.gx;
            let cz = current.gz;

            if cx == ex && cz == ez {
                found = true;
                break;
            }

            let ci = self.idx(cx, cz);
            if closed[ci] {
                continue;
            }
            closed[ci] = true;

            for (dx, dz, cost) in &neighbors {
                let nx = cx as i32 + dx;
                let nz = cz as i32 + dz;
                if nx < 0 || nx >= self.grid_size as i32 || nz < 0 || nz >= self.grid_size as i32 {
                    continue;
                }
                let nx = nx as usize;
                let nz = nz as usize;
                let ni = self.idx(nx, nz);

                if closed[ni] || self.is_blocked(nx, nz) {
                    continue;
                }

                // For diagonal moves, check that both adjacent cardinal cells are free
                if *dx != 0 && *dz != 0 {
                    if self.is_blocked(cx, nz) || self.is_blocked(nx, cz) {
                        continue;
                    }
                }

                let new_g = g_score[ci] + cost;
                if new_g < g_score[ni] {
                    g_score[ni] = new_g;
                    came_from[ni] = (cx, cz);
                    let h = heuristic(nx, nz, ex, ez);
                    open.push(AStarNode { gx: nx, gz: nz, f: new_g + h });
                }
            }
        }

        if !found {
            // Fallback: direct line
            return vec![end];
        }

        // Reconstruct path
        let mut path_grid = Vec::new();
        let mut cur = (ex, ez);
        while cur != (sx, sz) {
            path_grid.push(cur);
            let ci = self.idx(cur.0, cur.1);
            let prev = came_from[ci];
            if prev == (usize::MAX, usize::MAX) {
                break;
            }
            cur = prev;
        }
        path_grid.reverse();

        // Convert to world coords
        path_grid.iter().map(|&(gx, gz)| self.grid_to_world(gx, gz)).collect()
    }
}

fn heuristic(ax: usize, az: usize, bx: usize, bz: usize) -> f32 {
    let dx = (ax as f32 - bx as f32).abs();
    let dz = (az as f32 - bz as f32).abs();
    // Octile distance
    let min = dx.min(dz);
    let max = dx.max(dz);
    min * 1.414 + (max - min)
}

#[derive(Clone, Debug)]
struct AStarNode {
    gx: usize,
    gz: usize,
    f: f32,
}

impl PartialEq for AStarNode {
    fn eq(&self, other: &Self) -> bool {
        self.gx == other.gx && self.gz == other.gz
    }
}

impl Eq for AStarNode {}

impl Ord for AStarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for AStarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// ── Map Selection ────────────────────────────────────────────────────────────

fn setup_map_select(mut commands: Commands, maps: Res<AvailableMaps>) {
    // UI camera for the menu screen (despawned on state exit)
    commands.spawn((MapSelectUI, Camera2d));

    commands
        .spawn((
            MapSelectUI,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.9)),
            ZIndex(300),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("ZOMVIBE"),
                TextFont {
                    font_size: 80.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.1, 0.1)),
            ));

            parent.spawn((
                Text::new("Select Map"),
                TextFont {
                    font_size: 32.0,
                    ..default()
                },
                TextColor(Color::srgb(0.8, 0.8, 0.8)),
            ));

            for (i, map) in maps.0.iter().enumerate() {
                parent
                    .spawn((
                        MapButton(i),
                        Button,
                        Node {
                            width: Val::Px(250.0),
                            height: Val::Px(55.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.2, 0.6, 0.2)),
                    ))
                    .with_children(|p| {
                        p.spawn((
                            Text::new(map.name.clone()),
                            TextFont {
                                font_size: 28.0,
                                ..default()
                            },
                            TextColor(Color::WHITE),
                        ));
                    });
            }

            // Exit game button
            parent
                .spawn((
                    MapExitButton,
                    Button,
                    Node {
                        width: Val::Px(250.0),
                        height: Val::Px(55.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        margin: UiRect::top(Val::Px(20.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.6, 0.2, 0.2)),
                ))
                .with_children(|p| {
                    p.spawn((
                        Text::new("Exit Game"),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });
        });
}

fn map_select_interaction(
    mut interaction_q: Query<
        (&Interaction, &MapButton, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>, Without<MapExitButton>),
    >,
    maps: Res<AvailableMaps>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (interaction, button, mut bg) in interaction_q.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                let map_option = &maps.0[button.0];
                let config = match &map_option.path {
                    Some(path) => load_map_config(path),
                    None => default_map_config(),
                };
                commands.insert_resource(config);
                next_state.set(AppState::Playing);
            }
            Interaction::Hovered => {
                bg.0 = Color::srgb(0.3, 0.8, 0.3);
            }
            Interaction::None => {
                bg.0 = Color::srgb(0.2, 0.6, 0.2);
            }
        }
    }
}

fn map_exit_interaction(
    mut interaction_q: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, With<MapExitButton>),
    >,
    mut exit: EventWriter<AppExit>,
) {
    for (interaction, mut bg) in interaction_q.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                exit.send(AppExit::Success);
            }
            Interaction::Hovered => {
                bg.0 = Color::srgb(0.8, 0.3, 0.3);
            }
            Interaction::None => {
                bg.0 = Color::srgb(0.6, 0.2, 0.2);
            }
        }
    }
}

fn cleanup_map_select(
    mut commands: Commands,
    query: Query<Entity, With<MapSelectUI>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn_recursive();
    }
}

/// Stores entity IDs that existed before entering Playing state.
#[derive(Resource)]
struct PrePlayingEntities(Vec<Entity>);

fn snapshot_entities(
    entities: Query<Entity>,
    mut commands: Commands,
) {
    let ids: Vec<Entity> = entities.iter().collect();
    commands.insert_resource(PrePlayingEntities(ids));
}

fn cleanup_playing(
    mut commands: Commands,
    entities: Query<Entity>,
    pre: Res<PrePlayingEntities>,
    mut game: ResMut<GameState>,
    mut tree_positions: ResMut<TreePositions>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    for entity in entities.iter() {
        if !pre.0.contains(&entity) {
            commands.entity(entity).try_despawn_recursive();
        }
    }
    *game = GameState::default();
    tree_positions.0.clear();
    // Unlock cursor for menu
    if let Ok(mut window) = windows.get_single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }
    commands.remove_resource::<PrePlayingEntities>();
    commands.remove_resource::<NavGrid>();
    commands.remove_resource::<ZombieSpawnTimer>();
    commands.remove_resource::<FloorSurfaces>();
    commands.remove_resource::<HouseWalls>();
    commands.remove_resource::<MapConfig>();
}

// ── Setup ────────────────────────────────────────────────────────────────────

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut game: ResMut<GameState>,
    mut tree_positions: ResMut<TreePositions>,
    map_config: Res<MapConfig>,
) {
    let map_half = map_config.map_half();
    let grid_size = (map_half * 2.0 / GRID_CELL) as usize;
    let mut nav_grid = NavGrid::new(map_half, grid_size);
    let mut floor_surfaces = FloorSurfaces::default();
    let mut house_walls = HouseWalls::default();
    let tree_radius = map_config.trees.collision_radius;

    game.health = map_config.player.health;
    game.ammo = map_config.player.ammo.saturating_sub(10);
    game.magazine = map_config.player.ammo.min(10);
    game.is_grounded = true;

    // Ground
    let gc = &map_config.ground.color;
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(map_half * 2.0, map_half * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(gc[0], gc[1], gc[2]),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // Walls around the map
    let wc = &map_config.walls.color;
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(wc[0], wc[1], wc[2]),
        ..default()
    });
    let wall_height = map_config.walls.height;
    let wall_thickness = map_config.walls.thickness;
    let walls = [
        (map_half * 2.0 + wall_thickness * 2.0, wall_height, wall_thickness, 0.0, wall_height / 2.0, map_half + wall_thickness / 2.0),
        (map_half * 2.0 + wall_thickness * 2.0, wall_height, wall_thickness, 0.0, wall_height / 2.0, -(map_half + wall_thickness / 2.0)),
        (wall_thickness, wall_height, map_half * 2.0, map_half + wall_thickness / 2.0, wall_height / 2.0, 0.0),
        (wall_thickness, wall_height, map_half * 2.0, -(map_half + wall_thickness / 2.0), wall_height / 2.0, 0.0),
    ];
    for (w, h, d, x, y, z) in walls {
        commands.spawn((
            Wall,
            Mesh3d(meshes.add(Cuboid::new(w, h, d))),
            MeshMaterial3d(wall_material.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }

    // Trees
    let ts = &map_config.trees.trunk.size;
    let tc = &map_config.trees.trunk.color;
    let cs = &map_config.trees.canopy.size;
    let cc = &map_config.trees.canopy.color;
    let trunk_mesh = meshes.add(Cuboid::new(ts[0], ts[1], ts[2]));
    let canopy_mesh = meshes.add(Cuboid::new(cs[0], cs[1], cs[2]));
    let trunk_material = materials.add(StandardMaterial {
        base_color: Color::srgb(tc[0], tc[1], tc[2]),
        ..default()
    });
    let canopy_material = materials.add(StandardMaterial {
        base_color: Color::srgb(cc[0], cc[1], cc[2]),
        ..default()
    });

    let trees: Vec<Vec2> = match &map_config.trees.placement {
        TreePlacement::Random { count, min_spacing, clear_radius } => {
            let mut rng = rand::thread_rng();
            let mut placed = Vec::new();
            let mut attempts = 0;
            while placed.len() < *count && attempts < 500 {
                attempts += 1;
                let x = rng.gen_range(-(map_half - 2.0)..(map_half - 2.0));
                let z = rng.gen_range(-(map_half - 2.0)..(map_half - 2.0));
                if x.abs() < *clear_radius && z.abs() < *clear_radius {
                    continue;
                }
                let too_close = placed.iter().any(|t: &Vec2| t.distance(Vec2::new(x, z)) < *min_spacing);
                if too_close {
                    continue;
                }
                placed.push(Vec2::new(x, z));
            }
            placed
        }
        TreePlacement::Fixed(positions) => {
            positions.iter().map(|p| Vec2::new(p[0], p[1])).collect()
        }
    };

    let trunk_y = ts[1] / 2.0;
    let canopy_y = ts[1] + cs[1] / 2.0;

    for &pos in &trees {
        commands.spawn((
            Mesh3d(trunk_mesh.clone()),
            MeshMaterial3d(trunk_material.clone()),
            Transform::from_xyz(pos.x, trunk_y, pos.y),
        ));
        commands.spawn((
            Mesh3d(canopy_mesh.clone()),
            MeshMaterial3d(canopy_material.clone()),
            Transform::from_xyz(pos.x, canopy_y, pos.y),
        ));

        // Mark nav grid cells as blocked around this tree
        let radius_cells = (tree_radius / GRID_CELL).ceil() as i32 + 1;
        if let Some((cx, cz)) = nav_grid.world_to_grid(pos.x, pos.y) {
            for dx in -radius_cells..=radius_cells {
                for dz in -radius_cells..=radius_cells {
                    let nx = cx as i32 + dx;
                    let nz = cz as i32 + dz;
                    if nx >= 0 && nx < nav_grid.grid_size as i32 && nz >= 0 && nz < nav_grid.grid_size as i32 {
                        let world = nav_grid.grid_to_world(nx as usize, nz as usize);
                        if world.distance(pos) < tree_radius {
                            let idx = nav_grid.idx(nx as usize, nz as usize);
                            nav_grid.blocked[idx] = true;
                        }
                    }
                }
            }
        }
    }

    tree_positions.0 = trees;

    // Spawn structures
    for s in &map_config.structures {
        let ox = s.position[0];
        let oz = s.position[1];
        match s.kind {
            StructureType::House => spawn_house(&mut commands, &mut meshes, &mut materials, &mut nav_grid, &mut floor_surfaces.0, &mut house_walls.0, ox, oz),
            StructureType::Hut => spawn_hut(&mut commands, &mut meshes, &mut materials, &mut nav_grid, &mut floor_surfaces.0, &mut house_walls.0, ox, oz),
            StructureType::Castle => spawn_castle(&mut commands, &mut meshes, &mut materials, &mut nav_grid, &mut floor_surfaces.0, &mut house_walls.0, ox, oz),
        }
    }

    // Directional light (sun)
    let sa = &map_config.lighting.sun_angle;
    commands.spawn((
        DirectionalLight {
            illuminance: map_config.lighting.sun_illuminance,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, sa[0], sa[1], sa[2])),
    ));

    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: map_config.lighting.ambient_brightness,
    });

    // Player body
    let ps = &map_config.player.spawn;
    let player_entity = commands
        .spawn((
            Player,
            Transform::from_xyz(ps[0], ps[1], ps[2]),
            Visibility::default(),
        ))
        .id();

    // Camera anchor (rotates with mouse for pitch)
    let camera_anchor = commands
        .spawn((
            CameraAnchor,
            Transform::from_xyz(0.0, 0.7, 0.0),
            Visibility::default(),
        ))
        .set_parent(player_entity)
        .id();

    // First-person camera
    commands
        .spawn((
            Camera3d::default(),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Projection::Perspective(PerspectiveProjection {
                fov: 90_f32.to_radians(),
                ..default()
            }),
        ))
        .set_parent(camera_anchor);

    // Gun (black square, offset to bottom-right of view)
    commands
        .spawn((
            Gun,
            Mesh3d(meshes.add(Cuboid::new(0.08, 0.08, 0.4))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.05, 0.05, 0.05),
                ..default()
            })),
            Transform::from_xyz(0.25, -0.25, -0.5),
        ))
        .set_parent(camera_anchor);

    // Melee weapon (knife, offset to bottom-left of view, hidden by default)
    commands
        .spawn((
            MeleeWeapon,
            Mesh3d(meshes.add(Cuboid::new(0.04, 0.04, 0.35))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.6, 0.6, 0.65),
                metallic: 0.9,
                perceptual_roughness: 0.3,
                ..default()
            })),
            Transform::from_xyz(-0.3, -0.35, -0.45),
            Visibility::Hidden,
        ))
        .set_parent(camera_anchor);

    // Insert resources created locally
    let spawn_interval = map_config.zombies.spawn_interval;
    commands.insert_resource(ZombieSpawnTimer(Timer::from_seconds(spawn_interval, TimerMode::Repeating)));
    commands.insert_resource(nav_grid);
    commands.insert_resource(floor_surfaces);
    commands.insert_resource(house_walls);
}

fn setup_ui(mut commands: Commands) {
    // HUD root
    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|parent| {
            // Top bar - kills, score
            parent
                .spawn(Node {
                    padding: UiRect::all(Val::Px(12.0)),
                    column_gap: Val::Px(24.0),
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        KillText,
                        Text::new("Kills: 0"),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                    p.spawn((
                        ScoreText,
                        Text::new("Score: 0"),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.85, 0.0)),
                    ));
                });

            // Crosshair
            parent
                .spawn(Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    position_type: PositionType::Absolute,
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        Text::new("+"),
                        TextFont { font_size: 20.0, ..default() },
                        TextColor(Color::WHITE),
                    ));
                });

            // Bottom bar - health and ammo
            parent
                .spawn(Node {
                    padding: UiRect::all(Val::Px(12.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        HealthText,
                        Text::new("HP: 100"),
                        TextFont { font_size: 24.0, ..default() },
                        TextColor(Color::srgb(0.2, 1.0, 0.2)),
                    ));
                    p.spawn((
                        AmmoText,
                        Text::new("Ammo: 10 / 90"),
                        TextFont { font_size: 24.0, ..default() },
                        TextColor(Color::WHITE),
                    ));
                });
        });

    // Damage flash overlay (red, normally invisible)
    commands.spawn((
        DamageOverlay,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 0.0, 0.0, 0.0)),
        ZIndex(100),
    ));

    // Pause menu (hidden by default)
    commands
        .spawn((
            PauseMenu,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(20.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
            ZIndex(200),
            Visibility::Hidden,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("PAUSED"),
                TextFont {
                    font_size: 64.0,
                    ..default()
                },
                TextColor(Color::WHITE),
            ));

            // Resume button
            parent
                .spawn((
                    PauseButton::Resume,
                    Button,
                    Node {
                        width: Val::Px(200.0),
                        height: Val::Px(50.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.2, 0.6, 0.2)),
                ))
                .with_children(|p| {
                    p.spawn((
                        Text::new("Resume"),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });

            // Exit button
            parent
                .spawn((
                    PauseButton::Exit,
                    Button,
                    Node {
                        width: Val::Px(200.0),
                        height: Val::Px(50.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.6, 0.2, 0.2)),
                ))
                .with_children(|p| {
                    p.spawn((
                        Text::new("Exit"),
                        TextFont {
                            font_size: 28.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));
                });
        });
}

fn grab_cursor(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = windows.get_single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    }
}

// ── Collision helpers ────────────────────────────────────────────────────────

fn collides_with_tree(pos: Vec2, radius: f32, trees: &[Vec2], tree_radius: f32) -> Option<Vec2> {
    for &tree in trees {
        let diff = pos - tree;
        let dist = diff.length();
        let min_dist = radius + tree_radius;
        if dist < min_dist && dist > 0.001 {
            return Some(diff.normalize() * (min_dist - dist));
        }
    }
    None
}

fn collide_with_walls(pos: &mut Vec3, radius: f32, walls: &HouseWalls) {
    for wall in &walls.0 {
        if pos.y < wall.min_y || pos.y > wall.max_y {
            continue;
        }
        let exp_min_x = wall.min_x - radius;
        let exp_max_x = wall.max_x + radius;
        let exp_min_z = wall.min_z - radius;
        let exp_max_z = wall.max_z + radius;
        if pos.x > exp_min_x && pos.x < exp_max_x && pos.z > exp_min_z && pos.z < exp_max_z {
            let push_left = pos.x - exp_min_x;
            let push_right = exp_max_x - pos.x;
            let push_back = pos.z - exp_min_z;
            let push_front = exp_max_z - pos.z;
            let min_push = push_left.min(push_right).min(push_back).min(push_front);
            if min_push == push_left {
                pos.x = exp_min_x;
            } else if min_push == push_right {
                pos.x = exp_max_x;
            } else if min_push == push_back {
                pos.z = exp_min_z;
            } else {
                pos.z = exp_max_z;
            }
        }
    }
}

fn spawn_house(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    nav_grid: &mut NavGrid,
    floor_surfaces: &mut Vec<FloorRect>,
    house_walls: &mut Vec<WallRect>,
    ox: f32,
    oz: f32,
) {
    let wall_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.55, 0.45),
        ..default()
    });
    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.35, 0.2),
        ..default()
    });
    let stair_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.3, 0.15),
        ..default()
    });
    let railing_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.4, 0.4),
        ..default()
    });

    // ── Ground floor walls ──
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 3.0, 0.3))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox, 1.5, oz - 5.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.5, 3.0, 0.3))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox - 3.25, 1.5, oz + 5.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.5, 3.0, 0.3))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + 3.25, 1.5, oz + 5.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 3.0, 10.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + 5.0, 1.5, oz),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 3.0, 10.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox - 5.0, 1.5, oz),
    ));

    // ── Second floor slab ──
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 0.2, 7.5))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox, 3.0, oz - 1.25),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(7.5, 0.2, 2.5))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox + 1.25, 3.0, oz + 3.75),
    ));

    // ── Second floor walls ──
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 3.0, 0.3))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox, 4.5, oz - 5.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 3.0, 0.3))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox, 4.5, oz + 5.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 3.0, 10.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox - 5.0, 4.5, oz),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 3.0, 3.5))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + 5.0, 4.5, oz - 3.25),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.3, 3.0, 3.5))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + 5.0, 4.5, oz + 3.25),
    ));

    // ── Stairs ──
    for i in 0..8u32 {
        let step_height = (i + 1) as f32 * 0.375;
        let step_z = 4.55 - i as f32 * 0.3;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.5, step_height, 0.3))),
            MeshMaterial3d(stair_mat.clone()),
            Transform::from_xyz(ox - 3.45, step_height / 2.0, oz + step_z),
        ));
    }

    // ── Balcony ──
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 0.2, 6.0))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox + 6.5, 3.0, oz),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 1.0, 0.1))),
        MeshMaterial3d(railing_mat.clone()),
        Transform::from_xyz(ox + 6.5, 3.6, oz - 3.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(3.0, 1.0, 0.1))),
        MeshMaterial3d(railing_mat.clone()),
        Transform::from_xyz(ox + 6.5, 3.6, oz + 3.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.1, 1.0, 6.0))),
        MeshMaterial3d(railing_mat.clone()),
        Transform::from_xyz(ox + 8.0, 3.6, oz),
    ));

    // ── Roof slab ──
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(10.0, 0.2, 10.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox, 6.0, oz),
    ));

    // ── Floor surfaces for collision ──
    floor_surfaces.push(FloorRect { min_x: ox - 5.0, max_x: ox + 5.0, min_z: oz - 5.0, max_z: oz + 2.5, y: 3.0 });
    floor_surfaces.push(FloorRect { min_x: ox - 2.5, max_x: ox + 5.0, min_z: oz + 2.5, max_z: oz + 5.0, y: 3.0 });
    floor_surfaces.push(FloorRect { min_x: ox + 5.0, max_x: ox + 8.0, min_z: oz - 3.0, max_z: oz + 3.0, y: 3.0 });
    for i in 0..8u32 {
        let step_y = (i + 1) as f32 * 0.375;
        let step_z_center = 4.55 - i as f32 * 0.3;
        floor_surfaces.push(FloorRect {
            min_x: ox - 4.7,
            max_x: ox - 2.2,
            min_z: oz + step_z_center - 0.15,
            max_z: oz + step_z_center + 0.15,
            y: step_y,
        });
    }

    // ── Wall collision rects ──
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox + 5.15, min_z: oz - 5.15, max_z: oz - 4.85, min_y: 0.0, max_y: 3.0 });
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox - 1.5, min_z: oz + 4.85, max_z: oz + 5.15, min_y: 0.0, max_y: 3.0 });
    house_walls.push(WallRect { min_x: ox + 1.5, max_x: ox + 5.15, min_z: oz + 4.85, max_z: oz + 5.15, min_y: 0.0, max_y: 3.0 });
    house_walls.push(WallRect { min_x: ox + 4.85, max_x: ox + 5.15, min_z: oz - 5.15, max_z: oz + 5.15, min_y: 0.0, max_y: 3.0 });
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox - 4.85, min_z: oz - 5.15, max_z: oz + 5.15, min_y: 0.0, max_y: 3.0 });
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox + 5.15, min_z: oz - 5.15, max_z: oz - 4.85, min_y: 3.0, max_y: 6.0 });
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox + 5.15, min_z: oz + 4.85, max_z: oz + 5.15, min_y: 3.0, max_y: 6.0 });
    house_walls.push(WallRect { min_x: ox - 5.15, max_x: ox - 4.85, min_z: oz - 5.15, max_z: oz + 5.15, min_y: 3.0, max_y: 6.0 });
    house_walls.push(WallRect { min_x: ox + 4.85, max_x: ox + 5.15, min_z: oz - 5.15, max_z: oz - 1.5, min_y: 3.0, max_y: 6.0 });
    house_walls.push(WallRect { min_x: ox + 4.85, max_x: ox + 5.15, min_z: oz + 1.5, max_z: oz + 5.15, min_y: 3.0, max_y: 6.0 });
    house_walls.push(WallRect { min_x: ox + 5.0, max_x: ox + 8.05, min_z: oz - 3.05, max_z: oz - 2.95, min_y: 3.1, max_y: 4.1 });
    house_walls.push(WallRect { min_x: ox + 5.0, max_x: ox + 8.05, min_z: oz + 2.95, max_z: oz + 3.05, min_y: 3.1, max_y: 4.1 });
    house_walls.push(WallRect { min_x: ox + 7.95, max_x: ox + 8.05, min_z: oz - 3.05, max_z: oz + 3.05, min_y: 3.1, max_y: 4.1 });

    // ── Block nav grid ──
    for gx in 0..nav_grid.grid_size {
        for gz in 0..nav_grid.grid_size {
            let world = nav_grid.grid_to_world(gx, gz);
            let in_house = world.x >= ox - 5.5 && world.x <= ox + 5.5 && world.y >= oz - 5.5 && world.y <= oz + 5.5;
            let in_balcony = world.x >= ox + 5.0 && world.x <= ox + 8.5 && world.y >= oz - 3.5 && world.y <= oz + 3.5;
            if in_house || in_balcony {
                let idx = nav_grid.idx(gx, gz);
                nav_grid.blocked[idx] = true;
            }
        }
    }
}

fn spawn_hut(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    nav_grid: &mut NavGrid,
    _floor_surfaces: &mut Vec<FloorRect>,
    house_walls: &mut Vec<WallRect>,
    ox: f32,
    oz: f32,
) {
    let wall_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.4, 0.3),
        ..default()
    });
    let roof_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.25, 0.1),
        ..default()
    });

    let hw = 2.5; // half-width
    let h = 2.5;  // wall height
    let t = 0.2;  // wall thickness

    // North wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(hw * 2.0, h, t))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox, h / 2.0, oz - hw),
    ));
    // South wall - left of door
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, h, t))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox - 1.75, h / 2.0, oz + hw),
    ));
    // South wall - right of door
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, h, t))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + 1.75, h / 2.0, oz + hw),
    ));
    // East wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(t, h, hw * 2.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox + hw, h / 2.0, oz),
    ));
    // West wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(t, h, hw * 2.0))),
        MeshMaterial3d(wall_mat.clone()),
        Transform::from_xyz(ox - hw, h / 2.0, oz),
    ));
    // Roof
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(hw * 2.0 + 0.6, 0.15, hw * 2.0 + 0.6))),
        MeshMaterial3d(roof_mat),
        Transform::from_xyz(ox, h + 0.075, oz),
    ));

    // Wall collision rects
    house_walls.push(WallRect { min_x: ox - hw - 0.1, max_x: ox + hw + 0.1, min_z: oz - hw - 0.1, max_z: oz - hw + 0.1, min_y: 0.0, max_y: h });
    house_walls.push(WallRect { min_x: ox - hw - 0.1, max_x: ox - 1.0, min_z: oz + hw - 0.1, max_z: oz + hw + 0.1, min_y: 0.0, max_y: h });
    house_walls.push(WallRect { min_x: ox + 1.0, max_x: ox + hw + 0.1, min_z: oz + hw - 0.1, max_z: oz + hw + 0.1, min_y: 0.0, max_y: h });
    house_walls.push(WallRect { min_x: ox + hw - 0.1, max_x: ox + hw + 0.1, min_z: oz - hw - 0.1, max_z: oz + hw + 0.1, min_y: 0.0, max_y: h });
    house_walls.push(WallRect { min_x: ox - hw - 0.1, max_x: ox - hw + 0.1, min_z: oz - hw - 0.1, max_z: oz + hw + 0.1, min_y: 0.0, max_y: h });

    // Block nav grid
    for gx in 0..nav_grid.grid_size {
        for gz in 0..nav_grid.grid_size {
            let world = nav_grid.grid_to_world(gx, gz);
            if world.x >= ox - hw - 0.5 && world.x <= ox + hw + 0.5 && world.y >= oz - hw - 0.5 && world.y <= oz + hw + 0.5 {
                let idx = nav_grid.idx(gx, gz);
                nav_grid.blocked[idx] = true;
            }
        }
    }
}

fn spawn_castle(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    nav_grid: &mut NavGrid,
    floor_surfaces: &mut Vec<FloorRect>,
    house_walls: &mut Vec<WallRect>,
    ox: f32,
    oz: f32,
) {
    let stone_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.42, 0.38),
        ..default()
    });
    let dark_stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.32, 0.28),
        ..default()
    });
    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.38, 0.35),
        ..default()
    });
    let stair_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.38, 0.35, 0.3),
        ..default()
    });

    let half = 15.0; // 30x30 footprint
    let wall_h = 8.0;
    let wall_t = 1.0;
    let gate_w = 4.0; // gate opening width

    // ── Curtain walls ──
    // North wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, wall_h, wall_t))),
        MeshMaterial3d(stone_mat.clone()),
        Transform::from_xyz(ox, wall_h / 2.0, oz - half),
    ));
    // South wall - left of gate
    let south_side = (half * 2.0 - gate_w) / 2.0;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(south_side, wall_h, wall_t))),
        MeshMaterial3d(stone_mat.clone()),
        Transform::from_xyz(ox - half + south_side / 2.0, wall_h / 2.0, oz + half),
    ));
    // South wall - right of gate
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(south_side, wall_h, wall_t))),
        MeshMaterial3d(stone_mat.clone()),
        Transform::from_xyz(ox + half - south_side / 2.0, wall_h / 2.0, oz + half),
    ));
    // East wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_t, wall_h, half * 2.0))),
        MeshMaterial3d(stone_mat.clone()),
        Transform::from_xyz(ox + half, wall_h / 2.0, oz),
    ));
    // West wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(wall_t, wall_h, half * 2.0))),
        MeshMaterial3d(stone_mat.clone()),
        Transform::from_xyz(ox - half, wall_h / 2.0, oz),
    ));

    // ── Battlements (crenellations on top of curtain walls) ──
    let merlon_w = 1.0;
    let merlon_h = 1.5;
    let merlon_spacing = 2.5;
    // North and south battlements
    let mut bx = -half + 1.25;
    while bx <= half - 1.25 {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(merlon_w, merlon_h, wall_t))),
            MeshMaterial3d(stone_mat.clone()),
            Transform::from_xyz(ox + bx, wall_h + merlon_h / 2.0, oz - half),
        ));
        // Skip south merlons over the gate
        if (bx - 0.0).abs() > gate_w / 2.0 + 0.5 {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(merlon_w, merlon_h, wall_t))),
                MeshMaterial3d(stone_mat.clone()),
                Transform::from_xyz(ox + bx, wall_h + merlon_h / 2.0, oz + half),
            ));
        }
        bx += merlon_spacing;
    }
    // East and west battlements
    let mut bz = -half + 1.25;
    while bz <= half - 1.25 {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(wall_t, merlon_h, merlon_w))),
            MeshMaterial3d(stone_mat.clone()),
            Transform::from_xyz(ox + half, wall_h + merlon_h / 2.0, oz + bz),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(wall_t, merlon_h, merlon_w))),
            MeshMaterial3d(stone_mat.clone()),
            Transform::from_xyz(ox - half, wall_h + merlon_h / 2.0, oz + bz),
        ));
        bz += merlon_spacing;
    }

    // ── Walkway on top of curtain walls ──
    let walkway_w = 2.0;
    // North walkway
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(half * 2.0, 0.2, walkway_w))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox, wall_h, oz - half + walkway_w / 2.0 - 0.5),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - half, max_x: ox + half, min_z: oz - half - 0.5, max_z: oz - half + walkway_w - 0.5, y: wall_h });
    // South walkway (two pieces, gap for gate)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(south_side, 0.2, walkway_w))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox - half + south_side / 2.0, wall_h, oz + half - walkway_w / 2.0 + 0.5),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(south_side, 0.2, walkway_w))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox + half - south_side / 2.0, wall_h, oz + half - walkway_w / 2.0 + 0.5),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - half, max_x: ox - gate_w / 2.0, min_z: oz + half - walkway_w + 0.5, max_z: oz + half + 0.5, y: wall_h });
    floor_surfaces.push(FloorRect { min_x: ox + gate_w / 2.0, max_x: ox + half, min_z: oz + half - walkway_w + 0.5, max_z: oz + half + 0.5, y: wall_h });
    // East walkway
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(walkway_w, 0.2, half * 2.0))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox + half - walkway_w / 2.0 + 0.5, wall_h, oz),
    ));
    floor_surfaces.push(FloorRect { min_x: ox + half - walkway_w + 0.5, max_x: ox + half + 0.5, min_z: oz - half, max_z: oz + half, y: wall_h });
    // West walkway
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(walkway_w, 0.2, half * 2.0))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox - half + walkway_w / 2.0 - 0.5, wall_h, oz),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - half - 0.5, max_x: ox - half + walkway_w - 0.5, min_z: oz - half, max_z: oz + half, y: wall_h });

    // ── 4 Corner towers (4x4, 12 tall) ──
    let tw = 4.0; // tower width
    let th = 12.0; // tower height
    let tower_positions = [
        (ox - half + tw / 2.0, oz - half + tw / 2.0),
        (ox + half - tw / 2.0, oz - half + tw / 2.0),
        (ox - half + tw / 2.0, oz + half - tw / 2.0),
        (ox + half - tw / 2.0, oz + half - tw / 2.0),
    ];
    for &(tx, tz) in &tower_positions {
        // Tower walls (4 sides)
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(tw, th, 0.3))),
            MeshMaterial3d(dark_stone.clone()),
            Transform::from_xyz(tx, th / 2.0, tz - tw / 2.0),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(tw, th, 0.3))),
            MeshMaterial3d(dark_stone.clone()),
            Transform::from_xyz(tx, th / 2.0, tz + tw / 2.0),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.3, th, tw))),
            MeshMaterial3d(dark_stone.clone()),
            Transform::from_xyz(tx - tw / 2.0, th / 2.0, tz),
        ));
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.3, th, tw))),
            MeshMaterial3d(dark_stone.clone()),
            Transform::from_xyz(tx + tw / 2.0, th / 2.0, tz),
        ));
        // Tower top platform
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(tw + 0.6, 0.2, tw + 0.6))),
            MeshMaterial3d(floor_mat.clone()),
            Transform::from_xyz(tx, th, tz),
        ));
        floor_surfaces.push(FloorRect { min_x: tx - tw / 2.0 - 0.3, max_x: tx + tw / 2.0 + 0.3, min_z: tz - tw / 2.0 - 0.3, max_z: tz + tw / 2.0 + 0.3, y: th });

        // Stairs inside tower (spiral-ish, 16 steps from wall walkway to top)
        let steps = 16;
        for i in 0..steps {
            let step_y = wall_h + (i as f32 + 1.0) * (th - wall_h) / steps as f32;
            let step_z = tz - tw / 2.0 + 0.4 + (i as f32 / steps as f32) * (tw - 0.8);
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(tw - 0.4, 0.15, 0.25))),
                MeshMaterial3d(stair_mat.clone()),
                Transform::from_xyz(tx, step_y, step_z),
            ));
            floor_surfaces.push(FloorRect {
                min_x: tx - tw / 2.0 + 0.2,
                max_x: tx + tw / 2.0 - 0.2,
                min_z: step_z - 0.125,
                max_z: step_z + 0.125,
                y: step_y,
            });
        }

        // Tower wall collisions
        let thw = tw / 2.0;
        house_walls.push(WallRect { min_x: tx - thw - 0.15, max_x: tx + thw + 0.15, min_z: tz - thw - 0.15, max_z: tz - thw + 0.15, min_y: 0.0, max_y: th });
        house_walls.push(WallRect { min_x: tx - thw - 0.15, max_x: tx + thw + 0.15, min_z: tz + thw - 0.15, max_z: tz + thw + 0.15, min_y: 0.0, max_y: th });
        house_walls.push(WallRect { min_x: tx - thw - 0.15, max_x: tx - thw + 0.15, min_z: tz - thw - 0.15, max_z: tz + thw + 0.15, min_y: 0.0, max_y: th });
        house_walls.push(WallRect { min_x: tx + thw - 0.15, max_x: tx + thw + 0.15, min_z: tz - thw - 0.15, max_z: tz + thw + 0.15, min_y: 0.0, max_y: th });
    }

    // ── Central keep (12x12, 10 tall, two stories) ──
    let kh = 6.0; // keep half-width
    let keep_h = 10.0;
    let keep_t = 0.5;

    // Keep walls
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(kh * 2.0, keep_h, keep_t))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox, keep_h / 2.0, oz - kh),
    ));
    // South keep wall - with door
    let keep_door_w = 3.0;
    let keep_side = (kh * 2.0 - keep_door_w) / 2.0;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(keep_side, keep_h, keep_t))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox - kh + keep_side / 2.0, keep_h / 2.0, oz + kh),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(keep_side, keep_h, keep_t))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox + kh - keep_side / 2.0, keep_h / 2.0, oz + kh),
    ));
    // Door lintel above keep entrance
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(keep_door_w, keep_h - 3.0, keep_t))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox, 3.0 + (keep_h - 3.0) / 2.0, oz + kh),
    ));
    // East wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(keep_t, keep_h, kh * 2.0))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox + kh, keep_h / 2.0, oz),
    ));
    // West wall
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(keep_t, keep_h, kh * 2.0))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox - kh, keep_h / 2.0, oz),
    ));

    // Keep second floor (at y=5, with stairwell hole in NE corner)
    let second_y = 5.0;
    // Main piece
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(kh * 2.0, 0.2, kh * 2.0 - 3.0))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox, second_y, oz - 1.5),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - kh, max_x: ox + kh, min_z: oz - kh, max_z: oz + kh - 3.0, y: second_y });
    // Side piece (fill except stairwell)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(kh * 2.0 - 3.0, 0.2, 3.0))),
        MeshMaterial3d(floor_mat.clone()),
        Transform::from_xyz(ox - 1.5, second_y, oz + kh - 1.5),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - kh, max_x: ox + kh - 3.0, min_z: oz + kh - 3.0, max_z: oz + kh, y: second_y });

    // Keep roof
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(kh * 2.0 + 0.6, 0.2, kh * 2.0 + 0.6))),
        MeshMaterial3d(dark_stone.clone()),
        Transform::from_xyz(ox, keep_h, oz),
    ));
    floor_surfaces.push(FloorRect { min_x: ox - kh - 0.3, max_x: ox + kh + 0.3, min_z: oz - kh - 0.3, max_z: oz + kh + 0.3, y: keep_h });

    // Keep stairs (in SE corner, going up from ground to second floor)
    let keep_steps = 12;
    for i in 0..keep_steps {
        let step_y = (i as f32 + 1.0) * second_y / keep_steps as f32;
        let step_z = oz + kh - 0.4 - (i as f32 / keep_steps as f32) * 2.6;
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(2.5, step_y, 0.22))),
            MeshMaterial3d(stair_mat.clone()),
            Transform::from_xyz(ox + kh - 1.5, step_y / 2.0, step_z),
        ));
        floor_surfaces.push(FloorRect {
            min_x: ox + kh - 2.75,
            max_x: ox + kh - 0.25,
            min_z: step_z - 0.11,
            max_z: step_z + 0.11,
            y: step_y,
        });
    }

    // Keep wall collisions
    house_walls.push(WallRect { min_x: ox - kh - 0.25, max_x: ox + kh + 0.25, min_z: oz - kh - 0.25, max_z: oz - kh + 0.25, min_y: 0.0, max_y: keep_h });
    house_walls.push(WallRect { min_x: ox - kh - 0.25, max_x: ox - keep_door_w / 2.0, min_z: oz + kh - 0.25, max_z: oz + kh + 0.25, min_y: 0.0, max_y: keep_h });
    house_walls.push(WallRect { min_x: ox + keep_door_w / 2.0, max_x: ox + kh + 0.25, min_z: oz + kh - 0.25, max_z: oz + kh + 0.25, min_y: 0.0, max_y: keep_h });
    house_walls.push(WallRect { min_x: ox - keep_door_w / 2.0, max_x: ox + keep_door_w / 2.0, min_z: oz + kh - 0.25, max_z: oz + kh + 0.25, min_y: 3.0, max_y: keep_h });
    house_walls.push(WallRect { min_x: ox + kh - 0.25, max_x: ox + kh + 0.25, min_z: oz - kh - 0.25, max_z: oz + kh + 0.25, min_y: 0.0, max_y: keep_h });
    house_walls.push(WallRect { min_x: ox - kh - 0.25, max_x: ox - kh + 0.25, min_z: oz - kh - 0.25, max_z: oz + kh + 0.25, min_y: 0.0, max_y: keep_h });

    // Curtain wall collisions
    house_walls.push(WallRect { min_x: ox - half - 0.5, max_x: ox + half + 0.5, min_z: oz - half - 0.5, max_z: oz - half + 0.5, min_y: 0.0, max_y: wall_h });
    house_walls.push(WallRect { min_x: ox - half - 0.5, max_x: ox - gate_w / 2.0, min_z: oz + half - 0.5, max_z: oz + half + 0.5, min_y: 0.0, max_y: wall_h });
    house_walls.push(WallRect { min_x: ox + gate_w / 2.0, max_x: ox + half + 0.5, min_z: oz + half - 0.5, max_z: oz + half + 0.5, min_y: 0.0, max_y: wall_h });
    house_walls.push(WallRect { min_x: ox + half - 0.5, max_x: ox + half + 0.5, min_z: oz - half - 0.5, max_z: oz + half + 0.5, min_y: 0.0, max_y: wall_h });
    house_walls.push(WallRect { min_x: ox - half - 0.5, max_x: ox - half + 0.5, min_z: oz - half - 0.5, max_z: oz + half + 0.5, min_y: 0.0, max_y: wall_h });

    // ── Block nav grid for castle footprint ──
    for gx in 0..nav_grid.grid_size {
        for gz in 0..nav_grid.grid_size {
            let world = nav_grid.grid_to_world(gx, gz);
            let wx = world.x;
            let wz = world.y;

            // Block curtain walls
            let on_north = wz >= oz - half - 1.0 && wz <= oz - half + 1.0 && wx >= ox - half - 1.0 && wx <= ox + half + 1.0;
            let on_south = wz >= oz + half - 1.0 && wz <= oz + half + 1.0 && wx >= ox - half - 1.0 && wx <= ox + half + 1.0
                && !((wx - ox).abs() < gate_w / 2.0); // leave gate open
            let on_east = wx >= ox + half - 1.0 && wx <= ox + half + 1.0 && wz >= oz - half - 1.0 && wz <= oz + half + 1.0;
            let on_west = wx >= ox - half - 1.0 && wx <= ox - half + 1.0 && wz >= oz - half - 1.0 && wz <= oz + half + 1.0;

            // Block keep
            let in_keep = wx >= ox - kh - 0.5 && wx <= ox + kh + 0.5 && wz >= oz - kh - 0.5 && wz <= oz + kh + 0.5
                && !((wx - ox).abs() < keep_door_w / 2.0 && wz > oz + kh - 0.5); // leave keep door open

            // Block towers
            let in_tower = tower_positions.iter().any(|&(tx, tz)| {
                wx >= tx - tw / 2.0 - 0.5 && wx <= tx + tw / 2.0 + 0.5 && wz >= tz - tw / 2.0 - 0.5 && wz <= tz + tw / 2.0 + 0.5
            });

            if on_north || on_south || on_east || on_west || in_keep || in_tower {
                let idx = nav_grid.idx(gx, gz);
                nav_grid.blocked[idx] = true;
            }
        }
    }
}

// ── Player Systems ────────────────────────────────────────────────────────────

fn player_look(
    mut mouse_motion: EventReader<MouseMotion>,
    mut player_q: Query<&mut Transform, (With<Player>, Without<CameraAnchor>)>,
    mut anchor_q: Query<&mut Transform, (With<CameraAnchor>, Without<Player>)>,
) {
    let sensitivity = 0.002;
    let mut delta = Vec2::ZERO;
    for ev in mouse_motion.read() {
        delta += ev.delta;
    }
    if delta == Vec2::ZERO {
        return;
    }

    if let Ok(mut pt) = player_q.get_single_mut() {
        pt.rotate_y(-delta.x * sensitivity);
    }

    if let Ok(mut at) = anchor_q.get_single_mut() {
        let current_pitch = at.rotation.to_euler(EulerRot::XYZ).0;
        let new_pitch = (current_pitch - delta.y * sensitivity).clamp(-1.4, 1.4);
        at.rotation = Quat::from_rotation_x(new_pitch);
    }
}

fn player_move(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut game: ResMut<GameState>,
    mut player_q: Query<&mut Transform, With<Player>>,
    tree_positions: Res<TreePositions>,
    map_config: Res<MapConfig>,
    floor_surfaces: Res<FloorSurfaces>,
    house_walls: Res<HouseWalls>,
) {
    if game.is_dead {
        return;
    }
    let Ok(mut transform) = player_q.get_single_mut() else {
        return;
    };

    let sprinting = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let speed = if sprinting { map_config.player.sprint_speed } else { map_config.player.speed };
    let forward = transform.forward();
    let right = transform.right();
    let mut velocity = Vec3::ZERO;

    if keys.pressed(KeyCode::KeyW) {
        velocity += *forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        velocity -= *forward;
    }
    if keys.pressed(KeyCode::KeyA) {
        velocity -= *right;
    }
    if keys.pressed(KeyCode::KeyD) {
        velocity += *right;
    }

    velocity.y = 0.0;
    if velocity.length_squared() > 0.0 {
        let movement = velocity.normalize() * speed * time.delta_secs();
        transform.translation += movement;

        // Tree collision - push player out
        let player_pos = Vec2::new(transform.translation.x, transform.translation.z);
        if let Some(push) = collides_with_tree(player_pos, PLAYER_RADIUS, &tree_positions.0, map_config.trees.collision_radius) {
            transform.translation.x += push.x;
            transform.translation.z += push.y;
        }

        // Wall collision
        collide_with_walls(&mut transform.translation, PLAYER_RADIUS, &house_walls);
    }

    // Clamp to map bounds
    let map_bound = map_config.map_half() - 0.5;
    transform.translation.x = transform.translation.x.clamp(-map_bound, map_bound);
    transform.translation.z = transform.translation.z.clamp(-map_bound, map_bound);

    // Jump
    if keys.just_pressed(KeyCode::Space) && game.is_grounded {
        game.vertical_velocity = 7.0;
        game.is_grounded = false;
    }

    // Gravity
    let gravity = 18.0;
    game.vertical_velocity -= gravity * time.delta_secs();
    transform.translation.y += game.vertical_velocity * time.delta_secs();

    // Dynamic ground detection
    let px = transform.translation.x;
    let pz = transform.translation.z;
    let mut ground_y = 0.9_f32;
    for floor in &floor_surfaces.0 {
        if px >= floor.min_x && px <= floor.max_x && pz >= floor.min_z && pz <= floor.max_z {
            let standing_y = floor.y + 0.9;
            if standing_y <= transform.translation.y + 0.5 {
                ground_y = ground_y.max(standing_y);
            }
        }
    }

    if transform.translation.y <= ground_y {
        transform.translation.y = ground_y;
        game.vertical_velocity = 0.0;
        game.is_grounded = true;
    }
}

// ── Shooting ──────────────────────────────────────────────────────────────────

fn reload(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<GameState>,
    time: Res<Time>,
    mut ammo_text: Query<&mut Text, With<AmmoText>>,
) {
    if game.is_dead { return; }

    // Tick reload timer if active
    if let Some(ref mut timer) = game.reload_timer {
        timer.tick(time.delta());
        if timer.finished() {
            let refill = (10 - game.magazine).min(game.ammo);
            game.magazine += refill;
            game.ammo -= refill;
            game.reload_timer = None;
            if let Ok(mut text) = ammo_text.get_single_mut() {
                **text = format!("Ammo: {} / {}", game.magazine, game.ammo);
            }
        }
        return;
    }

    // Manual reload with R (only if magazine isn't full and we have reserve ammo)
    if keys.just_pressed(KeyCode::KeyR) && game.magazine < 10 && game.ammo > 0 {
        game.reload_timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
        if let Ok(mut text) = ammo_text.get_single_mut() {
            **text = "Reloading...".to_string();
        }
    }
}

fn shoot(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut game: ResMut<GameState>,
    player_q: Query<&Transform, With<Player>>,
    anchor_q: Query<&Transform, (With<CameraAnchor>, Without<Player>)>,
    mut ammo_text: Query<&mut Text, With<AmmoText>>,
) {
    if game.is_dead || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // Can't fire while reloading
    if game.reload_timer.is_some() {
        return;
    }
    if game.magazine == 0 {
        // Auto-reload if we have reserve ammo
        if game.ammo > 0 {
            game.reload_timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
            if let Ok(mut text) = ammo_text.get_single_mut() {
                **text = "Reloading...".to_string();
            }
        }
        return;
    }
    game.magazine -= 1;
    if let Ok(mut text) = ammo_text.get_single_mut() {
        **text = format!("Ammo: {} / {}", game.magazine, game.ammo);
    }

    // Auto-reload when magazine hits 0
    if game.magazine == 0 && game.ammo > 0 {
        game.reload_timer = Some(Timer::from_seconds(2.0, TimerMode::Once));
        if let Ok(mut text) = ammo_text.get_single_mut() {
            **text = "Reloading...".to_string();
        }
    }

    let Ok(pt) = player_q.get_single() else { return };
    let Ok(at) = anchor_q.get_single() else { return };

    let combined = pt.rotation * at.rotation;
    let direction = combined * Vec3::NEG_Z;
    let spawn_pos = pt.translation + Vec3::new(0.0, 0.7, 0.0) + direction * 0.5;

    commands.spawn((
        Bullet {
            velocity: direction.normalize() * 30.0,
            lifetime: 2.0,
        },
        Mesh3d(meshes.add(Sphere::new(0.05))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.9, 0.0),
            emissive: LinearRgba::new(5.0, 4.0, 0.0, 1.0),
            ..default()
        })),
        Transform::from_translation(spawn_pos),
    ));
}

fn bullet_movement(
    mut commands: Commands,
    mut bullets: Query<(Entity, &mut Transform, &mut Bullet)>,
    time: Res<Time>,
) {
    for (entity, mut transform, mut bullet) in bullets.iter_mut() {
        transform.translation += bullet.velocity * time.delta_secs();
        bullet.lifetime -= time.delta_secs();
        if bullet.lifetime <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

fn check_bullet_zombie_collision(
    mut commands: Commands,
    bullets: Query<(Entity, &Transform), With<Bullet>>,
    zombies: Query<(Entity, &Transform), With<Zombie>>,
    mut game: ResMut<GameState>,
    mut kill_text: Query<&mut Text, (With<KillText>, Without<ScoreText>)>,
    mut score_text: Query<&mut Text, (With<ScoreText>, Without<KillText>)>,
) {
    for (bullet_entity, bullet_transform) in bullets.iter() {
        for (zombie_entity, zombie_transform) in zombies.iter() {
            let dist = bullet_transform
                .translation
                .distance(zombie_transform.translation);
            if dist < 1.0 {
                commands.entity(bullet_entity).despawn();
                commands.entity(zombie_entity).despawn();
                game.kills += 1;
                game.score += 100;
                if let Ok(mut text) = kill_text.get_single_mut() {
                    **text = format!("Kills: {}", game.kills);
                }
                if let Ok(mut text) = score_text.get_single_mut() {
                    **text = format!("Score: {}", game.score);
                }
                break;
            }
        }
    }
}

// ── Melee ─────────────────────────────────────────────────────────────────────

fn melee_attack(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<GameState>,
    time: Res<Time>,
    player_q: Query<&Transform, With<Player>>,
    anchor_q: Query<&Transform, (With<CameraAnchor>, Without<Player>)>,
    zombies: Query<(Entity, &Transform), With<Zombie>>,
    mut kill_text: Query<&mut Text, (With<KillText>, Without<ScoreText>, Without<AmmoText>)>,
    mut score_text: Query<&mut Text, (With<ScoreText>, Without<KillText>, Without<AmmoText>)>,
    mut melee_q: Query<(Entity, &mut Visibility), With<MeleeWeapon>>,
) {
    // Tick cooldown
    if game.melee_cooldown > 0.0 {
        game.melee_cooldown -= time.delta_secs();
    }

    if game.is_dead || !keyboard.just_pressed(KeyCode::KeyV) {
        return;
    }
    if game.melee_cooldown > 0.0 {
        return;
    }

    game.melee_cooldown = 0.5;

    // Show knife and start swing animation
    if let Ok((entity, mut vis)) = melee_q.get_single_mut() {
        *vis = Visibility::Visible;
        commands.entity(entity).insert(MeleeSwing {
            timer: Timer::from_seconds(0.3, TimerMode::Once),
        });
    }

    let Ok(pt) = player_q.get_single() else { return };
    let Ok(at) = anchor_q.get_single() else { return };

    let combined = pt.rotation * at.rotation;
    let direction = combined * Vec3::NEG_Z;
    let player_pos = pt.translation + Vec3::new(0.0, 0.7, 0.0);

    // Hit all zombies within melee range (2.5 units) and in front of the player
    for (zombie_entity, zombie_transform) in zombies.iter() {
        let to_zombie = zombie_transform.translation - player_pos;
        let dist = to_zombie.length();
        if dist > 2.5 {
            continue;
        }
        // Check zombie is roughly in front of the player
        let dot = direction.normalize().dot(to_zombie.normalize());
        if dot < 0.3 {
            continue;
        }
        commands.entity(zombie_entity).despawn();
        game.kills += 1;
        game.score += 150; // melee kills worth more
        if let Ok(mut text) = kill_text.get_single_mut() {
            **text = format!("Kills: {}", game.kills);
        }
        if let Ok(mut text) = score_text.get_single_mut() {
            **text = format!("Score: {}", game.score);
        }
    }
}

fn melee_swing_animation(
    mut commands: Commands,
    time: Res<Time>,
    mut melee_q: Query<(Entity, &mut Transform, &mut MeleeSwing, &mut Visibility), With<MeleeWeapon>>,
) {
    for (entity, mut transform, mut swing, mut vis) in melee_q.iter_mut() {
        swing.timer.tick(time.delta());
        let progress = swing.timer.fraction();

        // Swing arc: move knife from left to center-right
        let swing_angle = std::f32::consts::FRAC_PI_4 * (1.0 - (progress * std::f32::consts::PI).sin());
        transform.translation = Vec3::new(
            -0.3 + progress * 0.4,
            -0.35 + (progress * std::f32::consts::PI).sin() * 0.1,
            -0.45,
        );
        transform.rotation = Quat::from_rotation_z(swing_angle);

        if swing.timer.finished() {
            // Reset position and hide
            transform.translation = Vec3::new(-0.3, -0.35, -0.45);
            transform.rotation = Quat::IDENTITY;
            *vis = Visibility::Hidden;
            commands.entity(entity).remove::<MeleeSwing>();
        }
    }
}

// ── Zombies ───────────────────────────────────────────────────────────────────

fn spawn_zombies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spawn_timer: ResMut<ZombieSpawnTimer>,
    game: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    time: Res<Time>,
    map_config: Res<MapConfig>,
) {
    if game.is_dead { return; }
    spawn_timer.0.tick(time.delta());
    if !spawn_timer.0.just_finished() { return; }

    let Ok(player_transform) = player_q.get_single() else { return };
    let mut rng = rand::thread_rng();
    let map_half = map_config.map_half();

    let count = 1 + (game.kills / 10).min(4) as usize;
    for _ in 0..count {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(15.0..35.0);
        let offset = Vec3::new(angle.cos() * dist, 0.0, angle.sin() * dist);
        let spawn_pos = player_transform.translation + offset;
        // Clamp spawn inside map
        let x = spawn_pos.x.clamp(-map_half + 1.0, map_half - 1.0);
        let z = spawn_pos.z.clamp(-map_half + 1.0, map_half - 1.0);

        commands.spawn((
            Zombie {
                path: vec![],
                path_index: 0,
                repath_timer: Timer::from_seconds(0.0, TimerMode::Once),
                idle: false,
                idle_timer: Timer::from_seconds(0.0, TimerMode::Once),
            },
            Mesh3d(meshes.add(Capsule3d::new(0.5, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.1, 0.1),
                ..default()
            })),
            Transform::from_translation(Vec3::new(x, 0.9, z)),
        ));
    }
}

fn zombie_ai(
    mut zombie_q: Query<(&mut Transform, &mut Zombie), Without<Player>>,
    player_q: Query<&Transform, With<Player>>,
    game: Res<GameState>,
    time: Res<Time>,
    nav_grid: Res<NavGrid>,
    map_config: Res<MapConfig>,
) {
    if game.is_dead { return; }
    let Ok(player_transform) = player_q.get_single() else { return };
    let mut rng = rand::thread_rng();

    let speed = game.zombie_speed(&map_config);
    let move_chance = game.zombie_move_chance(&map_config);
    let map_half = map_config.map_half();
    let player_pos = Vec2::new(player_transform.translation.x, player_transform.translation.z);

    for (mut transform, mut zombie) in zombie_q.iter_mut() {
        zombie.repath_timer.tick(time.delta());
        zombie.idle_timer.tick(time.delta());

        // If idle, wait until idle timer expires then decide again
        if zombie.idle {
            if zombie.idle_timer.finished() {
                zombie.idle = false;
                zombie.repath_timer = Timer::from_seconds(0.0, TimerMode::Once);
            } else {
                continue;
            }
        }

        // Time to compute a new path?
        if zombie.repath_timer.finished() {
            let interval = rng.gen_range(0.8..1.5);
            zombie.repath_timer = Timer::from_seconds(interval, TimerMode::Once);

            // Roll: move towards player or idle
            if rng.r#gen::<f32>() > move_chance {
                // Idle for a bit
                zombie.idle = true;
                zombie.idle_timer = Timer::from_seconds(rng.gen_range(0.3..1.0), TimerMode::Once);
                zombie.path.clear();
                continue;
            }

            // A* pathfind towards player
            let zombie_pos = Vec2::new(transform.translation.x, transform.translation.z);
            zombie.path = nav_grid.find_path(zombie_pos, player_pos);
            zombie.path_index = 0;
        }

        // Follow path
        if zombie.path_index < zombie.path.len() {
            let target = zombie.path[zombie.path_index];
            let current = Vec2::new(transform.translation.x, transform.translation.z);
            let to_target = target - current;
            let dist = to_target.length();

            if dist < 0.5 {
                zombie.path_index += 1;
            } else {
                let dir = to_target.normalize();
                let movement = dir * speed * time.delta_secs();
                transform.translation.x += movement.x;
                transform.translation.z += movement.y;
                transform.translation.y = 0.9;

                // Clamp to map bounds
                transform.translation.x = transform.translation.x.clamp(-map_half, map_half);
                transform.translation.z = transform.translation.z.clamp(-map_half, map_half);

                // Face direction of travel
                let dir3 = Vec3::new(dir.x, 0.0, dir.y);
                if dir3.length_squared() > 0.01 {
                    transform.look_to(-dir3, Vec3::Y);
                }
            }
        }
    }
}

// ── Health / Damage ───────────────────────────────────────────────────────────

fn zombie_attack_player(
    zombie_q: Query<&Transform, With<Zombie>>,
    player_q: Query<&Transform, With<Player>>,
    mut game: ResMut<GameState>,
    mut commands: Commands,
    overlay_q: Query<Entity, With<DamageOverlay>>,
    time: Res<Time>,
) {
    if game.is_dead { return; }
    let Ok(player_transform) = player_q.get_single() else { return };

    for zombie_transform in zombie_q.iter() {
        let dist = zombie_transform
            .translation
            .distance(player_transform.translation);
        if dist < 1.2 {
            let now = time.elapsed_secs();

            if now - game.last_hit_time > 5.0 {
                game.hit_count_in_window = 0;
            }

            if now - game.last_hit_time < 1.0 {
                continue;
            }

            game.last_hit_time = now;
            game.hit_count_in_window += 1;
            game.health -= 40.0;

            if let Ok(entity) = overlay_q.get_single() {
                commands.entity(entity).insert(DamageFlash {
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                });
            }

            if game.health <= 0.0 {
                game.is_dead = true;
                game.health = 0.0;
            }

            break;
        }
    }
}

fn health_regen(mut game: ResMut<GameState>, time: Res<Time>) {
    if game.is_dead { return; }
    let now = time.elapsed_secs();
    if now - game.last_hit_time > 5.0 && game.health < 100.0 {
        game.health = (game.health + 5.0 * time.delta_secs()).min(100.0);
    }
}

fn damage_flash(
    mut commands: Commands,
    mut overlay_q: Query<(Entity, &mut BackgroundColor, Option<&mut DamageFlash>), With<DamageOverlay>>,
    time: Res<Time>,
) {
    for (entity, mut bg, flash) in overlay_q.iter_mut() {
        if let Some(mut flash) = flash {
            flash.timer.tick(time.delta());
            let progress = flash.timer.fraction();
            let alpha = 0.45 * (1.0 - progress);
            bg.0 = Color::srgba(1.0, 0.0, 0.0, alpha);

            if flash.timer.finished() {
                bg.0 = Color::srgba(1.0, 0.0, 0.0, 0.0);
                commands.entity(entity).remove::<DamageFlash>();
            }
        }
    }
}

// ── Pause ─────────────────────────────────────────────────────────────────────

fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<GameState>,
    mut pause_menu_q: Query<&mut Visibility, With<PauseMenu>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    game.is_paused = !game.is_paused;

    if let Ok(mut vis) = pause_menu_q.get_single_mut() {
        *vis = if game.is_paused {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }

    if let Ok(mut window) = windows.get_single_mut() {
        if game.is_paused {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        } else {
            window.cursor_options.grab_mode = CursorGrabMode::Locked;
            window.cursor_options.visible = false;
        }
    }
}

fn pause_button_interaction(
    mut interaction_q: Query<
        (&Interaction, &PauseButton, &mut BackgroundColor),
        (Changed<Interaction>, With<Button>),
    >,
    mut game: ResMut<GameState>,
    mut pause_menu_q: Query<&mut Visibility, With<PauseMenu>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    for (interaction, button, mut bg) in interaction_q.iter_mut() {
        let base_color = match button {
            PauseButton::Resume => Color::srgb(0.2, 0.6, 0.2),
            PauseButton::Exit => Color::srgb(0.6, 0.2, 0.2),
        };
        let hover_color = match button {
            PauseButton::Resume => Color::srgb(0.3, 0.8, 0.3),
            PauseButton::Exit => Color::srgb(0.8, 0.3, 0.3),
        };

        match *interaction {
            Interaction::Pressed => {
                match button {
                    PauseButton::Resume => {
                        game.is_paused = false;
                        if let Ok(mut vis) = pause_menu_q.get_single_mut() {
                            *vis = Visibility::Hidden;
                        }
                        if let Ok(mut window) = windows.get_single_mut() {
                            window.cursor_options.grab_mode = CursorGrabMode::Locked;
                            window.cursor_options.visible = false;
                        }
                    }
                    PauseButton::Exit => {
                        game.is_paused = false;
                        next_state.set(AppState::MapSelect);
                    }
                }
            }
            Interaction::Hovered => {
                bg.0 = hover_color;
            }
            Interaction::None => {
                bg.0 = base_color;
            }
        }
    }
}

fn update_health_ui(
    game: Res<GameState>,
    mut health_text: Query<(&mut Text, &mut TextColor), With<HealthText>>,
) {
    if let Ok((mut text, mut color)) = health_text.get_single_mut() {
        if game.is_dead {
            **text = "YOU DIED".to_string();
            color.0 = Color::srgb(1.0, 0.0, 0.0);
        } else {
            **text = format!("HP: {:.0}", game.health);
            let hp_frac = game.health / 100.0;
            color.0 = Color::srgb(1.0 - hp_frac, hp_frac, 0.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    fn test_map_config() -> MapConfig {
        map::default_map_config()
    }

    // ── GameState tests ──

    #[test]
    fn test_zombie_speed_no_kills() {
        let game = GameState::default();
        let config = test_map_config();
        assert_eq!(game.zombie_speed(&config), config.zombies.base_speed);
    }

    #[test]
    fn test_zombie_speed_with_kills() {
        let mut game = GameState::default();
        game.kills = 10;
        let config = test_map_config();
        let expected = config.zombies.base_speed + (10.0 * config.zombies.speed_per_kill).min(config.zombies.max_speed_bonus);
        assert_eq!(game.zombie_speed(&config), expected);
    }

    #[test]
    fn test_zombie_speed_capped_at_max_bonus() {
        let mut game = GameState::default();
        game.kills = 10000;
        let config = test_map_config();
        let expected = config.zombies.base_speed + config.zombies.max_speed_bonus;
        assert_eq!(game.zombie_speed(&config), expected);
    }

    #[test]
    fn test_zombie_move_chance_no_kills() {
        let game = GameState::default();
        let config = test_map_config();
        assert_eq!(game.zombie_move_chance(&config), config.zombies.base_move_chance);
    }

    #[test]
    fn test_zombie_move_chance_with_kills() {
        let mut game = GameState::default();
        game.kills = 20;
        let config = test_map_config();
        let expected = config.zombies.base_move_chance + (20.0 * config.zombies.move_chance_per_kill).min(config.zombies.max_move_chance_bonus);
        assert_eq!(game.zombie_move_chance(&config), expected);
    }

    #[test]
    fn test_zombie_move_chance_capped_at_max_bonus() {
        let mut game = GameState::default();
        game.kills = 100000;
        let config = test_map_config();
        let expected = config.zombies.base_move_chance + config.zombies.max_move_chance_bonus;
        assert_eq!(game.zombie_move_chance(&config), expected);
    }

    // ── Heuristic tests ──

    #[test]
    fn test_heuristic_same_point() {
        assert_eq!(heuristic(5, 5, 5, 5), 0.0);
    }

    #[test]
    fn test_heuristic_cardinal_direction() {
        // Moving 3 cells in one direction: cost = 3.0
        assert_eq!(heuristic(0, 0, 3, 0), 3.0);
        assert_eq!(heuristic(0, 0, 0, 3), 3.0);
    }

    #[test]
    fn test_heuristic_diagonal() {
        // Pure diagonal 3 cells: cost = 3 * 1.414
        let h = heuristic(0, 0, 3, 3);
        assert!((h - 3.0 * 1.414).abs() < 0.001);
    }

    #[test]
    fn test_heuristic_octile_mixed() {
        // 2 diagonal + 1 cardinal = 2*1.414 + 1 = 3.828
        let h = heuristic(0, 0, 3, 2);
        assert!((h - (2.0 * 1.414 + 1.0)).abs() < 0.001);
    }

    // ── NavGrid tests ──

    #[test]
    fn test_nav_grid_new() {
        let grid = NavGrid::new(40.0, 80);
        assert_eq!(grid.blocked.len(), 80 * 80);
        assert_eq!(grid.map_half, 40.0);
        assert_eq!(grid.grid_size, 80);
        assert!(grid.blocked.iter().all(|&b| !b));
    }

    #[test]
    fn test_nav_grid_idx() {
        let grid = NavGrid::new(40.0, 80);
        assert_eq!(grid.idx(0, 0), 0);
        assert_eq!(grid.idx(1, 0), 1);
        assert_eq!(grid.idx(0, 1), 80);
        assert_eq!(grid.idx(5, 3), 3 * 80 + 5);
    }

    #[test]
    fn test_nav_grid_world_to_grid_center() {
        let grid = NavGrid::new(40.0, 80);
        // World (0, 0) -> grid center
        let result = grid.world_to_grid(0.0, 0.0);
        assert!(result.is_some());
        let (gx, gz) = result.unwrap();
        assert_eq!(gx, 40);
        assert_eq!(gz, 40);
    }

    #[test]
    fn test_nav_grid_world_to_grid_out_of_bounds() {
        let grid = NavGrid::new(40.0, 80);
        assert!(grid.world_to_grid(-50.0, 0.0).is_none());
        assert!(grid.world_to_grid(0.0, 50.0).is_none());
        assert!(grid.world_to_grid(100.0, 100.0).is_none());
    }

    #[test]
    fn test_nav_grid_grid_to_world_roundtrip() {
        let grid = NavGrid::new(40.0, 80);
        let world = grid.grid_to_world(40, 40);
        // Should be near center
        assert!((world.x - 0.5).abs() < 0.01);
        assert!((world.y - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_nav_grid_is_blocked() {
        let mut grid = NavGrid::new(40.0, 80);
        assert!(!grid.is_blocked(5, 5));
        let idx = grid.idx(5, 5);
        grid.blocked[idx] = true;
        assert!(grid.is_blocked(5, 5));
    }

    #[test]
    fn test_nav_grid_find_path_unblocked() {
        let grid = NavGrid::new(10.0, 20);
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(5.0, 0.0);
        let path = grid.find_path(start, end);
        assert!(!path.is_empty());
        // Last point should be near the end
        let last = path.last().unwrap();
        assert!((last.x - end.x).abs() < 1.5);
    }

    #[test]
    fn test_nav_grid_find_path_out_of_bounds_returns_empty() {
        let grid = NavGrid::new(10.0, 20);
        let path = grid.find_path(Vec2::new(-20.0, 0.0), Vec2::new(5.0, 0.0));
        assert!(path.is_empty());
    }

    #[test]
    fn test_nav_grid_find_path_blocked_goal_returns_direct() {
        let mut grid = NavGrid::new(10.0, 20);
        let end = Vec2::new(5.0, 0.0);
        if let Some((gx, gz)) = grid.world_to_grid(end.x, end.y) {
            let idx = grid.idx(gx, gz);
            grid.blocked[idx] = true;
        }
        let path = grid.find_path(Vec2::new(0.0, 0.0), end);
        assert_eq!(path.len(), 1);
        assert_eq!(path[0], end);
    }

    #[test]
    fn test_nav_grid_find_path_around_obstacle() {
        let mut grid = NavGrid::new(10.0, 20);
        // Block a vertical wall at x=0 (grid column 10)
        for gz in 5..15 {
            let idx = grid.idx(10, gz);
            grid.blocked[idx] = true;
        }
        let start = Vec2::new(-3.0, 0.0);
        let end = Vec2::new(3.0, 0.0);
        let path = grid.find_path(start, end);
        assert!(!path.is_empty());
    }

    // ── AStarNode ordering tests ──

    #[test]
    fn test_astar_node_ordering_min_f_first() {
        use std::collections::BinaryHeap;
        let mut heap = BinaryHeap::new();
        heap.push(AStarNode { gx: 0, gz: 0, f: 10.0 });
        heap.push(AStarNode { gx: 1, gz: 1, f: 5.0 });
        heap.push(AStarNode { gx: 2, gz: 2, f: 15.0 });
        let best = heap.pop().unwrap();
        assert_eq!(best.f, 5.0);
        assert_eq!(best.gx, 1);
    }

    #[test]
    fn test_astar_node_equality() {
        let a = AStarNode { gx: 3, gz: 4, f: 10.0 };
        let b = AStarNode { gx: 3, gz: 4, f: 20.0 };
        assert_eq!(a, b); // Equality is based on position, not f
    }

    // ── Collision tests ──

    #[test]
    fn test_collides_with_tree_no_collision() {
        let trees = vec![Vec2::new(10.0, 10.0)];
        let result = collides_with_tree(Vec2::new(0.0, 0.0), 0.4, &trees, 1.2);
        assert!(result.is_none());
    }

    #[test]
    fn test_collides_with_tree_collision() {
        let trees = vec![Vec2::new(1.0, 0.0)];
        let result = collides_with_tree(Vec2::new(0.5, 0.0), 0.4, &trees, 1.2);
        assert!(result.is_some());
        let push = result.unwrap();
        // Push should be in the -x direction (away from tree)
        assert!(push.x < 0.0);
    }

    #[test]
    fn test_collides_with_tree_exact_overlap_ignored() {
        // When distance is nearly zero, the function skips (dist > 0.001 check)
        let trees = vec![Vec2::new(0.0, 0.0)];
        let result = collides_with_tree(Vec2::new(0.0, 0.0), 0.4, &trees, 1.2);
        assert!(result.is_none());
    }

    #[test]
    fn test_collides_with_tree_multiple_trees() {
        let trees = vec![
            Vec2::new(10.0, 10.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(20.0, 20.0),
        ];
        let result = collides_with_tree(Vec2::new(0.5, 0.0), 0.4, &trees, 1.2);
        assert!(result.is_some());
    }

    #[test]
    fn test_collide_with_walls_no_collision() {
        let walls = HouseWalls(vec![WallRect {
            min_x: 10.0, max_x: 11.0,
            min_z: 10.0, max_z: 11.0,
            min_y: 0.0, max_y: 3.0,
        }]);
        let mut pos = Vec3::new(0.0, 1.0, 0.0);
        let original = pos;
        collide_with_walls(&mut pos, 0.4, &walls);
        assert_eq!(pos, original);
    }

    #[test]
    fn test_collide_with_walls_pushes_out() {
        let walls = HouseWalls(vec![WallRect {
            min_x: -1.0, max_x: 1.0,
            min_z: -1.0, max_z: 1.0,
            min_y: 0.0, max_y: 3.0,
        }]);
        let mut pos = Vec3::new(0.9, 1.0, 0.0);
        collide_with_walls(&mut pos, 0.4, &walls);
        // Should be pushed out to x = 1.0 + 0.4 = 1.4
        assert!((pos.x - 1.4).abs() < 0.01);
    }

    #[test]
    fn test_collide_with_walls_y_out_of_range() {
        let walls = HouseWalls(vec![WallRect {
            min_x: -1.0, max_x: 1.0,
            min_z: -1.0, max_z: 1.0,
            min_y: 0.0, max_y: 3.0,
        }]);
        let mut pos = Vec3::new(0.0, 5.0, 0.0); // Above the wall
        let original = pos;
        collide_with_walls(&mut pos, 0.4, &walls);
        assert_eq!(pos, original);
    }

    // ── GameState default tests ──

    #[test]
    fn test_game_state_default() {
        let game = GameState::default();
        assert_eq!(game.kills, 0);
        assert_eq!(game.score, 0);
        assert_eq!(game.health, 0.0);
        assert!(!game.is_dead);
        assert!(!game.is_paused);
    }
}
