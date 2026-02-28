# ZomVibe 🧟

> A first-person 3D zombie shooter built in Rust with the Bevy game engine.

---

## The Idea

You're alone. The world is quiet — green grass stretching out in every direction, broken only by the dark silhouettes of trees on the horizon.

Then they come.

ZomVibe is a survival shooter where you hold off endless waves of zombies for as long as you can. The longer you survive, the faster they get. The faster they get, the more they swarm. The more they swarm... well, you get the idea.

There's no winning. Only holding on.

---

## Controls

| Input | Action |
|---|---|
| `W A S D` | Move |
| Mouse | Look around |
| `Left Click` | Shoot |

---

## What's In The Game

### World
- **Green grass ground** stretching 200 units in every direction
- **12 tree obstacles** scattered across the map — brown box trunks with green canopy tops — blocking sightlines and forcing you to navigate

### Player
- **First-person perspective** with a locked cursor
- **Smooth WASD movement** relative to where you're looking
- **Mouse look** — horizontal yaw on the player body, vertical pitch clamped to prevent flipping
- **Black gun mesh** visible in the bottom-right of your view, parented to the camera so it moves with your aim

### Shooting
- **Unlimited ammo** — click to fire
- **Yellow glowing bullets** that travel at 30 units/sec
- **2 second bullet lifetime** before despawning
- **1 unit hit radius** on zombies — clean instant kills

### Zombies
- **Red capsule enemies** spawning at random angles, 20–40 units from the player
- **Wave spawning** — zombies arrive in groups, with more per wave as your kill count climbs
- **Tick-based AI** — each update tick, every zombie independently rolls three outcomes:
  - Chase the player (pathfind directly toward you)
  - Wander in a random direction
  - Stay still

### Difficulty Scaling
As you kill more zombies, the horde adapts:

| Kills | Speed | Move Frequency | Chase Tendency |
|---|---|---|---|
| 0 | 2.0 | 60% | 50% |
| 20 | 3.0 | 70% | 66% |
| 60 | 5.0 | 85% | 98% |
| 80+ | 6.0 (cap) | 95% (cap) | 95% (cap) |

### Health & Damage
- **100 HP** to start
- Zombies deal **40 damage** on contact (once per second per contact)
- **Health regenerates** at 5 HP/sec after 5 seconds without being hit
- **Red screen flash** — a translucent red overlay pulses across your screen every time you take damage, fading over 0.4 seconds
- **Death** — movement stops, zombie spawning halts, "YOU DIED" displayed in red

### HUD
- **Kill counter** in the top-left
- **Crosshair** centered on screen
- **HP display** in the bottom-left — color shifts from green to red as health drops

---

## Next Steps

### Gameplay
- [ ] **Melee attack** — a secondary attack when zombies are too close
- [ ] **Multiple weapons** — shotgun, rifle, pistol with different fire rates and ammo types
- [ ] **Ammo pickups** — find ammo scattered around the map or dropped by zombies
- [ ] **Score system** — time survived, accuracy rating, kill streaks
- [ ] **Zombie variants** — fast/fragile runners, slow/tanky brutes, ranged spitters

### World
- [ ] **Collision detection** — currently players and zombies can walk through trees
- [ ] **Map boundaries** — prevent walking off the edge of the world
- [ ] **Day/night cycle** — visibility and zombie behavior change over time
- [ ] **Procedural map generation** — different layouts each run
- [ ] **Destructible environment** — chop down trees, break fences

### Visuals & Feel
- [ ] **Proper 3D models** — replace capsules and boxes with actual character meshes
- [ ] **Muzzle flash** on the gun when firing
- [ ] **Blood splatter** particle effect on zombie hit
- [ ] **Zombie death animation** — ragdoll or dissolve on kill
- [ ] **Footstep sounds** and directional audio for approaching zombies
- [ ] **Fog of war** — limited visibility adding tension
- [ ] **Screen shake** on damage

### AI & Systems
- [ ] **True pathfinding** — A* or navmesh so zombies navigate around obstacles rather than walking into trees
- [ ] **Zombie grouping behavior** — pack tactics, flanking
- [ ] **Zombie line-of-sight** — they only chase when they can see or hear you
- [ ] **Save system** — persist high scores between sessions
- [ ] **Pause menu** with restart and quit options

### Polish
- [ ] **Main menu** screen
- [ ] **Game over screen** with final stats
- [ ] **Settings** — mouse sensitivity, FOV, keybinds
- [ ] **Sound effects and music** — ambient dread, gunshots, zombie groans

---

## Tech Stack

- **[Rust](https://www.rust-lang.org/)** — systems language with no garbage collector, perfect for game loops
- **[Bevy 0.15](https://bevyengine.org/)** — data-driven ECS game engine built for Rust
- **[rand 0.8](https://docs.rs/rand)** — zombie spawn positions, AI decision rolls

- https://github.com/cBournhonesque/lightyear

---

## Running Locally

```bash
# Clone the repo
git clone <repo-url>
cd zomvibe

# Run (first build will take a minute — Bevy is a big engine)
cargo run
```

> Tip: run with `cargo run --release` for a significantly faster experience once you're past initial development.

---

## License

Do what you want with it. Survive as long as you can.
