//! Original-instruction comparisons for the movement compatibility boundary.
use digdug_re::decomp::{step_human_cpu1, DecompStats};
use digdug_re::hardware::DigDugBus;
use digdug_re::rom::DigDugRoms;
use digdug_re::verify::{VerificationOutcome, VerifyLog};
use digdug_re::z80::{Z80, Z80Bus};
use digdug_re::z80_ref::{ref_run_until, RefRun};
use std::path::Path;

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn rock_slots_and_every_dirt_contact_pattern_match_original_instructions() {
    use digdug_re::player::Position;
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted")).unwrap();
    let mut log = VerifyLog::new(500);
    for direction in [0, 2, 4, 6] {
        for hit_slot in 0..=8 {
            let mut bus = DigDugBus::new(roms.clone());
            let position = Position { x: 0x71, y: 0x80 };
            bus.write_byte(1, 0x98A2, position.x);
            bus.write_byte(1, 0x98A3, position.y);
            bus.write_byte(1, 0x85AE, direction);
            if hit_slot < 8 {
                let target = position.rock_probe(direction).unwrap();
                bus.write_byte(1, 0x8465 + hit_slot * 8, target.x);
                bus.write_byte(1, 0x8466 + hit_slot * 8, target.y);
            }
            check_boundary(&roms, &mut log, bus, 0x11EA);
        }
    }
    for mask in 0..16 {
        let mut bus = DigDugBus::new(roms.clone());
        for tile in 0..4u16 {
            bus.write_word(1, 0x85D6 + 2 * tile, 0x9000 + tile);
            bus.write_byte(1, 0x9000 + tile, if mask & (1 << tile) != 0 { 0x7F } else { 0x0C });
        }
        check_boundary(&roms, &mut log, bus, 0x1353);
    }
    assert_eq!(log.checked, 52);
}

fn check_boundary(roms: &DigDugRoms, log: &mut VerifyLog, mut bus: DigDugBus, pc: u16) {
    let mut cpu = Z80::new(1);
    cpu.pc = pc;
    cpu.sp = 0x9A80;
    cpu.f = 1;
    bus.write_word(1, cpu.sp, 0x4444);
    let pre = cpu.clone();
    let pre_bus = bus.clone();
    step_human_cpu1(&mut cpu, &mut bus, roms, &mut DecompStats::new());
    log.check(0, "rock and dirt contact", &pre, &pre_bus, &cpu, &bus);
    assert_eq!(log.outcome(true), VerificationOutcome::Passed, "cpu1 ${pc:04X}");
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn movement_and_bounds_match_original_instructions_at_edges_and_with_direction_flags() {
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted")).unwrap();
    let base = DigDugBus::new(roms.clone());
    let coordinates = [0, 1, 7, 8, 9, 0x10, 0x11, 0x12, 0xDF, 0xE0, 0xE1, 0xE2, 0xEF, 0xF0, 0xF1, 0xFF];
    let mut log = VerifyLog::new(500);
    for pc in [0x1136, 0x1174, 0x118F, 0x11EA, 0x1353, 0x13CB, 0x1393, 0x15D3] {
        for x in coordinates {
            for y in coordinates {
                for direction in 0..=0x0F {
                    for flags in [0, 0x40, 0x80, 0xF0] {
                        let mut bus = base.clone();
                        bus.write_byte(1, 0x98A2, x);
                        bus.write_byte(1, 0x98A3, y);
                        bus.write_byte(1, 0x85AE, direction | flags);
                        let mut cpu = Z80::new(1);
                        cpu.pc = pc;
                        cpu.sp = 0x9A80;
                        let seed = usize::from(flags >> 6);
                        bus.write_byte(1, 0x85AF, [0, 2, 4, 6][seed] | flags);
                        bus.write_byte(1, 0x8657, if seed & 1 != 0 { 2 } else { 0 });
                        for tile in 0..4u16 {
                            bus.write_word(1, 0x85D6 + tile * 2, 0x9000 + tile);
                            bus.write_byte(1, 0x9000 + tile, if seed & (1 << tile) != 0 { 0x7F } else { 0 });
                        }
                        cpu.set_af(0x5A00 | u16::from([0u8, 1, 0xD7, 0xFF][seed]));
                        cpu.set_bc(0x7300 | u16::from([0u8, 2, 0x7D, 0xFF][seed]));
                        cpu.set_de((u16::from(x) << 8) | u16::from(y));
                        cpu.set_hl(0x1234);
                        cpu.ix = 0x8182;
                        cpu.iy = 0x8384;
                        bus.write_word(1, cpu.sp, 0x4444);
                        let pre = cpu.clone();
                        let pre_bus = bus.clone();
                        step_human_cpu1(&mut cpu, &mut bus, &roms, &mut DecompStats::new());
                        log.check(0, "player movement boundary", &pre, &pre_bus, &cpu, &bus);
                        assert_eq!(log.outcome(true), VerificationOutcome::Passed,
                            "cpu1 ${pc:04X}: x={x:02X}, y={y:02X}, direction={:02X}; differences={:?}, timeouts={:?}",
                            direction | flags,
                            log.mismatches.values().flat_map(|m| m.reg_example.iter().chain(&m.state_example)).collect::<Vec<_>>(),
                            log.timeouts);
                    }
                }
            }
        }
    }
    assert_eq!(log.checked, 131_072);
}

struct CoordinateWrites {
    bus: DigDugBus,
    writes: Vec<(u16, u8)>,
}

impl Z80Bus for CoordinateWrites {
    fn read_byte(&mut self, cpu: usize, address: u16) -> u8 {
        self.bus.read_byte(cpu, address)
    }

    fn write_byte(&mut self, cpu: usize, address: u16, value: u8) {
        if (0x98A2..=0x98A3).contains(&address) {
            self.writes.push((address, value));
        }
        self.bus.write_byte(cpu, address, value);
    }
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn coordinate_writes_and_rollbacks_follow_original_instruction_order() {
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted")).unwrap();
    for (x, y) in [(0x71, 0x80), (0x11, 8), (0xE1, 0xF0), (0x12, 9), (0xE0, 0xEF), (0, 0xFF)] {
        for latch in 0..=0xFF {
            let mut initial = DigDugBus::new(roms.clone());
            initial.write_byte(1, 0x98A2, x);
            initial.write_byte(1, 0x98A3, y);
            initial.write_byte(1, 0x85AE, latch);
            initial.write_word(1, 0x9A80, 0x4444);
            let mut native_bus = CoordinateWrites { bus: initial.clone(), writes: Vec::new() };
            let mut reference_bus = CoordinateWrites { bus: initial, writes: Vec::new() };
            let mut native = Z80::new(1);
            native.pc = 0x1136;
            native.sp = 0x9A80;
            let mut reference = native.clone();
            step_human_cpu1(&mut native, &mut native_bus, &roms, &mut DecompStats::new());
            assert!(matches!(ref_run_until(&mut reference, &mut reference_bus, native.pc, native.sp, 500), RefRun::Reached(_)));
            assert_eq!(native_bus.writes, reference_bus.writes,
                "x={x:02X}, y={y:02X}, latch={latch:02X}");
        }
    }
}
