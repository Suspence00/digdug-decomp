//! Player movement rules in arcade coordinates, independent of CPU registers and RAM.
//!
//! This is a small extraction from the existing translation, not an independent
//! reimplementation. Speed, animation and tile painting retain legacy adapters.

pub(crate) mod legacy;
pub mod tunnels;

/// The game's decoded facing direction. These codes differ from controller button bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    /// Ignores the latch's high-bit flags. Neutral (8) and unknown codes do not move.
    pub fn from_latch(latch: u8) -> Option<Self> {
        match latch & 0x0F {
            0 => Some(Self::Up),
            2 => Some(Self::Right),
            4 => Some(Self::Down),
            6 => Some(Self::Left),
            _ => None,
        }
    }
}

/// Native arcade coordinates, before the renderer's screen transforms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub x: u8,
    pub y: u8,
}

/// A step's candidate is retained even when rejected: the processor boundary
/// exposes it in DE while restoring the previous position in shared RAM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementAttempt {
    pub candidate: Position,
    pub position: Position,
    pub blocked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TurnDecision {
    pub accepted: bool,
    pub facing_latch: u8,
    pub reset_movement_clock: bool,
}

impl Position {
    /// Vertical turns require a column center; horizontal turns require a row
    /// center, with the surface tunnel at y=8 treated separately.
    pub fn turn(self, requested: u8, previous: u8, attract_demo: bool) -> TurnDecision {
        let accepted = match Direction::from_latch(requested) {
            Some(Direction::Up | Direction::Down) => self.x & 0x0F == 1,
            Some(Direction::Left | Direction::Right) => if self.y < 0x20 { self.y == 8 } else { self.y & 0x0F == 0 },
            None => true,
        };
        TurnDecision {
            accepted,
            facing_latch: if accepted { requested } else { previous & 0x0F },
            reset_movement_clock: accepted && !attract_demo && requested & 0x0F != previous & 0x0F,
        }
    }

    /// Tile-sized probe used by the original stationary-rock collision check.
    pub fn rock_probe(self, latch: u8) -> Option<Self> {
        let mut probe = self;
        match Direction::from_latch(latch)? {
            Direction::Up => probe.y = probe.y.wrapping_sub(16),
            Direction::Right => probe.x = probe.x.wrapping_add(16),
            Direction::Down => probe.y = probe.y.wrapping_add(16),
            Direction::Left => probe.x = probe.x.wrapping_sub(16),
        }
        Some(probe)
    }
    pub fn within_playfield(self) -> bool {
        (0x11..=0xE1).contains(&self.x) && (0x08..=0xF0).contains(&self.y)
    }

    /// Attempts a complete two-pixel step. An invalid candidate leaves the original
    /// position intact rather than clamping to an edge. Byte arithmetic wraps first,
    /// matching the original counters, including positions supplied by diagnostics.
    pub fn step_two_pixels(self, direction: Direction) -> MovementAttempt {
        let mut candidate = self;
        match direction {
            Direction::Up => candidate.y = self.y.wrapping_sub(2),
            Direction::Right => candidate.x = self.x.wrapping_add(2),
            Direction::Down => candidate.y = self.y.wrapping_add(2),
            Direction::Left => candidate.x = self.x.wrapping_sub(2),
        }
        let blocked = !candidate.within_playfield();
        MovementAttempt { candidate, position: if blocked { self } else { candidate }, blocked }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_wait_for_grid_centers_and_preserve_previous_direction_when_rejected() {
        let off_grid = Position { x: 0x72, y: 0x81 };
        assert_eq!(off_grid.turn(0x80, 0x42, false), TurnDecision {
            accepted: false, facing_latch: 2, reset_movement_clock: false,
        });
        assert!(Position { x: 0x71, y: 0x81 }.turn(0, 2, false).accepted);
        assert!(Position { x: 0x72, y: 8 }.turn(2, 0, false).accepted);
        assert!(!Position { x: 0x72, y: 0x18 }.turn(2, 0, false).accepted);
        assert!(Position { x: 0x72, y: 0x20 }.turn(2, 0, false).accepted);
    }

    #[test]
    fn turn_delay_resets_only_for_a_changed_direction_outside_attract_mode() {
        let position = Position { x: 0x71, y: 0x80 };
        assert!(position.turn(0x82, 0, false).reset_movement_clock);
        assert!(!position.turn(0x82, 2, false).reset_movement_clock);
        assert!(!position.turn(0x82, 0, true).reset_movement_clock);
        assert_eq!(position.turn(0x82, 0, false).facing_latch, 0x82);
        assert!(position.turn(8, 0, false).accepted);
    }

    #[test]
    fn rock_probe_checks_a_tile_ahead_and_rejects_neutral() {
        let position = Position { x: 0x71, y: 8 };
        assert_eq!(position.rock_probe(0), Some(Position { x: 0x71, y: 0xF8 }));
        assert_eq!(position.rock_probe(0x82), Some(Position { x: 0x81, y: 8 }));
        assert_eq!(position.rock_probe(8), None);
    }

    #[test]
    fn directions_move_only_the_requested_axis() {
        let start = Position { x: 0x71, y: 0x80 };
        for (direction, x, y) in [
            (Direction::Up, 0x71, 0x7E), (Direction::Right, 0x73, 0x80),
            (Direction::Down, 0x71, 0x82), (Direction::Left, 0x6F, 0x80),
        ] {
            let step = start.step_two_pixels(direction);
            assert!(!step.blocked);
            assert_eq!(step.position, Position { x, y });
        }
    }

    #[test]
    fn edges_reject_the_whole_step_including_a_one_pixel_overshoot() {
        for (position, direction) in [
            (Position { x: 0x11, y: 0x80 }, Direction::Left),
            (Position { x: 0x12, y: 0x80 }, Direction::Left),
            (Position { x: 0xE0, y: 0x80 }, Direction::Right),
            (Position { x: 0xE1, y: 0x80 }, Direction::Right),
            (Position { x: 0x71, y: 8 }, Direction::Up),
            (Position { x: 0x71, y: 9 }, Direction::Up),
            (Position { x: 0x71, y: 0xEF }, Direction::Down),
            (Position { x: 0x71, y: 0xF0 }, Direction::Down),
        ] {
            let step = position.step_two_pixels(direction);
            assert!(step.blocked);
            assert_eq!(step.position, position);
        }
    }

    #[test]
    fn candidate_bounds_allow_reentry_but_reject_wrapped_coordinates() {
        let reentry = Position { x: 0x0F, y: 8 }.step_two_pixels(Direction::Right);
        assert_eq!(reentry.position, Position { x: 0x11, y: 8 });
        assert!(!reentry.blocked);
        for (x, direction, candidate_x) in [(0xFF, Direction::Right, 1), (0, Direction::Left, 0xFE)] {
            let start = Position { x, y: 0x80 };
            let step = start.step_two_pixels(direction);
            assert!(step.blocked);
            assert_eq!(step.candidate.x, candidate_x);
            assert_eq!(step.position, start);
        }
    }

    #[test]
    fn a_move_is_rejected_when_the_other_axis_is_outside_the_playfield() {
        let start = Position { x: 0x71, y: 0 };
        assert_eq!(start.step_two_pixels(Direction::Right).position, start);
        assert!(start.step_two_pixels(Direction::Right).blocked);
    }

    #[test]
    fn direction_latch_flags_are_independent_of_facing_and_neutral() {
        for flags in [0, 0x40, 0x80, 0xF0] {
            assert_eq!(Direction::from_latch(flags), Some(Direction::Up));
            assert_eq!(Direction::from_latch(flags | 2), Some(Direction::Right));
            assert_eq!(Direction::from_latch(flags | 4), Some(Direction::Down));
            assert_eq!(Direction::from_latch(flags | 6), Some(Direction::Left));
            for code in [1, 3, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15] {
                assert_eq!(Direction::from_latch(flags | code), None);
            }
        }
    }
}
