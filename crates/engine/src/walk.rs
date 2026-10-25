//! Walking character controller: a box that runs, jumps, falls, climbs
//! steps and slides along walls, with Quake's movement math. How it
//! feels is data ([`WalkSettings`]); `arena()` and `survival()` are two
//! ready-made profiles, and any field can be changed for a custom one.
//!
//! One tick:
//!
//! ```text
//!   look ─► on ground? ──yes─► jump? ─► friction ─► accelerate ─► step-slide
//!                │                                                  │
//!                no ─► air accelerate (capped) ─► gravity ─► slide  │
//!                                                   │               │
//!                                 snap down onto stairs and slopes ◄┘
//! ```
//!
//! Air acceleration is capped at `air_speed_cap` along the wish
//! direction but not across it, which is what makes Quake's
//! strafe-jumping possible in the Arena profile.

use std::f32::consts::PI;

use base::Key;
use glam::{Vec2, Vec3};
use physics::mover;

use crate::collision::Aabb;
use crate::gfx::Camera;
use crate::input::Input;
use crate::look::{Look, LookSettings, MouseLook};
use crate::world::World;

const JUMP_KEY: Key = Key::Space;
const SPRINT_KEY: Key = Key::LShift;
/// Spawned this far above the floor, so the box never starts touching it
/// (touching counts as inside).
const SPAWN_GAP: f32 = 0.01;
/// Horizontal speeds below this (m/s) count as standing still.
const MIN_SPEED: f32 = 1e-3;
/// After running dry, sprint returns once stamina refills this far.
const SPRINT_RESUME: f32 = 0.25;

/// Faster movement while Left Shift is held, limited by stamina. Once
/// the bar runs dry, sprint is off until it refills to a quarter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprint {
    /// Speed multiplier while sprinting.
    pub multiplier: f32,
    /// Seconds of sprint on a full stamina bar.
    pub duration: f32,
    /// Seconds to refill an empty bar while not sprinting.
    pub recovery: f32,
}

/// Camera bob while walking on the ground.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeadBob {
    /// Deepest dip, meters, at full speed.
    pub height: f32,
    /// Meters walked per dip: one footstep.
    pub step_length: f32,
}

/// Movement tuning. Speeds in m/s, accelerations in m/s².
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WalkSettings {
    /// Half-size of the collision box.
    pub half_size: Vec3,
    /// Camera height above the feet.
    pub eye_height: f32,
    /// Tallest ledge walked up without jumping, meters.
    pub step_height: f32,
    /// Top ground speed.
    pub speed: f32,
    /// How fast ground speed builds toward `speed`, per second: 10 means
    /// a tenth of a second from standstill.
    pub accel: f32,
    /// How fast ground speed bleeds off without input.
    pub friction: f32,
    /// Below this speed, friction acts as if moving at it, so stops are
    /// crisp instead of a long slow creep.
    pub stop_speed: f32,
    /// Steering in the air; 0 for none.
    pub air_accel: f32,
    /// Speed air steering can add along the wish direction.
    pub air_speed_cap: f32,
    /// Downward acceleration.
    pub gravity: f32,
    /// Upward speed at takeoff; jump height is speed² / (2 × gravity).
    pub jump_speed: f32,
    /// Sprinting, if this profile has it.
    pub sprint: Option<Sprint>,
    /// Head bob, if this profile has it.
    pub bob: Option<HeadBob>,
    /// Mouse and arrow-key look, field of view.
    pub look: LookSettings,
}

/// A person-sized box: 0.6 × 1.8 m, eyes at 1.6 m, 45 cm steps. Shared
/// by both profiles so switching never leaves the player stuck.
const HALF_SIZE: Vec3 = Vec3::new(0.3, 0.9, 0.3);
const EYE_HEIGHT: f32 = 1.6;
const STEP_HEIGHT: f32 = 0.45;

impl WalkSettings {
    /// Quake-like: fast (8 m/s), floaty 1.2 m jumps, strong air control
    /// with strafe-jumping. No sprint or head bob.
    pub fn arena() -> Self {
        Self {
            half_size: HALF_SIZE,
            eye_height: EYE_HEIGHT,
            step_height: STEP_HEIGHT,
            speed: 8.0,
            accel: 10.0,
            friction: 4.0,
            stop_speed: 2.5,
            air_accel: 10.0,
            air_speed_cap: 0.76,
            gravity: 20.0,
            jump_speed: 6.9,
            sprint: None,
            bob: None,
            look: LookSettings::default(),
        }
    }

    /// Slow and weighty: 3 m/s walk, a 5 s sprint at 1.8×, low jumps, no
    /// air control, head bob.
    pub fn survival() -> Self {
        Self {
            speed: 3.0,
            accel: 8.0,
            friction: 6.0,
            stop_speed: 1.5,
            air_accel: 0.0,
            air_speed_cap: 0.0,
            gravity: 15.0,
            jump_speed: 4.2,
            sprint: Some(Sprint {
                multiplier: 1.8,
                duration: 5.0,
                recovery: 8.0,
            }),
            bob: Some(HeadBob {
                height: 0.04,
                step_length: 0.75,
            }),
            ..Self::arena()
        }
    }
}

/// Position, orientation and bob at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    /// Center of the collision box.
    center: Vec3,
    look: Look,
    /// Camera offset from head bob, meters (≤ 0).
    bob: f32,
}

impl Pose {
    fn lerp(&self, to: &Pose, t: f32) -> Pose {
        Pose {
            center: self.center.lerp(to.center, t),
            look: self.look.lerp(&to.look, t),
            bob: self.bob + (to.bob - self.bob) * t,
        }
    }
}

/// Walking player. Bindings, read every tick:
///
/// ```text
/// W / S           walk forward / back, level whatever the view tilt
/// A / D           strafe left / right
/// Space           jump (on the ground, once per press)
/// Left Shift      held: sprint, if the profile has it and stamina lasts
/// ← → / ↑ ↓       turn / look up-down
/// mouse           turn and look, only with MouseLook::Enabled
/// ```
pub struct Walker {
    /// Tuning; change freely between ticks (e.g. switch profiles).
    pub settings: WalkSettings,
    prev: Pose,
    current: Pose,
    velocity: Vec3,
    on_ground: bool,
    /// Sprint left, 0..1.
    stamina: f32,
    /// Ran dry; no sprint until stamina reaches SPRINT_RESUME.
    winded: bool,
    /// Ground distance covered, meters: drives head bob.
    walked: f32,
}

impl Walker {
    /// Walker standing at `feet`, facing `yaw` / `pitch` radians.
    pub fn new(feet: Vec3, yaw: f32, pitch: f32, settings: WalkSettings) -> Self {
        let pose = Pose {
            center: feet + Vec3::Y * (settings.half_size.y + SPAWN_GAP),
            look: Look::new(yaw, pitch),
            bob: 0.0,
        };

        Self {
            settings,
            prev: pose,
            current: pose,
            velocity: Vec3::ZERO,
            on_ground: false,
            stamina: 1.0,
            winded: false,
            walked: 0.0,
        }
    }

    /// Advances one tick: look, move, collide against `world`.
    pub fn update(&mut self, input: &Input, mouse: MouseLook, world: &World, dt: f32) {
        self.prev = self.current;
        self.current
            .look
            .turn(input, mouse, &self.settings.look, dt);

        let s = self.settings;
        let physics = world.physics();
        let start = self.current.center;
        let ground = mover::ground(physics, start, s.half_size);

        let (wish_dir, wish_speed) = self.wish(input, ground.is_some(), dt);
        let mut velocity = self.velocity;

        let jumped = ground.is_some() && input.was_pressed(JUMP_KEY);
        let moved = match ground {
            Some(floor) if !jumped => {
                velocity = friction(velocity, s.friction, s.stop_speed, dt);
                velocity = accelerate(velocity, wish_dir, wish_speed, s.accel, dt);
                velocity = along_plane(velocity, floor.normal);
                mover::step_slide(physics, start, velocity, s.half_size, dt, s.step_height)
            }
            _ => {
                if jumped {
                    velocity.y = s.jump_speed;
                }
                velocity = air_accelerate(velocity, wish_dir, wish_speed, &s, dt);
                velocity.y -= s.gravity * dt;
                mover::slide(physics, start, velocity, s.half_size, dt)
            }
        };

        let mut center = moved.position;
        // Walking off a step or down a slope: stay on the ground rather
        // than launch off every edge.
        let was_walking = ground.is_some() && !jumped;
        if was_walking && mover::ground(physics, center, s.half_size).is_none() {
            center =
                mover::snap_down(physics, center, s.half_size, s.step_height).unwrap_or(center);
        }

        self.on_ground = mover::ground(physics, center, s.half_size).is_some();
        self.velocity = moved.velocity;
        self.current.center = center;
        self.current.bob = self.bob(start, center);
    }

    /// Feet position (latest tick, not interpolated).
    pub fn feet(&self) -> Vec3 {
        self.current.center - Vec3::Y * self.settings.half_size.y
    }

    /// Camera position (latest tick), head bob included.
    pub fn eye(&self) -> Vec3 {
        self.eye_of(&self.current)
    }

    /// Unit view direction (latest tick).
    pub fn forward(&self) -> Vec3 {
        self.current.look.forward()
    }

    /// Heading, radians. 0 faces -Z; positive turns left.
    pub fn yaw(&self) -> f32 {
        self.current.look.yaw
    }

    /// Tilt, radians. Positive looks up.
    pub fn pitch(&self) -> f32 {
        self.current.look.pitch
    }

    /// Current velocity, m/s.
    pub fn velocity(&self) -> Vec3 {
        self.velocity
    }

    /// True when standing on walkable ground.
    pub fn on_ground(&self) -> bool {
        self.on_ground
    }

    /// Sprint left, 0 (exhausted) to 1 (full).
    pub fn stamina(&self) -> f32 {
        self.stamina
    }

    /// Collision box (latest tick): pass to `World::tick` for triggers.
    pub fn bounds(&self) -> Aabb {
        Aabb::from_center(self.current.center, self.settings.half_size)
    }

    /// Render camera between the last two ticks; pass `frame.alpha`.
    pub fn camera(&self, alpha: f32) -> Camera {
        let pose = self.prev.lerp(&self.current, alpha);
        pose.look.camera(self.eye_of(&pose), &self.settings.look)
    }

    fn eye_of(&self, pose: &Pose) -> Vec3 {
        let feet = pose.center.y - self.settings.half_size.y;
        Vec3::new(
            pose.center.x,
            feet + self.settings.eye_height + pose.bob,
            pose.center.z,
        )
    }

    /// Wish direction (level, unit or zero) and speed from the movement
    /// keys. Spends or refills stamina.
    fn wish(&mut self, input: &Input, grounded: bool, dt: f32) -> (Vec3, f32) {
        let (forward, right) = self.current.look.ground_axes();
        let wish = forward * input.axis(Key::W, Key::S) + right * input.axis(Key::D, Key::A);
        let direction = wish.normalize_or_zero();

        let mut speed = self.settings.speed;
        let Some(sprint) = self.settings.sprint else {
            return (direction, speed);
        };

        let moving = direction != Vec3::ZERO;
        let sprinting = moving && grounded && input.is_down(SPRINT_KEY) && !self.winded;
        if sprinting {
            speed *= sprint.multiplier;
            self.stamina -= dt / sprint.duration;
        } else {
            self.stamina += dt / sprint.recovery;
        }
        self.stamina = self.stamina.clamp(0.0, 1.0);

        if self.stamina == 0.0 {
            self.winded = true;
        }
        if self.stamina >= SPRINT_RESUME {
            self.winded = false;
        }

        (direction, speed)
    }

    /// Head bob offset after moving from `from` to `to`.
    fn bob(&mut self, from: Vec3, to: Vec3) -> f32 {
        let Some(bob) = self.settings.bob else {
            return 0.0;
        };
        if !self.on_ground {
            return 0.0;
        }

        self.walked += horizontal(to - from).length();
        let pace = (horizontal(self.velocity).length() / self.settings.speed).min(1.0);

        // |sin| dips once per footstep.
        -bob.height * pace * (PI * self.walked / bob.step_length).sin().abs()
    }
}

fn horizontal(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Quake ground friction on the horizontal velocity.
fn friction(velocity: Vec3, friction: f32, stop_speed: f32, dt: f32) -> Vec3 {
    let speed = horizontal(velocity).length();
    if speed < MIN_SPEED {
        return Vec3::new(0.0, velocity.y, 0.0);
    }

    let control = speed.max(stop_speed);
    let scale = (speed - control * friction * dt).max(0.0) / speed;
    Vec3::new(velocity.x * scale, velocity.y, velocity.z * scale)
}

/// Quake acceleration: adds speed along `direction` up to `wish_speed`,
/// at most `accel × wish_speed × dt` per tick.
fn accelerate(velocity: Vec3, direction: Vec3, wish_speed: f32, accel: f32, dt: f32) -> Vec3 {
    let current = velocity.dot(direction);
    let add = wish_speed - current;
    if add <= 0.0 {
        return velocity;
    }

    velocity + direction * (accel * wish_speed * dt).min(add)
}

/// Quake air control: like `accelerate`, but the speed it may add along
/// the wish direction is capped. Turning sideways stays possible, so
/// strafing while turning gains speed.
fn air_accelerate(
    velocity: Vec3,
    direction: Vec3,
    wish_speed: f32,
    s: &WalkSettings,
    dt: f32,
) -> Vec3 {
    let current = velocity.dot(direction);
    let add = wish_speed.min(s.air_speed_cap) - current;
    if add <= 0.0 {
        return velocity;
    }

    velocity + direction * (s.air_accel * wish_speed * dt).min(add)
}

/// `velocity` with its push into the surface removed: walking along a
/// slope instead of into it.
fn along_plane(velocity: Vec3, normal: Vec3) -> Vec3 {
    velocity - normal * velocity.dot(normal)
}

#[cfg(test)]
mod tests {
    use platform::{Event, KeyState};

    use super::*;
    use crate::collision::Brush;
    use crate::entity::{Close, Entity, Mover};

    const DT: f32 = 1.0 / 60.0;
    const EPS: f32 = 0.02;

    fn holding(keys: &[Key]) -> Input {
        let mut input = Input::default();
        for &key in keys {
            input.apply(Event::Key {
                key,
                state: KeyState::Pressed,
            });
        }
        input
    }

    /// 200 m floor with its top at y = 0 (room for long sprints), plus
    /// `extra` brushes.
    fn world(extra: impl IntoIterator<Item = Brush>) -> World {
        let mut world = World::new();
        world.add_brush(Brush::cuboid(
            Vec3::new(-100.0, -1.0, -100.0),
            Vec3::new(100.0, 0.0, 100.0),
        ));
        for brush in extra {
            world.add_brush(brush);
        }
        world
    }

    fn walker(settings: WalkSettings) -> Walker {
        Walker::new(Vec3::ZERO, 0.0, 0.0, settings)
    }

    /// Runs `seconds` of ticks with `input` held. Edges (was_pressed)
    /// count on the first tick only, as in the engine.
    fn run(walker: &mut Walker, world: &World, input: &mut Input, seconds: f32) {
        for _ in 0..(seconds / DT).round() as u32 {
            walker.update(input, MouseLook::Disabled, world, DT);
            input.end_tick();
        }
    }

    fn speed(walker: &Walker) -> f32 {
        horizontal(walker.velocity()).length()
    }

    #[test]
    fn stands_on_the_floor_without_drifting() {
        let world = world([]);
        let mut w = walker(WalkSettings::arena());

        run(&mut w, &world, &mut Input::default(), 1.0);

        assert!(w.on_ground());
        assert!(w.feet().length() < EPS, "{:?}", w.feet());
        assert!((w.eye().y - EYE_HEIGHT).abs() < EPS);
    }

    #[test]
    fn falls_and_lands() {
        let world = world([]);
        let mut w = Walker::new(Vec3::Y * 3.0, 0.0, 0.0, WalkSettings::arena());

        run(&mut w, &world, &mut Input::default(), 0.1);
        assert!(!w.on_ground() && w.velocity().y < 0.0);

        run(&mut w, &world, &mut Input::default(), 1.0);
        assert!(w.on_ground() && w.feet().y.abs() < EPS);
    }

    #[test]
    fn walking_reaches_top_speed_and_no_more() {
        let world = world([]);
        let mut w = walker(WalkSettings::arena());

        run(&mut w, &world, &mut holding(&[Key::W]), 2.0);

        assert!((speed(&w) - w.settings.speed).abs() < 0.05, "{}", speed(&w));
        assert!(w.feet().z < -5.0, "walked forward (-Z): {:?}", w.feet());
    }

    #[test]
    fn friction_stops_the_walker() {
        let world = world([]);
        let mut w = walker(WalkSettings::survival());

        run(&mut w, &world, &mut holding(&[Key::W]), 1.0);
        run(&mut w, &world, &mut Input::default(), 1.0);

        assert!(speed(&w) < MIN_SPEED);
    }

    #[test]
    fn jump_height_matches_settings() {
        let world = world([]);
        let mut w = walker(WalkSettings::arena());
        run(&mut w, &world, &mut Input::default(), 0.1);

        let mut peak: f32 = 0.0;
        let mut input = holding(&[JUMP_KEY]);
        for _ in 0..60 {
            w.update(&input, MouseLook::Disabled, &world, DT);
            input.end_tick();
            peak = peak.max(w.feet().y);
        }

        let s = WalkSettings::arena();
        let expected = s.jump_speed * s.jump_speed / (2.0 * s.gravity);
        assert!(
            (peak - expected).abs() < 0.1,
            "peak {peak}, expected {expected}"
        );
        assert!(w.on_ground(), "landed again");
    }

    #[test]
    fn climbs_stairs_but_not_walls() {
        // A 30 cm step 1 m ahead, then a 1 m wall 3 m ahead.
        let step = Brush::cuboid(Vec3::new(-2.0, 0.0, -10.0), Vec3::new(2.0, 0.3, -1.0));
        let wall = Brush::cuboid(Vec3::new(-2.0, 0.3, -10.0), Vec3::new(2.0, 1.3, -3.0));
        let world = world([step, wall]);
        let mut w = walker(WalkSettings::survival());

        run(&mut w, &world, &mut holding(&[Key::W]), 3.0);

        assert!(
            (w.feet().y - 0.3).abs() < EPS,
            "on the step: {:?}",
            w.feet()
        );
        assert!(
            (w.feet().z - (-3.0 + HALF_SIZE.z)).abs() < EPS,
            "at the wall"
        );
    }

    #[test]
    fn stays_grounded_walking_down_a_step() {
        let step = Brush::cuboid(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 0.3, 2.0));
        let world = world([step]);
        let mut w = Walker::new(Vec3::Y * 0.3, 0.0, 0.0, WalkSettings::survival());
        run(&mut w, &world, &mut Input::default(), 0.1);

        let mut input = holding(&[Key::W]);
        for _ in 0..120 {
            w.update(&input, MouseLook::Disabled, &world, DT);
            input.end_tick();
            assert!(w.on_ground(), "launched off the edge at {:?}", w.feet());
        }
        assert!(w.feet().y.abs() < EPS && w.feet().z < -2.0);
    }

    #[test]
    fn only_arena_steers_in_the_air() {
        let world = world([]);
        let sideways = |settings| {
            let mut w = Walker::new(Vec3::Y * 5.0, 0.0, 0.0, settings);
            run(&mut w, &world, &mut holding(&[Key::D]), 0.5);
            w.velocity().x
        };

        assert!(sideways(WalkSettings::arena()) > 0.5);
        assert_eq!(sideways(WalkSettings::survival()), 0.0);
    }

    #[test]
    fn sprint_is_faster_until_stamina_runs_out() {
        let world = world([]);
        let mut w = walker(WalkSettings::survival());
        let sprint = w.settings.sprint.unwrap();

        run(&mut w, &world, &mut holding(&[Key::W, SPRINT_KEY]), 1.0);
        let top = w.settings.speed * sprint.multiplier;
        assert!((speed(&w) - top).abs() < 0.1, "{}", speed(&w));
        assert!(w.stamina() < 1.0);

        // Run dry, then keep holding Shift: walking pace while the bar
        // refills, not a sprint flicker.
        let mut sprinting = holding(&[Key::W, SPRINT_KEY]);
        let drain = w.stamina() * sprint.duration;
        run(&mut w, &world, &mut sprinting, drain + 0.5);
        assert!((speed(&w) - w.settings.speed).abs() < 0.1, "winded");
        assert!(w.stamina() > 0.0 && w.stamina() < SPRINT_RESUME);

        let refill = (SPRINT_RESUME - w.stamina()) * sprint.recovery;
        run(&mut w, &world, &mut sprinting, refill + 0.2);
        assert!(speed(&w) > w.settings.speed + 0.5, "sprints again");
    }

    #[test]
    fn arena_ignores_the_sprint_key() {
        let world = world([]);
        let mut w = walker(WalkSettings::arena());

        run(&mut w, &world, &mut holding(&[Key::W, SPRINT_KEY]), 2.0);
        assert!((speed(&w) - w.settings.speed).abs() < 0.05);
    }

    #[test]
    fn head_bobs_only_when_the_profile_has_it() {
        let world = world([]);
        let eye_range = |settings| {
            let mut w = walker(settings);
            let mut input = holding(&[Key::W]);
            let (mut low, mut high) = (f32::MAX, f32::MIN);
            for _ in 0..120 {
                w.update(&input, MouseLook::Disabled, &world, DT);
                input.end_tick();
                low = low.min(w.eye().y);
                high = high.max(w.eye().y);
            }
            high - low
        };

        let survival = WalkSettings::survival();
        let range = eye_range(survival);
        assert!(range > survival.bob.unwrap().height * 0.5, "{range}");
        assert!(eye_range(WalkSettings::arena()) < 1e-4);
    }

    #[test]
    fn camera_interpolates_between_ticks() {
        let world = world([]);
        let mut w = walker(WalkSettings::arena());
        run(&mut w, &world, &mut holding(&[Key::W]), 0.5);

        let eye = |alpha| w.camera(alpha).view.inverse().transform_point3(Vec3::ZERO);
        let (prev, now) = (eye(0.0), eye(1.0));

        assert!((now - w.eye()).length() < 1e-4);
        assert!((eye(0.5) - (prev + now) * 0.5).length() < 1e-4);
    }

    #[test]
    fn walks_through_a_door_once_it_opens() {
        let mut world = world([]);
        world.spawn(Entity {
            name: Some("door".into()),
            collider: Some(Brush::cuboid(
                Vec3::new(-1.0, 0.0, -3.1),
                Vec3::new(1.0, 2.2, -2.9),
            )),
            mover: Some(Mover {
                travel: Vec3::X * -2.5,
                speed: 5.0,
                close: Close::After(0.2),
            }),
            ..Default::default()
        });
        let mut w = walker(WalkSettings::survival());
        let mut input = holding(&[Key::W]);
        let mut step = |w: &mut Walker, world: &mut World, seconds: f32| {
            for _ in 0..(seconds / DT).round() as u32 {
                w.update(&input, MouseLook::Disabled, world, DT);
                world.tick(DT, Some(w.bounds()));
                input.end_tick();
            }
        };

        step(&mut w, &mut world, 2.0);
        assert!(w.feet().z > -2.9, "walked through a closed door");

        // Opens; it would close again while the walker is in the
        // doorway, but backs off instead.
        world.fire("door");
        step(&mut w, &mut world, 2.0);
        assert!(w.feet().z < -4.0, "still stuck at {:?}", w.feet());
    }

    #[test]
    fn air_acceleration_is_capped_along_the_wish_direction() {
        let s = WalkSettings::arena();
        let still = air_accelerate(Vec3::ZERO, Vec3::X, s.speed, &s, 1.0);
        assert!((still.x - s.air_speed_cap).abs() < 1e-5);

        // Moving fast along Z, wishing along X: X is still added.
        let fast = air_accelerate(Vec3::Z * 10.0, Vec3::X, s.speed, &s, 0.01);
        assert!(fast.x > 0.0 && fast.z == 10.0);
    }
}
