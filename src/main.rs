use bevy::{
    input::mouse::MouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};
use rand::Rng;

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
        .add_systems(Startup, (setup_scene, setup_ui, grab_cursor))
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
            ),
        )
        .run();
}

// ── Components ──────────────────────────────────────────────────────────────

#[derive(Component)]
struct Player;

#[derive(Component)]
struct CameraAnchor;

#[derive(Component)]
struct Zombie;

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
struct DamageFlash {
    timer: Timer,
}

#[derive(Component)]
struct DamageOverlay;

// ── Resources ───────────────────────────────────────────────────────────────

#[derive(Resource, Default)]
struct GameState {
    kills: u32,
    health: f32,
    last_hit_time: f32,
    hit_count_in_window: u32,
    is_dead: bool,
}

impl GameState {
    fn zombie_speed(&self) -> f32 {
        2.0 + (self.kills as f32 * 0.05).min(4.0)
    }

    fn zombie_move_chance(&self) -> f32 {
        0.6 + (self.kills as f32 * 0.005).min(0.35)
    }

    fn zombie_chase_chance(&self) -> f32 {
        0.5 + (self.kills as f32 * 0.008).min(0.45)
    }
}

#[derive(Resource)]
struct ZombieSpawnTimer(Timer);

// ── Setup ────────────────────────────────────────────────────────────────────

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut game: ResMut<GameState>,
) {
    game.health = 100.0;

    // Ground (green grass)
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.6, 0.15),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // Trees (brown boxes scattered around)
    let tree_positions = [
        (10.0_f32, 10.0_f32),
        (-15.0, 8.0),
        (5.0, -20.0),
        (-8.0, -12.0),
        (20.0, -5.0),
        (-20.0, 15.0),
        (15.0, 20.0),
        (-25.0, -20.0),
        (30.0, 10.0),
        (0.0, 25.0),
        (-30.0, 5.0),
        (12.0, -30.0),
    ];

    for (x, z) in tree_positions {
        // Trunk
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(1.0, 4.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.45, 0.25, 0.1),
                ..default()
            })),
            Transform::from_xyz(x, 2.0, z),
        ));
        // Canopy
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(3.0, 3.0, 3.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.1, 0.45, 0.1),
                ..default()
            })),
            Transform::from_xyz(x, 5.5, z),
        ));
    }

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

    // Player body (blue oval - visible in 3rd person debug; first-person so mostly hidden)
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
            // Top bar - kills
            parent
                .spawn(Node {
                    padding: UiRect::all(Val::Px(12.0)),
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

            // Bottom bar - health
            parent
                .spawn(Node {
                    padding: UiRect::all(Val::Px(12.0)),
                    justify_content: JustifyContent::FlexEnd,
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        HealthText,
                        Text::new("HP: 100"),
                        TextFont { font_size: 24.0, ..default() },
                        TextColor(Color::srgb(0.2, 1.0, 0.2)),
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
}

fn grab_cursor(mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = windows.get_single_mut() {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
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

    // Yaw on player (left/right)
    if let Ok(mut pt) = player_q.get_single_mut() {
        pt.rotate_y(-delta.x * sensitivity);
    }

    // Pitch on camera anchor (up/down), clamped
    if let Ok(mut at) = anchor_q.get_single_mut() {
        let current_pitch = at.rotation.to_euler(EulerRot::XYZ).0;
        let new_pitch = (current_pitch - delta.y * sensitivity).clamp(-1.4, 1.4);
        at.rotation = Quat::from_rotation_x(new_pitch);
    }
}

fn player_move(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    game: Res<GameState>,
    mut player_q: Query<&mut Transform, With<Player>>,
) {
    if game.is_dead {
        return;
    }
    let Ok(mut transform) = player_q.get_single_mut() else {
        return;
    };

    let speed = 5.0;
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
        transform.translation += velocity.normalize() * speed * time.delta_secs();
        transform.translation.y = 0.9; // stay on ground
    }
}

// ── Shooting ──────────────────────────────────────────────────────────────────

fn shoot(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mouse: Res<ButtonInput<MouseButton>>,
    game: Res<GameState>,
    player_q: Query<&Transform, With<Player>>,
    anchor_q: Query<&Transform, (With<CameraAnchor>, Without<Player>)>,
) {
    if game.is_dead || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Ok(pt) = player_q.get_single() else { return };
    let Ok(at) = anchor_q.get_single() else { return };

    // World-space shoot direction = player yaw * anchor pitch * forward
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
    mut kill_text: Query<&mut Text, With<KillText>>,
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
                if let Ok(mut text) = kill_text.get_single_mut() {
                    **text = format!("Kills: {}", game.kills);
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

    // Spawn 1-3 zombies each wave, more as kills increase
    let count = 1 + (game.kills / 10).min(4) as usize;
    for _ in 0..count {
        let angle = rng.gen_range(0.0..std::f32::consts::TAU);
        let dist = rng.gen_range(20.0..40.0);
        let offset = Vec3::new(angle.cos() * dist, 0.0, angle.sin() * dist);
        let spawn_pos = player_transform.translation + offset;

        commands.spawn((
            Zombie,
            Mesh3d(meshes.add(Capsule3d::new(0.5, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.8, 0.1, 0.1),
                ..default()
            })),
            Transform::from_translation(Vec3::new(spawn_pos.x, 0.9, spawn_pos.z)),
        ));
    }
}

fn zombie_ai(
    mut zombie_q: Query<&mut Transform, (With<Zombie>, Without<Player>)>,
    player_q: Query<&Transform, With<Player>>,
    game: Res<GameState>,
    time: Res<Time>,
) {
    if game.is_dead { return; }
    let Ok(player_transform) = player_q.get_single() else { return };
    let mut rng = rand::thread_rng();

    let speed = game.zombie_speed();
    let move_chance = game.zombie_move_chance();
    let chase_chance = game.zombie_chase_chance();

    for mut zombie_transform in zombie_q.iter_mut() {
        // Each tick: roll whether the zombie moves at all
        if rng.r#gen::<f32>() > move_chance {
            continue; // no movement this tick
        }

        let direction = if rng.r#gen::<f32>() < chase_chance {
            // Chase player
            let to_player = player_transform.translation - zombie_transform.translation;
            if to_player.length_squared() > 0.01 {
                let mut d = to_player.normalize();
                d.y = 0.0;
                d
            } else {
                Vec3::ZERO
            }
        } else {
            // Random wander
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            Vec3::new(angle.cos(), 0.0, angle.sin())
        };

        zombie_transform.translation += direction * speed * time.delta_secs();
        zombie_transform.translation.y = 0.9;

        // Face direction of travel
        if direction.length_squared() > 0.01 {
            zombie_transform.look_to(-direction, Vec3::Y);
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

            // Track hits within 5-second window
            if now - game.last_hit_time > 5.0 {
                game.hit_count_in_window = 0;
            }

            // Limit to once per second per contact
            if now - game.last_hit_time < 1.0 {
                continue;
            }

            game.last_hit_time = now;
            game.hit_count_in_window += 1;
            game.health -= 40.0;

            // Trigger flash
            if let Ok(entity) = overlay_q.get_single() {
                commands.entity(entity).insert(DamageFlash {
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                });
            }

            if game.health <= 0.0 {
                game.is_dead = true;
                game.health = 0.0;
            }

            break; // one hit per frame
        }
    }
}

fn health_regen(mut game: ResMut<GameState>, time: Res<Time>) {
    if game.is_dead { return; }
    let now = time.elapsed_secs();
    // Regen starts 5 seconds after last hit
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
            // Fade from 0.45 alpha to 0 as timer progresses
            let alpha = 0.45 * (1.0 - progress);
            bg.0 = Color::srgba(1.0, 0.0, 0.0, alpha);

            if flash.timer.finished() {
                bg.0 = Color::srgba(1.0, 0.0, 0.0, 0.0);
                commands.entity(entity).remove::<DamageFlash>();
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
