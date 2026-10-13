use std::sync::Arc;

use crate::decomp::{
    step_human_cpu0, step_human_cpu1, step_human_cpu2, DecompStats, GameStateSnapshot,
};
use crate::rom::DigDugRoms;
use crate::z80::{Z80, Z80Bus};

pub const INPUT_UP: u8 = 0x01;
pub const INPUT_RIGHT: u8 = 0x02;
pub const INPUT_DOWN: u8 = 0x04;
pub const INPUT_LEFT: u8 = 0x08;
pub const INPUT_FIRE: u8 = 0x10;
pub const INPUT_START1: u8 = 0x20;
pub const INPUT_COIN: u8 = 0x40;
pub const INPUT_START2: u8 = 0x80;

const NAMCO_NMI_DELAY: u8 = 30;

// 3 lives, bonus at 20K & 60K, 1 coin 1 credit
const DIGDUG_DIP_A: u8 = 0b0100_0000 | 0b0010_0000 | 0b0000_0110;
// 1 coin 1 credit, Demo Sounds ON, Allow Continue, Easy difficulty
const DIGDUG_DIP_B: u8 = 0b1100_0000 | 0b0001_0000 | 0b0000_1000 | 0b0000_0011;

#[derive(Clone)]
pub struct DigDugBus {
    pub roms: Arc<DigDugRoms>,
    /// Shared RAM window 0x8000..0x9FFF (8 KB)
    pub shared_ram: [u8; 0x2000],
    /// Namco WSG sound registers 0x6800..0x681F (32 nibbles) for 120 Hz Tick 0 (first half-frame) and Tick 1 (second half-frame)
    pub sound_regs_tick0: [u8; 32],
    pub sound_regs: [u8; 32],
    /// High-score non-volatile EAROM (64 bytes)
    pub earom: [u8; 64],
    pub earom_addr: usize,
    pub earom_data: u8,

    /// LS259 misc latch ($6820..$6827): [irq0_en, irq1_en, sound_nmi_dis]
    pub irq_enable: [bool; 3],
    pub sub_cpu_reset: bool,
    pub video_latch: u8,
    pub game_started: bool,

    /// Namco 06XX / 51XX / 53XX custom I/O state
    pub namco_command: u8,
    pub namco_mode: u8,
    pub namco_nmi_counter: u8,
    pub namco_credit: u8,

    /// Live controller button bitmask and 3-read history for edge detection
    pub buttons: u8,
    pub keymask_d: [u8; 3],

    /// Optional trace of CPU accesses to the Namco 06XX/51XX window ($7000..$71FF) as
    /// `(kind 'R'|'W', address, value)`. `None` in normal play; used by differential tests vs MAME.
    pub trace: Option<Vec<(char, u16, u8)>>,
}

impl DigDugBus {
    pub fn new(roms: DigDugRoms) -> Self {
        Self {
            roms: Arc::new(roms),
            shared_ram: [0u8; 0x2000],
            sound_regs_tick0: [0u8; 32],
            sound_regs: [0u8; 32],
            earom: [0u8; 64],
            earom_addr: 0,
            earom_data: 0,
            irq_enable: [false; 3],
            sub_cpu_reset: true,
            video_latch: 0,
            game_started: false,
            namco_command: 0,
            namco_mode: 0,
            namco_nmi_counter: 0,
            namco_credit: 0,
            buttons: 0,
            keymask_d: [0; 3],
            trace: None,
        }
    }

    fn namco_write_dd(&mut self, addr: u16, val: u8) {
        if let Some(t) = &mut self.trace {
            t.push(('W', 0x7000 | addr, val));
        }
        if (addr & 0x0100) != 0 {
            self.namco_command = val;
            if val == 0xA1 {
                self.namco_mode = 1;
            }
            if val == 0xC1 || val == 0xE1 {
                self.namco_mode = 0;
            }
            if val != 0x10 {
                self.namco_nmi_counter = NAMCO_NMI_DELAY;
            } else {
                self.namco_nmi_counter = 0;
            }
        }
    }

    fn namco_read_dd(&mut self, addr: u16) -> u8 {
        let val = self.namco_read_dd_inner(addr);
        if let Some(t) = &mut self.trace {
            t.push(('R', 0x7000 | addr, val));
        }
        val
    }

    fn namco_read_dd_inner(&mut self, addr: u16) -> u8 {
        if (addr & 0x0100) != 0 {
            return self.namco_command;
        }

        let keymask = self.buttons;
        if self.namco_command == 0x71 && (addr & 0x0F) == 0 {
            self.keymask_d[2] = self.keymask_d[1];
            self.keymask_d[1] = self.keymask_d[0];
            self.keymask_d[0] = keymask;

            if (self.keymask_d[0] & INPUT_COIN) != 0
                && (self.keymask_d[1] & INPUT_COIN) == 0
                && self.namco_credit < 99
            {
                self.namco_credit += 1;
            }

            if (self.keymask_d[0] & INPUT_START1) != 0
                && (self.keymask_d[1] & INPUT_START1) == 0
                && self.namco_credit > 0
            {
                self.namco_credit -= 1;
            }

            if (self.keymask_d[0] & INPUT_START2) != 0
                && (self.keymask_d[1] & INPUT_START2) == 0
                && self.namco_credit >= 2
            {
                self.namco_credit -= 2;
            }
        }

        // Single-read rising edge for Namco 51XX fire button press detection
        let keymask_p = (self.keymask_d[0] ^ self.keymask_d[1]) & self.keymask_d[0];

        match self.namco_command {
            0x71 => {
                if self.namco_mode != 0 {
                    match addr & 0x0F {
                        0 => {
                            let mut retval = 0u8;
                            if (keymask & INPUT_COIN) != 0 {
                                retval |= 0x01;
                            }
                            if (keymask & INPUT_START1) != 0 {
                                retval |= 0x10;
                            }
                            if (keymask & INPUT_START2) != 0 {
                                retval |= 0x20;
                            }
                            !retval
                        }
                        1 => {
                            let mut retval = 0u8;
                            if (keymask & INPUT_UP) != 0 {
                                retval |= 0x01;
                            }
                            if (keymask & INPUT_DOWN) != 0 {
                                retval |= 0x04;
                            }
                            if (keymask & INPUT_LEFT) != 0 {
                                retval |= 0x08;
                            }
                            if (keymask & INPUT_RIGHT) != 0 {
                                retval |= 0x02;
                            }
                            if (keymask & INPUT_FIRE) != 0 {
                                retval |= 0x20;
                            }
                            if (keymask_p & INPUT_FIRE) != 0 {
                                retval |= 0x10;
                            }
                            !retval
                        }
                        2 => 0xFF,
                        _ => 0xFF,
                    }
                } else {
                    match addr & 0x0F {
                        0 => 16 * (self.namco_credit / 10) + (self.namco_credit % 10),
                        1 => {
                            let mut retval = 0u8;
                            if (keymask & INPUT_FIRE) != 0 {
                                retval |= 0x20;
                            }
                            if (keymask_p & INPUT_FIRE) != 0 {
                                retval |= 0x10;
                            }
                            if (keymask & INPUT_UP) != 0 {
                                retval |= 0x0F;
                            } else if (keymask & INPUT_RIGHT) != 0 {
                                retval |= 0x0D;
                            } else if (keymask & INPUT_DOWN) != 0 {
                                retval |= 0x0B;
                            } else if (keymask & INPUT_LEFT) != 0 {
                                retval |= 0x09;
                            } else {
                                retval |= 0x07;
                            }
                            !retval
                        }
                        2 => 0xF8,
                        _ => 0xFF,
                    }
                }
            }
            0xD2 => match addr & 0x0F {
                0 => !DIGDUG_DIP_A,
                1 => !DIGDUG_DIP_B,
                _ => 0xFF,
            },
            0xB1 => {
                if (addr & 0x0F) <= 2 {
                    0x00
                } else {
                    0xFF
                }
            }
            _ => 0xFF,
        }
    }
}

impl Z80Bus for DigDugBus {
    #[inline(always)]
    fn read_byte(&mut self, cpu_id: usize, addr: u16) -> u8 {
        if addr < 0x4000 {
            return match cpu_id {
                0 => self.roms.cpu0[addr as usize],
                1 => self.roms.cpu1[addr as usize],
                _ => self.roms.cpu2[addr as usize],
            };
        }
        if (addr & 0xE000) == 0x8000 {
            return self.shared_ram[(addr - 0x8000) as usize];
        }
        if (addr & 0xFE00) == 0x7000 {
            return self.namco_read_dd(addr & 0x01FF);
        }
        if (addr & 0xFFC0) == 0xB800 {
            return self.earom[self.earom_addr & 0x3F];
        }
        0xFF
    }

    #[inline(always)]
    fn write_byte(&mut self, _cpu_id: usize, addr: u16, val: u8) {
        if (addr & 0xE000) == 0x8000 {
            if addr == 0x8000 + 985 && (val & 0x7F) == 46 {
                self.game_started = true;
            }
            self.shared_ram[(addr - 0x8000) as usize] = val;
            return;
        }
        if (addr & 0xFFE0) == 0x6800 {
            self.sound_regs[(addr - 0x6800) as usize] = val & 0x0F;
            return;
        }
        if (addr & 0xFFF8) == 0x6820 {
            if (addr & 0x0C) == 0x00 {
                let bit_idx = (addr & 3) as usize;
                if bit_idx < 3 {
                    self.irq_enable[bit_idx] = (val & 1) != 0;
                } else {
                    self.sub_cpu_reset = (val & 1) == 0;
                    if self.sub_cpu_reset {
                        self.namco_command = 0;
                        self.namco_mode = 0;
                        self.namco_nmi_counter = 0;
                    }
                }
            }
            return;
        }
        if (addr & 0xFE00) == 0x7000 {
            self.namco_write_dd(addr & 0x01FF, val);
            return;
        }
        if (addr & 0xFFF8) == 0xA000 {
            let bit = 1u8 << (addr & 7);
            if (val & 1) != 0 {
                self.video_latch |= bit;
            } else {
                self.video_latch &= !bit;
            }
            return;
        }
        if (addr & 0xFFC0) == 0xB800 {
            self.earom_addr = (addr & 0x3F) as usize;
            self.earom_data = val;
            return;
        }
        if addr == 0xB840 && (val & 0x02) != 0 {
            self.earom[self.earom_addr & 0x3F] = self.earom_data;
        }
    }
}

/// Complete Dig Dug arcade hardware cabinet and native Rust decompilation runner.
pub struct DigDugMachine {
    pub cpus: [Z80; 3],
    pub bus: DigDugBus,
    pub decomp: DecompStats,
    pub frame_count: u64,
    prev_sub_reset: bool,
    /// When `Some`, every CPU step is cross-checked against the reference Z80 interpreter
    /// running the original ROM bytes (see `verify.rs`). `None` in normal play.
    pub verify: Option<Box<crate::verify::VerifyLog>>,
}

impl DigDugMachine {
    pub fn new(roms: DigDugRoms) -> Self {
        let mut machine = Self::new_unbooted(roms);
        machine.fast_forward_boot();
        machine
    }

    /// Creates the machine at power-on without fast-forwarding through the boot self-test.
    pub fn new_unbooted(roms: DigDugRoms) -> Self {
        Self {
            cpus: [Z80::new(0), Z80::new(1), Z80::new(2)],
            bus: DigDugBus::new(roms),
            decomp: DecompStats::new(),
            frame_count: 0,
            prev_sub_reset: true,
            verify: None,
        }
    }

    /// Warm-resets the arcade cabinet while preserving the non-volatile 64-byte ER2055 EAROM chip.
    pub fn warm_reset(&mut self) {
        let saved_earom = self.bus.earom;
        let roms = (*self.bus.roms).clone();
        self.cpus = [Z80::new(0), Z80::new(1), Z80::new(2)];
        self.bus = DigDugBus::new(roms);
        self.bus.earom = saved_earom;
        self.decomp = DecompStats::new();
        self.frame_count = 0;
        self.prev_sub_reset = true;
        self.fast_forward_boot();
    }

    fn fast_forward_boot(&mut self) {
        // Fast-forward through the initial 1982 power-on RAM/ROM self-test and crosshatch screen
        // (~315 frames) so the user lands right at the title/attract screen immediately on launch!
        for _ in 0..340 {
            self.run_frame();
            if self.bus.game_started {
                for _ in 0..10 {
                    self.run_frame();
                }
                break;
            }
        }
    }

    fn step_cpu_raw(&mut self, cpu_idx: usize, roms_ref: &Arc<DigDugRoms>) {
        match cpu_idx {
            0 => {
                step_human_cpu0(&mut self.cpus[0], &mut self.bus, roms_ref, &mut self.decomp);
            }
            1 => {
                step_human_cpu1(&mut self.cpus[1], &mut self.bus, roms_ref, &mut self.decomp);
            }
            2 => {
                step_human_cpu2(&mut self.cpus[2], &mut self.bus, roms_ref, &mut self.decomp);
            }
            _ => {}
        }
    }

    fn step_cpu(&mut self, cpu_idx: usize, roms_ref: &Arc<DigDugRoms>) {
        if self.verify.is_none() || cpu_idx > 2 {
            self.step_cpu_raw(cpu_idx, roms_ref);
            return;
        }
        let pre_cpu = self.cpus[cpu_idx].clone();
        let pre_bus = self.bus.clone();
        self.step_cpu_raw(cpu_idx, roms_ref);
        let name = self.decomp.last_hooked_name;
        let frame = self.frame_count;
        if let Some(v) = self.verify.as_mut() {
            v.check(frame, name, &pre_cpu, &pre_bus, &self.cpus[cpu_idx], &self.bus);
        }
    }


    pub fn run_frame(&mut self) {
        self.frame_count += 1;
        self.decomp.calls_this_frame = 0;

        // VBlank interrupts (IM 1 -> RST $38) for CPU 0 and CPU 1
        if self.bus.irq_enable[0] {
            self.cpus[0].trigger_irq(&mut self.bus);
        }
        if !self.bus.sub_cpu_reset && self.bus.irq_enable[1] {
            self.cpus[1].trigger_irq(&mut self.bus);
        }

        let roms_ref = Arc::clone(&self.bus.roms);
        const SUB_STEPS_PER_QUARTER: usize = 300_000 / 60 / 4; // 1250 groups of 4 instructions = 5000 inst/frame

        for quarter in 0..4 {
            for _ in 0..SUB_STEPS_PER_QUARTER {
                // Execute 4 steps on CPU 0 (Main)
                for _ in 0..4 {
                    self.step_cpu(0, &roms_ref);
                    if self.bus.sub_cpu_reset && !self.prev_sub_reset {
                        self.cpus[1].reset();
                        self.cpus[2].reset();
                    }
                    self.prev_sub_reset = self.bus.sub_cpu_reset;
                }

                if !self.bus.sub_cpu_reset {
                    // Execute 4 steps on CPU 1 (Sub/AI)
                    for _ in 0..4 {
                        self.step_cpu(1, &roms_ref);
                    }
                    // Execute 4 steps on CPU 2 (Sound)
                    for _ in 0..4 {
                        self.step_cpu(2, &roms_ref);
                    }
                }

                // Namco 06XX NMI timer for CPU 0
                if self.bus.namco_nmi_counter > 0 {
                    self.bus.namco_nmi_counter -= 1;
                    if self.bus.namco_nmi_counter == 0 {
                        self.cpus[0].trigger_nmi(&mut self.bus);
                        if self.bus.namco_command != 0x10 {
                            self.bus.namco_nmi_counter = NAMCO_NMI_DELAY;
                        }
                    }
                }
            }

            // Snapshot the first 120 Hz Sound CPU tick registers before triggering the second half-frame NMI
            if quarter == 1 {
                self.bus.sound_regs_tick0 = self.bus.sound_regs;
            }

            // Sound CPU (CPU 2) periodic NMI at scanlines ~64 and ~192 (120 Hz)
            if !self.bus.sub_cpu_reset
                && !self.bus.irq_enable[2]
                && (quarter == 1 || quarter == 3)
            {
                self.cpus[2].trigger_nmi(&mut self.bus);
            }
        }
    }

    pub fn snapshot(&self) -> GameStateSnapshot {
        GameStateSnapshot::from_shared_ram(&self.bus.shared_ram, self.bus.namco_credit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_highscore_big_score_flash_and_initial_entry() {
        let rom_dir = Path::new("roms/extracted");
        let roms = DigDugRoms::load_from_extracted_dir(rom_dir).expect("Failed to load ROMs");
        let mut machine = DigDugMachine::new(roms);

        // Set Player 1 Game Over state with a score of 25,000 ($8414..$8416 little-endian BCD)
        machine.bus.write_byte(0, 0x8400, 0x48); // In-game (bit 6) + initialized (bit 3), Player 1
        machine.bus.write_byte(0, 0x8401, 0x80); // Player dead
        machine.bus.write_byte(0, 0x8402, 0x13); // Game Over banner finished
        machine.bus.write_byte(0, 0x861C, 0x00);
        machine.bus.write_byte(0, 0x861E, 0x00);
        machine.bus.write_byte(0, 0x840A, 0x00); // 0 lives remaining
        machine.bus.write_byte(0, 0x840D, 0x03); // Round 3
        machine.bus.write_byte(0, 0x8657, 0x00); // Not attract demo
        machine.bus.write_byte(0, 0x8414, 0x00);
        machine.bus.write_byte(0, 0x8415, 0x50);
        machine.bus.write_byte(0, 0x8416, 0x02);
        // Trigger high-score check ($87CC bit 1 = 1, bit 2 = 0)
        machine.bus.write_byte(0, 0x87CC, 0x02);

        let mut saw_flash_on = false;
        let mut saw_flash_off = false;

        // Step through the #1 High Score "CONGRATULATIONS !" flashing big score banner ($3552..$356D)
        for _ in 0..400 {
            machine.run_frame();
            if (0x3552..=0x356D).contains(&machine.cpus[0].pc) {
                let f23 = machine.bus.read_byte(0, 0x8423);
                if (f23 & 0x1F) == 0x02 {
                    // Flash ON phase: check that all 4 rows ($836D..$8370) have non-blank tiles
                    for row_off in 0..4u16 {
                        let has_digit_tile = (0..0x18u16).any(|col| {
                            let addr = (0x836Du16 + row_off).wrapping_sub(col * 0x20);
                            machine.bus.read_byte(0, addr) != 0x40
                        });
                        assert!(
                            has_digit_tile,
                            "Row {} of big score should contain digit tiles when flashed ON",
                            row_off
                        );
                    }
                    saw_flash_on = true;
                } else if (f23 & 0x1F) == 0x12 {
                    // Flash OFF phase: ALL 4 rows ($836D..$8370) across all 24 columns must be cleared to 0x40!
                    for row_off in 0..4u16 {
                        for col in 0..0x18u16 {
                            let addr = (0x836Du16 + row_off).wrapping_sub(col * 0x20);
                            let tile = machine.bus.read_byte(0, addr);
                            assert_eq!(
                                tile, 0x40,
                                "Row {} col {} at ${:04X} was not cleared during big score flash-OFF!",
                                row_off, col, addr
                            );
                        }
                    }
                    saw_flash_off = true;
                }
                if saw_flash_on && saw_flash_off {
                    // End the fanfare wait so we transition into initial entry ($33EB)
                    machine.bus.write_byte(0, 0x9A82, 0x00);
                }
            }
            if (0x33EB..=0x3481).contains(&machine.cpus[0].pc) {
                break;
            }
        }

        assert!(saw_flash_on, "Should have observed big score flashed ON");
        assert!(saw_flash_off, "Should have observed all 4 rows of big score flashed OFF");
        assert!(
            (0x33EB..=0x3481).contains(&machine.cpus[0].pc),
            "Should be in initial entry loop ($33EB..$3481), got PC=${:04X}",
            machine.cpus[0].pc
        );

        // Initial #1 (index 0): starts at $812F ('A' = 0x1A).
        assert_eq!(machine.bus.read_byte(0, 0x89C6), 0);
        assert_eq!(machine.bus.read_word(0, 0x89C9), 0x812F);

        // Move joystick RIGHT for 5 frames to change 1st initial from 'A' (0x1A) to 'B' (0x1B)
        for _ in 0..5 {
            machine.bus.buttons = INPUT_RIGHT;
            machine.run_frame();
        }
        machine.bus.buttons = 0;
        machine.run_frame();
        assert_eq!(machine.bus.read_byte(0, 0x812F) & 0x3F, 0x1B);

        // Press and hold FIRE for 6 frames (realistic human button press) to lock in 1st initial
        for _ in 0..6 {
            machine.bus.buttons = INPUT_FIRE;
            machine.run_frame();
        }
        machine.bus.buttons = 0;
        for _ in 0..4 {
            machine.run_frame();
        }

        // Must be on the MIDDLE initial (index 1, VRAM $810F) — NOT skipped to index 2!
        assert_eq!(
            machine.bus.read_byte(0, 0x89C6),
            1,
            "Pressing Fire once must advance to middle initial (1), not skip it!"
        );
        assert_eq!(machine.bus.read_word(0, 0x89C9), 0x810F);

        // Move joystick RIGHT to change 2nd (middle) initial from 'A' (0x1A) to 'C' (0x1C)
        for _ in 0..20 {
            machine.bus.buttons = INPUT_RIGHT;
            machine.run_frame();
            if (machine.bus.read_byte(0, 0x810F) & 0x3F) == 0x1C {
                break;
            }
        }
        machine.bus.buttons = 0;
        machine.run_frame();
        assert_eq!(machine.bus.read_byte(0, 0x810F) & 0x3F, 0x1C);

        // Press and hold FIRE for 6 frames to lock in 2nd (middle) initial
        for _ in 0..6 {
            machine.bus.buttons = INPUT_FIRE;
            machine.run_frame();
        }
        machine.bus.buttons = 0;
        for _ in 0..4 {
            machine.run_frame();
        }

        // Must now be on the 3rd initial (index 2, VRAM $80EF)!
        assert_eq!(machine.bus.read_byte(0, 0x89C6), 2);
        assert_eq!(machine.bus.read_word(0, 0x89C9), 0x80EF);

        // Move joystick RIGHT to change 3rd initial from 'A' (0x1A) to 'D' (0x1D)
        for _ in 0..35 {
            machine.bus.buttons = INPUT_RIGHT;
            machine.run_frame();
            if (machine.bus.read_byte(0, 0x80EF) & 0x3F) == 0x1D {
                break;
            }
        }
        machine.bus.buttons = 0;
        machine.run_frame();
        assert_eq!(machine.bus.read_byte(0, 0x80EF) & 0x3F, 0x1D);

        // Press and hold FIRE for 6 frames to lock in 3rd initial
        for _ in 0..6 {
            machine.bus.buttons = INPUT_FIRE;
            machine.run_frame();
        }
        machine.bus.buttons = 0;
        for _ in 0..10 {
            machine.run_frame();
        }

        // Verify all 3 initials ("BCD" = [0x1B, 0x1C, 0x1D]) are stored in 1st place ($89BC..$89BE)
        assert_eq!(machine.bus.read_byte(0, 0x89C6), 3);
        assert_eq!(machine.bus.read_byte(0, 0x89BC), 0x1B);
        assert_eq!(machine.bus.read_byte(0, 0x89BD), 0x1C);
        assert_eq!(machine.bus.read_byte(0, 0x89BE), 0x1D);
    }

    #[test]
    fn test_two_player_game_start() {
        use std::path::Path;
        let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
            .expect("ROMs missing in roms/extracted");
        let mut machine = DigDugMachine::new(roms);

        // Power-on self-test settling frames
        for _ in 0..100 {
            machine.run_frame();
        }

        // Insert Coin 1
        for _ in 0..6 {
            machine.bus.buttons = INPUT_COIN;
            machine.run_frame();
        }
        for _ in 0..10 {
            machine.bus.buttons = 0;
            machine.run_frame();
        }
        assert_eq!(machine.bus.namco_credit, 1);

        // Insert Coin 2
        for _ in 0..6 {
            machine.bus.buttons = INPUT_COIN;
            machine.run_frame();
        }
        for _ in 0..10 {
            machine.bus.buttons = 0;
            machine.run_frame();
        }
        assert_eq!(machine.bus.namco_credit, 2);

        // Press 2-Player Start button
        for _ in 0..6 {
            machine.bus.buttons = INPUT_START2;
            machine.run_frame();
        }
        for _ in 0..10 {
            machine.bus.buttons = 0;
            machine.run_frame();
        }

        // Both credits should be consumed
        assert_eq!(machine.bus.namco_credit, 0);

        // Run through opening fanfare until active gameplay begins
        for _ in 0..500 {
            machine.run_frame();
        }

        // $8400 bit 0 must be 1 (2-player game) and bit 6 must be 1 (game active)
        let flags_8400 = machine.bus.read_byte(0, 0x8400);
        assert_ne!(flags_8400 & 0x01, 0, "Expected bit 0 ($01) set for 2-Player mode");
        assert_ne!(flags_8400 & 0x40, 0, "Expected bit 6 ($40) set for Active Game");

        let snap = machine.snapshot();
        assert!(snap.in_game, "Expected in_game to be true");
        assert_eq!(snap.active_player, 1, "Player 1 should start first");
        // Starting DIP switch is 3 lives: Player 1 has 1 active + 2 in reserve ($840A = 2),
        // while Player 2 has not started yet and has 3 lives ($840B = 3).
        assert_eq!(snap.p1_lives, 2, "Player 1 should have 2 lives in reserve");
        assert_eq!(snap.p2_lives, 3, "Player 2 should have 3 lives in reserve");
    }
}
