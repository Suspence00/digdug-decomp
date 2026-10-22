//! Original-ROM comparisons for pump state transitions and their CPU endpoints.
//! The seed is an actual running game; each case explicitly selects a routine
//! boundary and patches only the inputs needed to exercise that boundary.
use digdug_re::decomp::{step_human_cpu0, step_human_cpu1, DecompStats};
use digdug_re::hardware::*;
use digdug_re::rom::DigDugRoms;
use digdug_re::verify::{VerificationOutcome, VerifyLog};
use digdug_re::z80::{Z80, Z80Bus};
use std::{path::Path, sync::OnceLock};

fn active_game() -> DigDugBus {
    static SEED: OnceLock<DigDugBus> = OnceLock::new();
    SEED.get_or_init(|| {
        let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
            .expect("install your own Dig Dug Rev 2 ROMs in roms/extracted");
        let mut machine = DigDugMachine::new_unbooted(roms);
        for frame in 1..=1379 {
            machine.bus.buttons = match frame {
                760..=765 => INPUT_COIN,
                826..=831 => INPUT_START1,
                1335..=1338 => INPUT_RIGHT,
                _ => 0,
            };
            machine.run_frame();
        }
        let snapshot = machine.snapshot();
        assert!(snapshot.in_game && !snapshot.attract_demo);
        assert_eq!(snapshot.active_enemies, 4);
        machine.bus
    }).clone()
}

fn put(bus: &mut DigDugBus, address: u16, value: u8) {
    bus.write_byte(0, address, value);
}

fn get(bus: &DigDugBus, address: u16) -> u8 {
    bus.shared_ram[usize::from(address - 0x8000)]
}

fn check_boundary(mut bus: DigDugBus, cpu_id: usize, pc: u16, label: &str) -> DigDugBus {
    let roms = bus.roms.clone();
    let mut cpu = Z80::new(cpu_id);
    cpu.pc = pc;
    // A return address belongs below the real stack top. Using $9A80 for
    // CPU0 would overwrite $9A81 and silently inhibit the pop score award.
    cpu.sp = if cpu_id == 0 { 0x99FE } else { 0x9A7E };
    cpu.set_af(0x5AD7);
    cpu.set_bc(0x7302);
    cpu.set_de(0x7180);
    cpu.set_hl(0x1234);
    cpu.ix = 0x8182;
    cpu.iy = 0x8384;
    bus.write_word(cpu_id, cpu.sp, 0x4444);
    let pre_cpu = cpu.clone();
    let pre_bus = bus.clone();
    let mut stats = DecompStats::new();
    if cpu_id == 0 {
        step_human_cpu0(&mut cpu, &mut bus, &roms, &mut stats);
    } else {
        step_human_cpu1(&mut cpu, &mut bus, &roms, &mut stats);
    }
    assert_eq!((cpu.pc, cpu.sp),
        (0x4444, if cpu_id == 0 { 0x9A00 } else { 0x9A80 }), "{label}");
    assert!(stats.total_rust_calls > 0, "no translated boundary: {label}");
    let mut log = VerifyLog::new(100_000);
    log.check(0, "pump boundary", &pre_cpu, &pre_bus, &cpu, &bus);
    let differences: Vec<_> = log.mismatches.values()
        .flat_map(|m| m.reg_example.iter().chain(&m.state_example)).collect();
    assert_eq!(log.outcome(true), VerificationOutcome::Passed,
        "cpu{cpu_id} ${pc:04X} {label}: {differences:?}; timeouts={:?}; halts={:?}; unsupported={:?}",
        log.timeouts, log.reference_halts, log.unsupported);
    assert_eq!(log.checked, 1, "{label}");
    bus
}

fn clear_scores(bus: &mut DigDugBus) {
    for address in 0x8414..=0x8419 { put(bus, address, 0); }
    for address in 0x89EF..=0x89F2 { put(bus, address, 0); }
    for address in 0x8927..=0x8929 { put(bus, address, 0); }
    put(bus, 0x9A81, 0);
}

fn bcd_score(bus: &DigDugBus, address: u16) -> u32 {
    (0..3).map(|i| {
        let byte = get(bus, address + i);
        assert!(byte & 0x0F < 10 && byte >> 4 < 10);
        u32::from((byte >> 4) * 10 + (byte & 0x0F)) * 100u32.pow(u32::from(i))
    }).sum()
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn inflation_stages_pop_scores_depths_and_deflate_timers_match_original_rom() {
    let seed = active_game();
    let mut checked = 0;
    for kind in [0, 1] {
        let monster = if kind == 0 { 0x8525 } else { 0x8565 };
        for direction in [0, 2] {
            for cocktail_player_two in [false, true] {
                for (depth, y) in [0x10, 0x60, 0xA0, 0xE0].into_iter().enumerate() {
                    for difficulty in [1u8, 10] {
                        for (stage, next_stage) in [(0, 4), (4, 8), (8, 0x10), (0x10, 0x20)] {
                            let mut bus = seed.clone();
                            clear_scores(&mut bus);
                            let game_flags = (get(&bus, 0x8400) & !0x07)
                                | if cocktail_player_two { 0x07 } else { 0 };
                            put(&mut bus, 0x8400, game_flags);
                            put(&mut bus, 0x8657, 0);
                            bus.write_word(0, 0x87C8, monster);
                            put(&mut bus, monster, stage | kind);
                            put(&mut bus, monster + 2, 0);
                            put(&mut bus, monster + 3, 0);
                            put(&mut bus, monster + 5, y);
                            put(&mut bus, monster + 0x0C, 0x55);
                            put(&mut bus, 0x8410, difficulty);
                            put(&mut bus, 0x85AE, direction);
                            put(&mut bus, 0x8403, 4);
                            put(&mut bus, 0x8404, 4);
                            put(&mut bus, 0x9A8B, 0);
                            put(&mut bus, 0x9A90, 0);
                            let deflate_time = bus.roms.cpu0[0x32DF + usize::from(difficulty - 1)];
                            let label = format!("kind={kind} dir={direction} cocktail={cocktail_player_two} depth={depth} difficulty={difficulty} stage={stage:02X}");
                            let post = check_boundary(bus, 0, 0x0E9F, &label);
                            checked += 1;
                            assert_eq!(get(&post, monster), next_stage | kind, "{label}");
                            assert_eq!(get(&post, monster + 0x0C),
                                if stage == 0x10 { 0x16 } else { deflate_time }, "{label}");
                            assert_eq!(get(&post, 0x9A90), 1, "inflation sound: {label}");
                            if stage == 0x10 {
                                assert_ne!(get(&post, monster + 3) & 0x80, 0, "{label}");
                                assert_ne!(get(&post, 0x8404) & 0x40, 0, "{label}");
                                assert_eq!(get(&post, 0x9A8B), 1, "pop sound: {label}");
                                let points = [200, 300, 400, 500][depth]
                                    * if kind == 1 && direction & 2 != 0 { 2 } else { 1 };
                                let score = if cocktail_player_two { 0x8417 } else { 0x8414 };
                                assert_eq!(bcd_score(&post, score), points, "pop score: {label}");
                                assert_eq!(bcd_score(&post, 0x89EF), points, "round score: {label}");
                            } else {
                                assert_eq!(bcd_score(&post, 0x8414) + bcd_score(&post, 0x8417), 0, "{label}");
                                assert_eq!(get(&post, monster + 2) & 0x10,
                                    if stage == 0 { 0 } else { 0x10 }, "{label}");
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 256);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn pump_cycle_boundaries_and_held_or_released_input_match_original_rom() {
    let seed = active_game();
    let mut checked = 0;
    for counter in [8u8, 9, 17, 18, 30, 31, 32, 0xFF] {
        for held in [false, true] {
            for pumping in [false, true] {
                let mut bus = seed.clone();
                let prior_stroke = (9..18).contains(&(counter & 0x1F));
                put(&mut bus, 0x8401, 0);
                put(&mut bus, 0x8403, 4 | if prior_stroke { 8 } else { 0 });
                put(&mut bus, 0x8404, if pumping { 4 } else { 0 });
                put(&mut bus, 0x8641, counter);
                put(&mut bus, 0x8657, 0);
                put(&mut bus, 0x85B0, if held { 0x18 } else { 0x38 });
                put(&mut bus, 0x8970, if held { 0x38 } else { 0x18 });
                put(&mut bus, 0x8410, 1);
                bus.write_word(0, 0x87C8, 0x8525);
                put(&mut bus, 0x8525, 4);
                put(&mut bus, 0x8527, 0);
                let label = format!("counter={counter:02X} held={held} pumping={pumping}");
                let post = check_boundary(bus, 0, 0x0DA2, &label);
                checked += 1;
                let next = counter.wrapping_add(1);
                let phase = next & 0x1F;
                let expected_counter = if !pumping || phase == 31 { 0 } else { next };
                assert_eq!(get(&post, 0x8641), expected_counter, "{label}");
                let expected_latch = if !pumping { held } else { held || phase <= 18 };
                assert_eq!(get(&post, 0x8404) & 4 != 0, expected_latch, "{label}");
                assert_eq!(get(&post, 0x8525),
                    if pumping && phase == 9 { 8 } else { 4 }, "{label}");
                if pumping && phase == 9 {
                    assert_ne!(get(&post, 0x8403) & 8, 0, "stroke: {label}");
                }
            }
        }
    }
    assert_eq!(checked, 32);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn harpoon_hitbox_edges_choose_the_target_and_inflate_once() {
    let seed = active_game();
    let mut checked = 0;
    for direction in [0u8, 2, 4, 6] {
        for slot in [0u16, 3, 7] {
            for delta in [-20i16, -8, -6, 0, 6, 8, 20] {
                let mut bus = seed.clone();
                for i in 0..8 { put(&mut bus, 0x8525 + i * 0x10, 0x80); }
                let monster = 0x8525 + slot * 0x10;
                let kind = u8::from(slot >= 4);
                put(&mut bus, monster, kind);
                put(&mut bus, monster + 2, 0);
                put(&mut bus, monster + 3, 0);
                put(&mut bus, monster + 4, 0x71);
                put(&mut bus, monster + 5, 0x80);
                put(&mut bus, monster + 6, 0x0C);
                put(&mut bus, 0x8401, 8);
                put(&mut bus, 0x8403, 0);
                put(&mut bus, 0x8404, 0);
                put(&mut bus, 0x879E, 0);
                put(&mut bus, 0x87E4, direction);
                put(&mut bus, 0x85AE, direction);
                put(&mut bus, 0x8410, 1);
                put(&mut bus, 0x863E, 0x30);
                bus.write_word(0, 0x8642, 0x863E);
                let (x, y) = if direction & 2 == 0 {
                    (0x71, (0x80 + delta) as u8)
                } else { ((0x71 + delta) as u8, 0x80) };
                put(&mut bus, 0x9886, x);
                put(&mut bus, 0x9887, y);
                let label = format!("dir={direction} slot={slot} delta={delta}");
                let post = check_boundary(bus, 0, 0x10E0, &label);
                checked += 1;
                let hit = if direction == 0 || direction == 6 {
                    [6, 8].contains(&delta)
                } else { [-8, -6].contains(&delta) };
                assert_eq!(get(&post, monster), kind | if hit { 4 } else { 0 }, "{label}");
                assert_eq!(get(&post, 0x8403) & 4 != 0, hit, "{label}");
                assert_eq!(get(&post, 0x8401) & 8 == 0, hit, "{label}");
                if hit {
                    assert_eq!(get(&post, 0x879E), 8 - slot as u8, "{label}");
                    let target = u16::from_le_bytes([get(&post, 0x87C8), get(&post, 0x87C9)]);
                    assert_eq!(target, monster, "{label}");
                }
            }
        }
    }
    assert_eq!(checked, 84);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn cpu1_tether_grace_and_release_match_original_rom() {
    let seed = active_game();
    let mut checked = 0;
    for buttons in [0x18, 0x38, 0x32, 0x12] {
        for grace in [0, 1, 2] {
            let mut bus = seed.clone();
            put(&mut bus, 0x8401, 0x20); // Player initialization has completed.
            put(&mut bus, 0x8403, 4);
            put(&mut bus, 0x8404, 4);
            put(&mut bus, 0x8646, 0);
            put(&mut bus, 0x8657, 0);
            put(&mut bus, 0x8785, 5); // Player one's init completed; normal controls selected.
            put(&mut bus, 0x896F, 1); // Normal controls are ready.
            put(&mut bus, 0x879F, grace);
            put(&mut bus, 0x85B0, buttons);
            put(&mut bus, 0x8970, buttons);
            let label = format!("buttons={buttons:02X} grace={grace}");
            let post = check_boundary(bus, 1, 0x0080, &label);
            checked += 1;
            let detach = buttons == 0x32 && grace == 0;
            assert_eq!(get(&post, 0x8403) & 4 == 0, detach, "{label}");
            assert_eq!(get(&post, 0x8404) & 4 == 0, detach, "{label}");
            if detach {
                assert_eq!(get(&post, 0x8641), 0, "{label}");
                assert!((0x9882..0x988A).all(|address| get(&post, address) == 0), "{label}");
            }
        }
    }
    assert_eq!(checked, 12);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn selector_bit_20_preserves_the_original_indirect_flag_word_behavior() {
    // This is a forced branch fixture, not a claim that normal play reaches it.
    // Keep natural tether flag words: the ROM reads their word indirectly as
    // an address, rather than being given an invented pointer to writable RAM.
    for stroke in [0, 8] {
        let mut bus = active_game();
        bus.write_word(0, 0x87C8, 0x8525);
        put(&mut bus, 0x8525, 4);
        put(&mut bus, 0x8527, 0x20);
        put(&mut bus, 0x8410, 1);
        put(&mut bus, 0x8403, 4 | stroke);
        put(&mut bus, 0x8404, 4);
        put(&mut bus, 0x9A90, 0);
        let timer = bus.roms.cpu0[0x32DF];
        let post = check_boundary(bus, 0, 0x0E9F, &format!("selector20 stroke={stroke}"));
        assert_eq!(get(&post, 0x8531), timer);
        assert_eq!(get(&post, 0x8525), 4);
        assert_eq!(get(&post, 0x9A90), 0);
    }
}
