use bevy::{
    input::mouse::MouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};
use rand::Rng;
use std::collections::BinaryHeap;
use std::cmp::Ordering;

const MAP_HALF: f32 = 40.0;
const GRID_CELL: f32 = 1.0;
const GRID_SIZE: usize = (MAP_HALF as usize) * 2; // 80x80 grid
const TREE_RADIUS: f32 = 1.2; // collision radius for trees (trunk ~0.8 wide)
const PLAYER_RADIUS: f32 = 0.4;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "ZomVibe".to_string(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(GameState::default())
        .insert_resource(ZombieSpawnTimer(Timer::from_seconds(3.0, TimerMode::Repeating)))
        .insert_resource(NavGrid::default())
        .insert_resource(TreePositions::default())
        .add_systems(Startup, (setup_scene, setup_ui, grab_cursor))
        .add_systems(Update, (toggle_pause, pause_button_interaction))
        .add_systems(
            Update,
            (
                player_look,
                player_move,
                shoot,
                spawn_zombies,
                zombie_ai,
                bullet_movement,
                check_bullet_zombie_collision,
                zombie_attack_player,
                update_health_ui,
                health_regen,
                damage_flash,
            )
                .run_if(game_active),
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
    health: f32,
    last_hit_time: f32,
    hit_count_in_window: u32,
    is_dead: bool,
    is_paused: bool,
    vertical_velocity: f32,
    is_grounded: bool,
}

impl GameState {
    fn zombie_speed(&self) -> f32 {
        4.5 + (self.kills as f32 * 0.08).min(5.0)
    }

    fn zombie_move_chance(&self) -> f32 {
        0.6 + (self.kills as f32 * 0.005).min(0.35)
    }
}

fn game_active(game: Res<GameState>) -> bool {
    !game.is_paused
}

#[derive(Resource)]
struct ZombieSpawnTimer(Timer);

#[derive(Resource, Default)]
struct TreePositions(Vec<Vec2>);

// ── A* Navigation Grid ──────────────────────────────────────────────────────

#[derive(Resource)]
struct NavGrid {
    blocked: Vec<bool>, // GRID_SIZE * GRID_SIZE
}

impl Default for NavGrid {
    fn default() -> Self {
        Self {
            blocked: vec![false; GRID_SIZE * GRID_SIZE],
        }
    }
}

impl NavGrid {
    fn world_to_grid(world_x: f32, world_z: f32) -> Option<(usize, usize)> {
        let gx = ((world_x + MAP_HALF) / GRID_CELL) as i32;
        let gz = ((world_z + MAP_HALF) / GRID_CELL) as i32;
        if gx >= 0 && gx < GRID_SIZE as i32 && gz >= 0 && gz < GRID_SIZE as i32 {
            Some((gx as usize, gz as usize))
        } else {
            None
        }
    }

    fn grid_to_world(gx: usize, gz: usize) -> Vec2 {
        Vec2::new(
            gx as f32 * GRID_CELL - MAP_HALF + GRID_CELL * 0.5,
            gz as f32 * GRID_CELL - MAP_HALF + GRID_CELL * 0.5,
        )
    }

    fn idx(gx: usize, gz: usize) -> usize {
        gz * GRID_SIZE + gx
    }

    fn is_blocked(&self, gx: usize, gz: usize) -> bool {
        self.blocked[Self::idx(gx, gz)]
    }

    fn find_path(&self, start: Vec2, end: Vec2) -> Vec<Vec2> {
        let Some((sx, sz)) = Self::world_to_grid(start.x, start.y) else {
            return vec![];
        };
        let Some((ex, ez)) = Self::world_to_grid(end.x, end.y) else {
            return vec![];
        };

        // If goal is blocked, just go direct
        if self.is_blocked(ex, ez) {
            return vec![end];
        }

        let mut open = BinaryHeap::new();
        let total = GRID_SIZE * GRID_SIZE;
        let mut g_score = vec![f32::INFINITY; total];
        let mut came_from = vec![(usize::MAX, usize::MAX); total];
        let mut closed = vec![false; total];

        g_score[Self::idx(sx, sz)] = 0.0;
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

            let ci = Self::idx(cx, cz);
            if closed[ci] {
                continue;
            }
            closed[ci] = true;

            for (dx, dz, cost) in &neighbors {
                let nx = cx as i32 + dx;
                let nz = cz as i32 + dz;
                if nx < 0 || nx >= GRID_SIZE as i32 || nz < 0 || nz >= GRID_SIZE as i32 {
                    continue;
                }
                let nx = nx as usize;
                let nz = nz as usize;
                let ni = Self::idx(nx, nz);

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
            let ci = Self::idx(cur.0, cur.1);
            let prev = came_from[ci];
            if prev == (usize::MAX, usize::MAX) {
                break;
            }
            cur = prev;
        }
        path_grid.reverse();

        // Convert to world coords, skip first few close waypoints
        path_grid.iter().map(|&(gx, gz)| Self::grid_to_world(gx, gz)).collect()
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

#[derive(Clone)]
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

// ── Setup ────────────────────────────────────────────────────────────────────

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut game: ResMut<GameState>,
    mut nav_grid: ResMut<NavGrid>,
    mut tree_positions: ResMut<TreePositions>,
) {
    game.health = 100.0;
    game.ammo = 100;
    game.is_grounded = true;

    // Ground (green grass)
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(MAP_HALF * 2.0, MAP_HALF * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.6, 0.15),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // Walls around the map
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.35),
        ..default()
    });
    let wall_height = 4.0;
    let wall_thickness = 1.0;
    let walls = [
        (MAP_HALF * 2.0 + wall_thickness * 2.0, wall_height, wall_thickness, 0.0, wall_height / 2.0, MAP_HALF + wall_thickness / 2.0),
        (MAP_HALF * 2.0 + wall_thickness * 2.0, wall_height, wall_thickness, 0.0, wall_height / 2.0, -(MAP_HALF + wall_thickness / 2.0)),
        (wall_thickness, wall_height, MAP_HALF * 2.0, MAP_HALF + wall_thickness / 2.0, wall_height / 2.0, 0.0),
        (wall_thickness, wall_height, MAP_HALF * 2.0, -(MAP_HALF + wall_thickness / 2.0), wall_height / 2.0, 0.0),
    ];
    for (w, h, d, x, y, z) in walls {
        commands.spawn((
            Wall,
            Mesh3d(meshes.add(Cuboid::new(w, h, d))),
            MeshMaterial3d(wall_material.clone()),
            Transform::from_xyz(x, y, z),
        ));
    }

    // Randomly placed trees (dense)
    let mut rng = rand::thread_rng();
    let trunk_mesh = meshes.add(Cuboid::new(0.8, 4.0, 0.8));
    let canopy_mesh = meshes.add(Cuboid::new(2.5, 2.5, 2.5));
    let trunk_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.25, 0.1),
        ..default()
    });
    let canopy_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.45, 0.1),
        ..default()
    });
    let tree_count = 50;
    let mut trees: Vec<Vec2> = Vec::new();
    let mut attempts = 0;
    while trees.len() < tree_count && attempts < 500 {
        attempts += 1;
        let x = rng.gen_range(-(MAP_HALF - 2.0)..(MAP_HALF - 2.0));
        let z = rng.gen_range(-(MAP_HALF - 2.0)..(MAP_HALF - 2.0));
        // Keep a clear area around the player spawn
        if x.abs() < 6.0 && z.abs() < 6.0 {
            continue;
        }
        // Minimum spacing between trees
        let too_close = trees.iter().any(|t| t.distance(Vec2::new(x, z)) < 3.5);
        if too_close {
            continue;
        }
        trees.push(Vec2::new(x, z));
    }

    for &pos in &trees {
        commands.spawn((
            Mesh3d(trunk_mesh.clone()),
            MeshMaterial3d(trunk_material.clone()),
            Transform::from_xyz(pos.x, 2.0, pos.y),
        ));
        commands.spawn((
            Mesh3d(canopy_mesh.clone()),
            MeshMaterial3d(canopy_material.clone()),
            Transform::from_xyz(pos.x, 5.0, pos.y),
        ));

        // Mark nav grid cells as blocked around this tree
        let radius_cells = (TREE_RADIUS / GRID_CELL).ceil() as i32 + 1;
        if let Some((cx, cz)) = NavGrid::world_to_grid(pos.x, pos.y) {
            for dx in -radius_cells..=radius_cells {
                for dz in -radius_cells..=radius_cells {
                    let nx = cx as i32 + dx;
                    let nz = cz as i32 + dz;
                    if nx >= 0 && nx < GRID_SIZE as i32 && nz >= 0 && nz < GRID_SIZE as i32 {
                        let world = NavGrid::grid_to_world(nx as usize, nz as usize);
                        if world.distance(pos) < TREE_RADIUS {
                            nav_grid.blocked[NavGrid::idx(nx as usize, nz as usize)] = true;
                        }
                    }
                }
            }
        }
    }

    tree_positions.0 = trees;

    // Directional light (sun)
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.8, 0.4, 0.0)),
    ));

    // Ambient light
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
    });

    // Player body
    let player_entity = commands
        .spawn((
            Player,
            Transform::from_xyz(0.0, 0.9, 0.0),
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
                        Text::new("Ammo: 100"),
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

fn collides_with_tree(pos: Vec2, radius: f32, trees: &[Vec2]) -> Option<Vec2> {
    for &tree in trees {
        let diff = pos - tree;
        let dist = diff.length();
        let min_dist = radius + TREE_RADIUS;
        if dist < min_dist && dist > 0.001 {
            // Push out direction
            return Some(diff.normalize() * (min_dist - dist));
        }
    }
    None
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
) {
    if game.is_dead {
        return;
    }
    let Ok(mut transform) = player_q.get_single_mut() else {
        return;
    };

    let sprinting = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let speed = if sprinting { 9.0 } else { 5.0 };
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
        if let Some(push) = collides_with_tree(player_pos, PLAYER_RADIUS, &tree_positions.0) {
            transform.translation.x += push.x;
            transform.translation.z += push.y;
        }
    }

    // Clamp to map bounds
    let map_bound = MAP_HALF - 0.5;
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

    let ground_y = 0.9;
    if transform.translation.y <= ground_y {
        transform.translation.y = ground_y;
        game.vertical_velocity = 0.0;
        game.is_grounded = true;
    }
}

// ── Shooting ──────────────────────────────────────────────────────────────────

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
    if game.ammo == 0 {
        return;
    }
    game.ammo -= 1;
    if let Ok(mut text) = ammo_text.get_single_mut() {
        **text = format!("Ammo: {}", game.ammo);
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

// ── Zombies ───────────────────────────────────────────────────────────────────

fn spawn_zombies(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut spawn_timer: ResMut<ZombieSpawnTimer>,
    game: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    time: Res<Time>,
) {
    if game.is_dead { return; }
    spawn_timer.0.tick(time.delta());
    if !spawn_timer.0.just_finished() { return; }

    let Ok(player_transform) = player_q.get_single() else { return };
    let mut rng = rand::thread_rng();

    let count = 1 + (game.kills / 10).min(4) as usize;
    for _ in 0..count {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(15.0..35.0);
        let offset = Vec3::new(angle.cos() * dist, 0.0, angle.sin() * dist);
        let spawn_pos = player_transform.translation + offset;
        // Clamp spawn inside map
        let x = spawn_pos.x.clamp(-MAP_HALF + 1.0, MAP_HALF - 1.0);
        let z = spawn_pos.z.clamp(-MAP_HALF + 1.0, MAP_HALF - 1.0);

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
) {
    if game.is_dead { return; }
    let Ok(player_transform) = player_q.get_single() else { return };
    let mut rng = rand::thread_rng();

    let speed = game.zombie_speed();
    let move_chance = game.zombie_move_chance();
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
                transform.translation.x = transform.translation.x.clamp(-MAP_HALF, MAP_HALF);
                transform.translation.z = transform.translation.z.clamp(-MAP_HALF, MAP_HALF);

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
    if game.is_dead {
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
    mut exit: EventWriter<AppExit>,
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
                        exit.send(AppExit::Success);
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
