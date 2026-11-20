//! The touch model: one left thumb stick, a right look area, a tap to jump.
//!
//! The desktop reads a mouse for the camera and a keyboard for movement. iOS has neither, so this
//! module turns multi-touch into the same two signals the engine's camera and movement code
//! already consume — a per-frame look delta and a movement vector — while keeping the raw points
//! for the on-screen controls. It is pure state with no OS calls, so it is tested on every
//! platform and the same code runs on the device.
use std::time::{Duration, Instant};

/// Positions and vectors are normalized: x and y run 0..=1 over the window, y down.
pub type Vec2 = [f32; 2];

/// Below this fraction of the stick radius no movement is sent: the thumb rests off-centre.
pub const DEAD_ZONE: f32 = 0.15;
/// The stick's travel, as a fraction of the window's shorter side.
pub const STICK_RADIUS: f32 = 0.12;
/// A touch that ends within this long and this close is a tap, not a drag.
pub const TAP: Duration = Duration::from_millis(180);
/// How far a finger may drift and still count as a tap.
pub const TAP_SLOP: f32 = 0.02;
/// Where the left thumb region ends: a touch starting left of this is the stick.
pub const STICK_REGION: f32 = 0.42;

/// What a finger is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Left region: drives the movement vector.
    Stick,
    /// Right region: drags the camera.
    Look,
}

/// One finger on the glass.
#[derive(Clone, Copy, Debug)]
pub struct Point {
    pub id: u64,
    pub pos: Vec2,
    pub start: Vec2,
    pub began: Instant,
    pub role: Role,
}

/// The touches of one frame, and what they mean.
#[derive(Clone, Debug, Default)]
pub struct Touches {
    points: Vec<Point>,
    stick_origin: Option<Vec2>,
    stick: Vec2,
    look: Vec2,
    jump: bool,
}

impl Touches {
    /// A finger landed. The first one on the left takes the stick; anything else looks.
    pub fn begin(&mut self, id: u64, pos: Vec2, now: Instant) {
        let role = if pos[0] < STICK_REGION && self.stick_origin.is_none() {
            self.stick_origin = Some(pos);
            Role::Stick
        } else {
            Role::Look
        };
        self.points.push(Point {
            id,
            pos,
            start: pos,
            began: now,
            role,
        });
        if role == Role::Stick {
            self.stick = [0.0, 0.0];
        }
    }

    /// A finger moved: the stick re-reads its vector, a look finger adds its travel.
    pub fn moved(&mut self, id: u64, pos: Vec2) {
        let Some(point) = self.points.iter_mut().find(|point| point.id == id) else {
            return;
        };
        let previous = point.pos;
        point.pos = pos;
        match point.role {
            Role::Stick => self.stick = stick_vector(self.stick_origin.unwrap_or(pos), pos),
            Role::Look => {
                self.look[0] += pos[0] - previous[0];
                self.look[1] += pos[1] - previous[1];
            }
        }
    }

    /// A finger lifted. A short, still touch on the stick is a jump.
    pub fn end(&mut self, id: u64, pos: Vec2, now: Instant) {
        let Some(index) = self.points.iter().position(|point| point.id == id) else {
            return;
        };
        let point = self.points.remove(index);
        if point.role == Role::Stick {
            self.stick_origin = None;
            self.stick = [0.0, 0.0];
            let held = now.saturating_duration_since(point.began);
            let drift = distance(point.start, pos);
            if held <= TAP && drift <= TAP_SLOP {
                self.jump = true;
            }
        }
    }

    /// The system cancelled the touches (a call, a gesture, leaving the app).
    pub fn cancel(&mut self) {
        self.points.clear();
        self.stick_origin = None;
        self.stick = [0.0, 0.0];
    }

    /// The movement vector the engine reads this frame, `[-1, 1]` on both axes: x is strafe, y is
    /// forward (negative is forward, as the touch origin sits above the thumb's travel).
    pub fn stick(&self) -> Vec2 {
        self.stick
    }

    /// Take the look delta accumulated since the last call: the camera adds it once per frame.
    pub fn take_look(&mut self) -> Vec2 {
        std::mem::take(&mut self.look)
    }

    /// Take the jump request, if a tap asked for one.
    pub fn take_jump(&mut self) -> bool {
        std::mem::replace(&mut self.jump, false)
    }

    /// The fingers to draw, with the stick knob's position for the on-screen ring.
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// Where the stick ring sits while a thumb holds it.
    pub fn stick_origin(&self) -> Option<Vec2> {
        self.stick_origin
    }
}

/// Clamp a thumb's travel to a circle and apply the dead zone: the value the game uses.
fn stick_vector(origin: Vec2, pos: Vec2) -> Vec2 {
    let mut delta = [pos[0] - origin[0], pos[1] - origin[1]];
    let length = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if length < 1e-6 {
        return [0.0, 0.0];
    }
    let normalized = (length / STICK_RADIUS).min(1.0);
    if normalized <= DEAD_ZONE {
        return [0.0, 0.0];
    }
    // Re-scale so the dead zone does not eat the first part of the travel.
    let scaled = (normalized - DEAD_ZONE) / (1.0 - DEAD_ZONE);
    delta[0] = delta[0] / length * scaled;
    delta[1] = delta[1] / length * scaled;
    delta
}

fn distance(a: Vec2, b: Vec2) -> f32 {
    let dx = a[0] - b[0];
    let dy = a[1] - b[1];
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32) -> Vec2 {
        [x, y]
    }

    #[test]
    fn the_left_thumb_takes_the_stick_and_the_right_one_looks() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.2, 0.8), now);
        touches.begin(2, at(0.8, 0.5), now);
        assert_eq!(touches.points()[0].role, Role::Stick);
        assert_eq!(touches.points()[1].role, Role::Look);
    }

    #[test]
    fn a_second_finger_on_the_left_looks_instead() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.1, 0.8), now);
        touches.begin(2, at(0.3, 0.6), now);
        assert_eq!(touches.points()[1].role, Role::Look);
    }

    #[test]
    fn the_stick_clamps_to_one_and_ignores_the_dead_zone() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.2, 0.8), now);
        touches.moved(1, at(0.2, 0.8 - STICK_RADIUS * 4.0));
        let stick = touches.stick();
        assert!(
            (stick[1] + 1.0).abs() < 1e-4,
            "full up is -1, got {stick:?}"
        );

        touches.moved(1, at(0.2, 0.8 - STICK_RADIUS * 0.1));
        assert_eq!(
            touches.stick(),
            [0.0, 0.0],
            "inside the dead zone nothing moves"
        );
    }

    #[test]
    fn the_stick_is_continuous_across_the_dead_zone_edge() {
        let just_past = STICK_RADIUS * (DEAD_ZONE + 1e-3);
        let value = stick_vector(at(0.2, 0.8), at(0.2, 0.8 - just_past));
        assert!(
            value[1] < 0.0 && value[1].abs() < 0.01,
            "starts from zero: {value:?}"
        );
    }

    #[test]
    fn a_look_drag_accumulates_and_is_taken_once() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(2, at(0.7, 0.5), now);
        touches.moved(2, at(0.75, 0.4));
        touches.moved(2, at(0.8, 0.35));
        let look = touches.take_look();
        assert!((look[0] - 0.1).abs() < 1e-5);
        assert!((look[1] + 0.15).abs() < 1e-5);
        assert_eq!(
            touches.take_look(),
            [0.0, 0.0],
            "a delta is never read twice"
        );
    }

    #[test]
    fn a_tap_on_the_stick_jumps_and_a_drag_does_not() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.2, 0.8), now);
        touches.end(1, at(0.2, 0.8), now);
        assert!(touches.take_jump());
        assert!(!touches.take_jump(), "one tap, one jump");

        touches.begin(1, at(0.2, 0.8), now);
        touches.moved(1, at(0.2, 0.6));
        touches.end(1, at(0.2, 0.6), now);
        assert!(!touches.take_jump(), "a drag is movement, not a jump");
    }

    #[test]
    fn a_long_press_is_not_a_jump() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.2, 0.8), now);
        touches.end(1, at(0.2, 0.8), now + TAP + Duration::from_millis(1));
        assert!(!touches.take_jump());
    }

    #[test]
    fn cancelling_lets_everything_go() {
        let mut touches = Touches::default();
        let now = Instant::now();
        touches.begin(1, at(0.2, 0.8), now);
        touches.begin(2, at(0.8, 0.5), now);
        touches.moved(1, at(0.2, 0.7));
        touches.cancel();
        assert!(touches.points().is_empty());
        assert_eq!(touches.stick(), [0.0, 0.0]);
        assert_eq!(touches.stick_origin(), None);
    }

    #[test]
    fn moving_an_unknown_finger_changes_nothing() {
        let mut touches = Touches::default();
        touches.moved(9, at(0.5, 0.5));
        assert_eq!(touches.take_look(), [0.0, 0.0]);
        assert!(touches.points().is_empty());
    }
}
