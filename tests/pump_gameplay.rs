//! Natural pump encounters reached through input only; no game RAM is staged.
use digdug_re::decomp::MonsterStatus;
use digdug_re::hardware::{DigDugMachine, INPUT_FIRE, INPUT_LEFT};
use digdug_re::rom::DigDugRoms;
use std::path::Path;

struct Encounter {
    machine: DigDugMachine,
    replay: Vec<u8>,
    target: usize,
}

impl Encounter {
    fn approach(script: &str) -> Self {
        let replay: Vec<u8> = script.lines()
            .map(|line| line.split('#').next().unwrap().trim())
            .filter(|line| !line.is_empty())
            .flat_map(|line| {
                let mut fields = line.split_whitespace();
                let buttons = u8::from_str_radix(fields.next().unwrap(), 16).unwrap();
                let frames: usize = fields.next().unwrap().parse().unwrap();
                std::iter::repeat(buttons).take(frames)
            }).collect();
        let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
            .expect("install your own Dig Dug Rev 2 ROMs in roms/extracted");
        let mut encounter = Self { machine: DigDugMachine::new_unbooted(roms), replay, target: 2 };
        for _ in 0..1800 {
            encounter.replay_frame();
            let s = encounter.machine.snapshot();
            if s.in_game && !s.attract_demo && s.harpoon_tethered {
                let ram = &encounter.machine.bus.shared_ram;
                let target = u16::from_le_bytes([ram[0x7C8], ram[0x7C9]]);
                assert_eq!(target, 0x8545, "the replay must hook its intended Pooka");
                assert_eq!(s.monster_inflate[encounter.target], 1);
                assert_eq!(s.monster_status[encounter.target], MonsterStatus::Inflating);
                encounter.assert_live_game();
                return encounter;
            }
        }
        panic!("replay never hooked the Pooka: {:?}", encounter.machine.snapshot());
    }

    fn replay_frame(&mut self) {
        let next_frame = self.machine.frame_count + 1;
        let buttons = next_frame.checked_sub(700)
            .and_then(|index| self.replay.get(index as usize)).copied().unwrap_or(0);
        self.frame(buttons);
    }

    fn frame(&mut self, buttons: u8) {
        self.machine.bus.buttons = buttons;
        self.machine.run_frame();
    }

    fn assert_live_game(&self) {
        let s = self.machine.snapshot();
        assert!(s.in_game && !s.attract_demo && s.p1_lives == 2
            && self.machine.bus.shared_ram[0x401] & 0x80 == 0,
            "encounter must stay in the same live game: frame {}: {s:?}", self.machine.frame_count);
        assert_eq!(self.machine.bus.namco_credit, 1, "pumping must preserve the spare credit");
    }
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn held_pump_inflates_then_pops_one_enemy_and_awards_its_score_once() {
    let mut encounter = Encounter::approach(include_str!("../tools/mame/pump_interaction.txt"));
    let before = encounter.machine.snapshot();
    let mut stages = vec![1];
    for _ in 0..180 {
        encounter.replay_frame();
        encounter.assert_live_game();
        let s = encounter.machine.snapshot();
        let stage = s.monster_inflate[encounter.target];
        if stage != 0 && stages.last() != Some(&stage) { stages.push(stage); }
        assert_eq!((s.digdug_x, s.digdug_y), (before.digdug_x, before.digdug_y));
        if s.monster_status[encounter.target] == MonsterStatus::DefeatedPopped && !s.harpoon_tethered {
            break;
        }
    }
    let after = encounter.machine.snapshot();
    assert_eq!(stages, [1, 2, 3, 4]);
    assert_eq!(after.monster_status[encounter.target], MonsterStatus::DefeatedPopped);
    assert_eq!(after.active_enemies + 1, before.active_enemies);
    assert_eq!(after.p1_score, before.p1_score + 400, "Pooka at this depth is worth 400 points");
    assert!(!after.harpoon_active);
    assert!(encounter.machine.bus.shared_ram[0x1882..0x188A].iter().all(|&v| v == 0),
        "the popped enemy must leave no cable segments");
    for _ in 0..30 {
        encounter.frame(INPUT_FIRE);
        encounter.assert_live_game();
        assert!(!encounter.machine.snapshot().harpoon_active, "holding cannot launch another shot");
        assert_eq!(encounter.machine.snapshot().p1_score, after.p1_score, "pop score cannot repeat");
    }
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn releasing_and_moving_detaches_and_lets_the_enemy_recover() {
    let mut encounter = Encounter::approach(include_str!("../tools/mame/pump_release.txt"));
    let before = encounter.machine.snapshot();
    let (mut reached_second_stage, mut detached, mut moved, mut deflated, mut recovered) =
        (false, false, false, false, false);
    for _ in 0..200 {
        encounter.replay_frame();
        encounter.assert_live_game();
        let s = encounter.machine.snapshot();
        let stage = s.monster_inflate[encounter.target];
        reached_second_stage |= stage == 2;
        detached |= !s.harpoon_tethered;
        moved |= (s.digdug_x, s.digdug_y) != (before.digdug_x, before.digdug_y);
        deflated |= reached_second_stage && stage == 1;
        recovered |= deflated && matches!(s.monster_status[encounter.target], MonsterStatus::Walking | MonsterStatus::Ghosting);
        assert!(!matches!(s.monster_status[encounter.target], MonsterStatus::Popping | MonsterStatus::DefeatedPopped));
        if recovered { break; }
    }
    assert!(reached_second_stage && detached && moved && deflated && recovered,
        "release must detach, permit movement, and let the surviving enemy recover");
    let after = encounter.machine.snapshot();
    assert_eq!(after.active_enemies, before.active_enemies);
    assert!(after.p1_score < before.p1_score + 400, "digging may score, but release cannot award the pop");
    assert!(!after.harpoon_active);
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn moving_while_holding_pump_keeps_the_tether_until_fire_is_released() {
    let mut encounter = Encounter::approach(include_str!("../tools/mame/pump_interaction.txt"));
    let before = encounter.machine.snapshot();
    let mut advanced = false;
    for _ in 0..16 {
        encounter.frame(INPUT_FIRE | INPUT_LEFT);
        encounter.assert_live_game();
        let s = encounter.machine.snapshot();
        assert!(s.harpoon_tethered);
        assert_eq!((s.digdug_x, s.digdug_y), (before.digdug_x, before.digdug_y));
        advanced |= s.monster_inflate[encounter.target] == 2;
    }
    assert!(advanced, "holding pump must continue inflation despite direction input");
    let mut detached = false;
    let mut moved = false;
    for _ in 0..24 {
        encounter.frame(INPUT_LEFT);
        encounter.assert_live_game();
        let s = encounter.machine.snapshot();
        detached |= !s.harpoon_tethered;
        moved |= (s.digdug_x, s.digdug_y) != (before.digdug_x, before.digdug_y);
    }
    assert!(detached && moved, "releasing fire must free the player to move away");
    assert!(!encounter.machine.snapshot().harpoon_tethered);
    assert!(encounter.machine.bus.shared_ram[0x1882..0x188A].iter().all(|&v| v == 0));
}

#[test]
#[ignore = "requires user-supplied Dig Dug Rev 2 ROMs; run with --include-ignored"]
fn releasing_without_movement_stops_inflation_until_the_enemy_recovers() {
    let mut encounter = Encounter::approach(include_str!("../tools/mame/pump_interaction.txt"));
    for _ in 0..16 { encounter.frame(INPUT_FIRE); }
    let before = encounter.machine.snapshot();
    assert_eq!(before.monster_inflate[encounter.target], 2);
    let mut deflated = false;
    let mut recovered = false;
    for _ in 0..200 {
        encounter.frame(0);
        encounter.assert_live_game();
        let s = encounter.machine.snapshot();
        assert_eq!((s.digdug_x, s.digdug_y), (before.digdug_x, before.digdug_y));
        assert!(s.monster_inflate[encounter.target] <= 2, "release must stop further inflation");
        assert_eq!(s.p1_score, before.p1_score, "a surviving enemy awards no pop score");
        deflated |= s.monster_inflate[encounter.target] == 1;
        recovered = deflated && matches!(s.monster_status[encounter.target], MonsterStatus::Walking | MonsterStatus::Ghosting);
        if recovered {
            assert!(!s.harpoon_active, "enemy recovery must free the tether");
            break;
        }
        assert!(s.harpoon_tethered, "standing still retains the tether until recovery");
    }
    assert!(deflated && recovered, "released enemy must shrink and recover");
    assert_eq!(encounter.machine.snapshot().active_enemies, before.active_enemies);
}
