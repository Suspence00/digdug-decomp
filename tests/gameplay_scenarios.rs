//! Gameplay contracts using locally supplied ROMs. These are behavioral scenarios,
//! not a claim of independent hardware or cycle equivalence.
use digdug_re::decomp::{step_human_cpu0, DecompStats};
use digdug_re::hardware::*;
use digdug_re::rom::DigDugRoms;
use digdug_re::verify::{VerificationOutcome, VerifyLog};
use digdug_re::z80::Z80Bus;
use std::path::Path;

fn cabinet() -> DigDugMachine {
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
        .expect("install your own Dig Dug Rev 2 ROMs in roms/extracted");
    DigDugMachine::new(roms)
}

fn run(machine: &mut DigDugMachine, buttons: u8, frames: usize) {
    machine.bus.buttons = buttons;
    for _ in 0..frames {
        machine.run_frame();
    }
}

fn start(machine: &mut DigDugMachine, two_players: bool) {
    run(machine, 0, 5);
    for _ in 0..if two_players { 2 } else { 1 } {
        run(machine, INPUT_COIN, 6);
        run(machine, 0, 14);
    }
    assert_eq!(machine.bus.namco_credit, if two_players { 2 } else { 1 });
    run(machine, if two_players { INPUT_START2 } else { INPUT_START1 }, 6);
    run(machine, 0, 400);
    let s = machine.snapshot();
    assert!(s.in_game && !s.attract_demo);
    assert_eq!((s.active_player, s.p1_round, s.p1_lives), (1, 1, 2));
    assert_eq!(machine.bus.namco_credit, 0);
}

fn wait_until(
    machine: &mut DigDugMachine,
    max_frames: usize,
    event: &str,
    ready: impl Fn(&DigDugMachine) -> bool,
) {
    for _ in 0..max_frames {
        if ready(machine) {
            return;
        }
        run(machine, 0, 1);
    }
    assert!(ready(machine), "timed out waiting for {event}: {:?}", machine.snapshot());
}

fn assert_next_vblank_matches_rom(machine: &DigDugMachine) {
    let mut pre = machine.cpus[0].clone();
    let mut pre_bus = machine.bus.clone();
    assert!(machine.bus.irq_enable[0] && pre.trigger_irq(&mut pre_bus));
    let roms = &machine.bus.roms;
    step_human_cpu0(&mut pre, &mut pre_bus, roms, &mut DecompStats::new());
    assert_eq!(pre.pc, 0x0280);
    let mut post = pre.clone();
    let mut post_bus = pre_bus.clone();
    step_human_cpu0(&mut post, &mut post_bus, roms, &mut DecompStats::new());
    let mut log = VerifyLog::new(100_000);
    log.check(machine.frame_count, "gameplay transition", &pre, &pre_bus, &post, &post_bus);
    let differences: Vec<_> = log.mismatches.values()
        .flat_map(|m| m.reg_example.iter().chain(&m.state_example)).collect();
    assert_eq!(log.outcome(true), VerificationOutcome::Passed,
        "frame {}: {:?}; timeouts={:?}; halts={:?}; unsupported={:?}",
        machine.frame_count, differences, log.timeouts, log.reference_halts, log.unsupported);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn two_players_alternate_after_deaths_and_finish_then_start_a_fresh_game() {
    let mut machine = cabinet();
    start(&mut machine, true);
    let mut handoffs = Vec::new();
    let mut previous_player = 1;
    let mut checked_handoffs = 0;
    let mut saw_game_over = false;
    // Let the enemies kill the stationary players naturally. No game state is patched.
    for _ in 0..10_000 {
        let ram = &machine.bus.shared_ram;
        if ram[0x401] & 0x80 != 0 && ram[0x402] & 1 != 0 && ram[0x61C] == 0
            && ram[0x40A] != 0 && ram[0x40B] != 0
        {
            assert_next_vblank_matches_rom(&machine);
            checked_handoffs += 1;
        }
        run(&mut machine, 0, 1);
        let s = machine.snapshot();
        if !s.in_game {
            assert_eq!((s.p1_lives, s.p2_lives), (0, 0));
            saw_game_over = true;
            break;
        }
        if s.active_player != previous_player {
            handoffs.push(s.active_player);
            previous_player = s.active_player;
            assert_eq!((s.p1_round, s.p2_round), (1, 1));
        }
    }
    assert!(saw_game_over, "both players must finish within the scenario budget");
    assert_eq!(handoffs, [2, 1, 2, 1, 2]);
    assert_eq!(checked_handoffs, 4, "exercise both handoff directions with reserve lives");
    // Two-player mode remains latched after game over; it must not report an active game.
    assert_eq!(machine.bus.shared_ram[0x400] & 0x41, 0x01);
    start(&mut machine, false);
    assert_eq!(machine.bus.shared_ram[0x400] & 0x03, 0);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn single_player_death_restarts_on_the_same_round_and_uses_one_reserve_life() {
    let mut machine = cabinet();
    start(&mut machine, false);
    wait_until(&mut machine, 2_000, "death animation", |m| m.bus.shared_ram[0x401] & 0x80 != 0);
    assert_eq!(machine.snapshot().p1_lives, 2);
    wait_until(&mut machine, 500, "post-death handoff", |m|
        m.bus.shared_ram[0x402] & 1 != 0 && m.bus.shared_ram[0x61C] == 0);
    assert_next_vblank_matches_rom(&machine);
    wait_until(&mut machine, 500, "next life", |m| m.snapshot().p1_lives == 1);
    let s = machine.snapshot();
    assert!(s.in_game);
    assert_eq!((s.active_player, s.p1_round), (1, 1));
    assert_eq!(machine.bus.shared_ram[0x401] & 0x80, 0);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn completed_round_advances_and_respawns_without_spending_a_life() {
    let mut machine = cabinet();
    start(&mut machine, false);
    let lives = machine.snapshot().p1_lives;
    // Fixture: stage the settled end of a round. This tests the transition, not
    // whether a particular input sequence can defeat every enemy.
    for slot in 0..8 {
        machine.bus.write_byte(0, 0x8528 + slot * 0x10, 0x80);
        machine.bus.write_byte(0, 0x852B + slot * 0x10, 0x32);
    }
    for address in [0x98F5, 0x98F7, 0x9A87, 0x8646] {
        machine.bus.write_byte(0, address, 0);
    }
    machine.bus.write_byte(0, 0x9822, 2);
    assert_next_vblank_matches_rom(&machine);
    wait_until(&mut machine, 600, "round two with new enemies", |m| {
        let s = m.snapshot();
        s.p1_round == 2 && s.active_enemies > 0 && m.bus.shared_ram[0x400] & 0x30 == 0x30
    });
    let s = machine.snapshot();
    assert!(s.in_game);
    assert_eq!(s.active_player, 1);
    assert_eq!(s.p1_lives, lives);
}
