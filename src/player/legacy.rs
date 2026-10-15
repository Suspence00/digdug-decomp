//! Compatibility with CPU1 $1136 and $1174. Gameplay decisions live in player.rs;
//! this adapter retains the observable registers, flags and coordinate writes.
use super::{Direction, Position};
use crate::z80::{Z80, Z80Bus};

const FACING: u16 = 0x85AE;
const PLAYER_X: u16 = 0x98A2;
const PLAYER_Y: u16 = 0x98A3;
const OUTSIDE_PLAYFIELD: u8 = 0x02;

pub(crate) fn enforce_grid_alignment<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let saved_hl = cpu.hl();
    let saved_bc = cpu.bc();
    let requested = bus.read_byte(1, FACING);
    let previous = bus.read_byte(1, 0x85AF);
    let attract = bus.read_byte(1, 0x8657);
    let position = Position { x: bus.read_byte(1, PLAYER_X), y: bus.read_byte(1, PLAYER_Y) };
    let decision = position.turn(requested, previous, attract & 2 != 0);
    cpu.a = requested;
    cpu.and8(0x0F);
    let vertical = if cpu.a == 0 { true } else { cpu.sub8(4, false, false); cpu.a == 4 };
    if vertical {
        cpu.a = position.x;
        cpu.and8(0x0F);
        cpu.sub8(1, false, false);
    } else {
        cpu.sub8(2, false, false);
        let horizontal = if cpu.a == 2 { true } else { cpu.sub8(6, false, false); cpu.a == 6 };
        if horizontal {
            cpu.a = position.y;
            cpu.sub8(0x20, false, false);
            if cpu.a >= 0x20 { cpu.and8(0x0F); } else { cpu.sub8(8, false, false); }
        }
    }
    if !decision.accepted {
        cpu.a = previous;
        cpu.and8(0x0F);
        bus.write_byte(1, FACING, decision.facing_latch);
        cpu.a &= !1;
    } else {
        cpu.a = attract;
        cpu.cb_op(0x4F, cpu.a);
        if attract & 2 == 0 {
            cpu.a = requested;
            cpu.and8(0x0F);
            let direction = cpu.a;
            cpu.a = previous;
            cpu.and8(0x0F);
            cpu.sub8(direction, false, false);
            if decision.reset_movement_clock { bus.write_byte(1, 0x85B3, 2); }
        }
        cpu.a |= 1;
    }
    cpu.set_hl(saved_hl);
    cpu.set_bc(saved_bc);
}

pub(crate) fn check_playfield_bounds(cpu: &mut Z80) {
    let position = Position { x: cpu.d, y: cpu.e };
    cpu.c = (cpu.c & !OUTSIDE_PLAYFIELD)
        | if position.within_playfield() { 0 } else { OUTSIDE_PLAYFIELD };
}

pub(crate) fn step_two_pixels<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let latch = bus.read_byte(1, FACING);
    // Keep the dispatch comparisons: unsupported directions also have observable
    // A/F/HL results, even though they leave the player's position unchanged.
    cpu.a = latch;
    cpu.set_hl(PLAYER_Y);
    cpu.and8(0x0F);
    if cpu.a != 0 {
        cpu.sub8(4, false, false);
        if cpu.a != 4 {
            cpu.set_hl(PLAYER_X);
            cpu.sub8(2, false, false);
            if cpu.a != 2 {
                cpu.sub8(6, false, false);
            }
        }
    }

    let Some(direction) = Direction::from_latch(latch) else { return };
    let start = Position { x: bus.read_byte(1, PLAYER_X), y: bus.read_byte(1, PLAYER_Y) };
    let attempt = start.step_two_pixels(direction);
    let horizontal = matches!(direction, Direction::Right | Direction::Left);
    let increasing = matches!(direction, Direction::Right | Direction::Down);
    let candidate_coordinate = if horizontal { attempt.candidate.x } else { attempt.candidate.y };
    write_two_pixel_delta(cpu, bus, cpu.hl(), candidate_coordinate, increasing);

    // The original keeps the candidate in DE/A even if the RAM coordinate rolls back.
    cpu.d = attempt.candidate.x;
    cpu.e = attempt.candidate.y;
    cpu.a = cpu.d;
    cpu.c = (cpu.c & !OUTSIDE_PLAYFIELD) | if attempt.blocked { OUTSIDE_PLAYFIELD } else { 0 };
    cpu.cb_op(0x49, cpu.c); // BIT 1, C
    if attempt.blocked {
        let restored_coordinate = if horizontal { attempt.position.x } else { attempt.position.y };
        write_two_pixel_delta(cpu, bus, cpu.hl(), restored_coordinate, !increasing);
    }
}

fn write_two_pixel_delta<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, address: u16, result: u8, increasing: bool) {
    let previous = bus.read_byte(1, address);
    let middle = if increasing { cpu.inc8(previous) } else { cpu.dec8(previous) };
    bus.write_byte(1, address, middle);
    let calculated = if increasing { cpu.inc8(middle) } else { cpu.dec8(middle) };
    debug_assert_eq!(calculated, result);
    // The pure movement rule determines the position; ALU operations above preserve
    // the original flags. Both one-pixel writes remain visible in their original order.
    bus.write_byte(1, address, result);
}


pub(crate) fn check_rock_collision<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.set_hl(0x98A2);
    cpu.set_de(0x85E1);
    cpu.and8(0x0F);
    let mut valid_dir = true;
    if cpu.a == 0x00 {
        let x = bus.read_byte(1, 0x98A2);
        bus.write_byte(1, 0x85E1, x);
        cpu.a = bus.read_byte(1, 0x98A3);
        cpu.sub8(0x10, false, true);
        bus.write_byte(1, 0x85E0, cpu.a);
    } else {
        cpu.sub8(0x04, false, false);
        if cpu.a == 0x04 {
            let x = bus.read_byte(1, 0x98A2);
            bus.write_byte(1, 0x85E1, x);
            cpu.a = bus.read_byte(1, 0x98A3);
            cpu.add8(0x10, false);
            bus.write_byte(1, 0x85E0, cpu.a);
        } else {
            cpu.sub8(0x02, false, false);
            if cpu.a == 0x02 {
                cpu.a = bus.read_byte(1, 0x98A2);
                cpu.add8(0x10, false);
                bus.write_byte(1, 0x85E1, cpu.a);
                let y = bus.read_byte(1, 0x98A3);
                cpu.a = y;
                bus.write_byte(1, 0x85E0, y);
            } else {
                cpu.sub8(0x06, false, false);
                if cpu.a == 0x06 {
                    cpu.a = bus.read_byte(1, 0x98A2);
                    cpu.sub8(0x10, false, true);
                    bus.write_byte(1, 0x85E1, cpu.a);
                    let y = bus.read_byte(1, 0x98A3);
                    cpu.a = y;
                    bus.write_byte(1, 0x85E0, y);
                } else {
                    valid_dir = false;
                }
            }
        }
    }
    if valid_dir {
        let mut rock_hl = 0x8465u16;
        cpu.b = 0x08;
        cpu.set_de(0x0008);
        let position = Position { x: bus.read_byte(1, PLAYER_X), y: bus.read_byte(1, PLAYER_Y) };
        let target = position.rock_probe(bus.read_byte(1, FACING)).expect("valid facing");
        let target_x = target.x;
        let target_y = target.y;
        loop {
            cpu.a = target_x;
            let rx = bus.read_byte(1, rock_hl);
            cpu.sub8(rx, false, false);
            if target_x == rx {
                cpu.a = target_y;
                let ry = bus.read_byte(1, rock_hl.wrapping_add(1));
                cpu.sub8(ry, false, false);
                if target_y == ry {
                    cpu.set_hl(0x8401);
                    let s = bus.read_byte(1, 0x8401) | 0x04;
                    bus.write_byte(1, 0x8401, s);
                    break;
                }
            }
            let s = bus.read_byte(1, 0x8401) & !0x04;
            bus.write_byte(1, 0x8401, s);
            rock_hl = cpu.add16(rock_hl, 0x0008);
            cpu.set_hl(rock_hl);
            cpu.b = cpu.b.wrapping_sub(1);
            if cpu.b == 0 {
                break;
            }
        }
    }
}
