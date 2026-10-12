use crate::rom::DigDugRoms;
use crate::z80::{ Z80, Z80Bus, FLAG_C, FLAG_H, FLAG_N, FLAG_PV, FLAG_S, FLAG_Z };

/// Tracks execution telemetry for our native Rust decompiled subroutines and static AOT recompiler.
#[derive(Clone, Debug, Default)]
pub struct DecompStats {
    pub total_rust_calls: u64,
    pub calls_this_frame: u32,
    pub aot_instructions_executed: u64,
    pub rst_table_calls: u64,
    pub dirt_init_calls: u64,
    pub round_advance_calls: u64,
    pub bcd_score_calls: u64,
    pub monster_speed_calls: u64,
    pub harpoon_pump_calls: u64,
    pub rock_physics_calls: u64,
    pub ghost_dig_ai_calls: u64,
    pub hud_lives_calls: u64,
    pub sound_engine_calls: u64,
    pub last_hooked_name: &'static str,
}

impl DecompStats {
    pub fn new() -> Self {
        Self {
            total_rust_calls: 0,
            calls_this_frame: 0,
            aot_instructions_executed: 0,
            rst_table_calls: 0,
            dirt_init_calls: 0,
            round_advance_calls: 0,
            bcd_score_calls: 0,
            monster_speed_calls: 0,
            harpoon_pump_calls: 0,
            rock_physics_calls: 0,
            ghost_dig_ai_calls: 0,
            hud_lives_calls: 0,
            sound_engine_calls: 0,
            last_hooked_name: "cpu0_boot_init",
        }
    }

    #[inline]
    pub fn merge(&mut self, other: &Self) {
        self.total_rust_calls += other.total_rust_calls;
        self.calls_this_frame += other.calls_this_frame;
        self.aot_instructions_executed += other.aot_instructions_executed;
        self.rst_table_calls += other.rst_table_calls;
        self.dirt_init_calls += other.dirt_init_calls;
        self.round_advance_calls += other.round_advance_calls;
        self.bcd_score_calls += other.bcd_score_calls;
        self.monster_speed_calls += other.monster_speed_calls;
        self.harpoon_pump_calls += other.harpoon_pump_calls;
        self.rock_physics_calls += other.rock_physics_calls;
        self.ghost_dig_ai_calls += other.ghost_dig_ai_calls;
        self.hud_lives_calls += other.hud_lives_calls;
        self.sound_engine_calls += other.sound_engine_calls;
        if !other.last_hooked_name.is_empty() && other.last_hooked_name != "cpu0_boot_init" {
            self.last_hooked_name = other.last_hooked_name;
        }
    }
}

thread_local! {
    static SUB_STATS_ACCUM: std::cell::RefCell<DecompStats> =
        std::cell::RefCell::new(DecompStats::new());
}

#[inline]
fn flush_nested_stats(stats: &mut DecompStats) {
    SUB_STATS_ACCUM.with(|acc| {
        let mut inner = acc.borrow_mut();
        if inner.total_rust_calls > 0 {
            stats.merge(&inner);
            *inner = DecompStats::new();
        }
    });
}

/// High-level strongly-typed snapshot of Dig Dug's shared RAM ($8000..$9FFF),
/// decoded directly from our reverse-engineered memory map.
#[derive(Clone, Debug)]
pub struct GameStateSnapshot {
    pub in_game: bool,
    pub active_player: u8,
    pub attract_demo: bool,
    pub credits_bcd: u8,
    pub p1_lives: u8,
    pub p2_lives: u8,
    pub p1_round: u8,
    pub p2_round: u8,
    pub active_round: u8,
    pub difficulty_step: u8,
    pub stage_layout_idx: u8,
    pub monster_speed: u8,
    pub bonus_veg_round_clamped: u8,
    pub p1_score: u32,
    pub high_score: u32,
    pub digdug_x: u8,
    pub digdug_y: u8,
    pub digdug_sprite: u8,
    pub harpoon_active: bool,
    pub harpoon_timer: u8,
    pub active_enemies: usize,
    pub monster_x: [u8; 6],
    pub monster_y: [u8; 6],
    pub monster_inflate: [u8; 6],
    pub monster_type_mask: u8,
    pub monster_ghost_mask: u8,
}

impl GameStateSnapshot {
    /// Decodes a 6-digit score from Foreground Tilemap VRAM given the starting tile offset and stride.
    fn decode_vram_score(mem: &[u8; 0x2000], start_offs: usize) -> u32 {
        let mut score = 0u32;
        for i in 0..6 {
            let tile = mem[start_offs - i] & 0x7F;
            let digit = if (0x10..=0x19).contains(&tile) {
                (tile - 0x10) as u32
            } else if (0x00..=0x09).contains(&tile) {
                tile as u32
            } else {
                0
            };
            score = score * 10 + digit;
        }
        score
    }

    pub fn from_shared_ram(mem: &[u8; 0x2000], credits_bcd: u8) -> Self {
        let r = |addr: u16| -> u8 { mem[(addr - 0x8000) as usize] };

        let flags_8400 = r(0x8400);
        let attract_demo = (r(0x8657) & 0x02) != 0;
        let in_game = (flags_8400 & 0x01) != 0 || (!attract_demo && r(0x840D) > 0 && r(0x840A) > 0);
        let active_player = if (flags_8400 & 0x02) != 0 { 2 } else { 1 };

        // Dig Dug's live pixel coordinates ($98A2=X, $98A3=Y) and hardware sprite #0 ($8B80)
        let digdug_sprite = r(0x8B80);
        let digdug_x = r(0x98A2);
        let digdug_y = r(0x98A3);
        let harpoon_active = (r(0x8401) & 0x08) != 0 || (r(0x8403) & 0x04) != 0;
        let harpoon_timer = r(0x863E);

        // Decode the first 6 16-byte monster actor structs ($8525 + i*16)
        let mut active_enemies = 0;
        let mut monster_x = [0u8; 6];
        let mut monster_y = [0u8; 6];
        let mut monster_inflate = [0u8; 6];
        let mut monster_type_mask = 0u8;
        let mut monster_ghost_mask = 0u8;

        for i in 0..8 {
            let base = 0x8525 + (i as u16) * 0x10;
            let state0 = r(base);
            let active_byte = r(base + 3);
            let mx = r(base + 4);
            let my = r(base + 5);
            let is_alive = (active_byte & 0x80) == 0 && mx > 1 && my > 1;
            if is_alive {
                active_enemies += 1;
            }
            if i < 6 {
                monster_x[i] = if is_alive { mx } else { 0 };
                monster_y[i] = if is_alive { my } else { 0 };
                if (state0 & 0x01) != 0 {
                    monster_type_mask |= 1 << i;
                }
                if (state0 & 0x02) != 0 {
                    monster_ghost_mask |= 1 << i;
                }
                monster_inflate[i] = if (state0 & 0x20) != 0 {
                    4
                } else if (state0 & 0x10) != 0 {
                    3
                } else if (state0 & 0x08) != 0 {
                    2
                } else if (state0 & 0x04) != 0 {
                    1
                } else {
                    0
                };
            }
        }

        Self {
            in_game,
            active_player,
            attract_demo,
            credits_bcd,
            p1_lives: r(0x840A),
            p2_lives: r(0x840B),
            p1_round: r(0x840D),
            p2_round: r(0x840E),
            active_round: r(0x840F),
            difficulty_step: r(0x8410),
            stage_layout_idx: r(0x8627),
            monster_speed: r(0x87BD),
            bonus_veg_round_clamped: r(0x87E8),
            p1_score: Self::decode_vram_score(mem, 1020),
            high_score: Self::decode_vram_score(mem, 1012),
            digdug_x,
            digdug_y,
            digdug_sprite,
            harpoon_active,
            harpoon_timer,
            active_enemies,
            monster_x,
            monster_y,
            monster_inflate,
            monster_type_mask,
            monster_ghost_mask,
        }
    }
}

/// Decompiled from CPU0 `$1A21` (`sub_1A21`):
/// Converts an 8-bit binary integer at `(HL)` into a 2-digit packed BCD value at `(HL)`
/// (and hundreds digit in `DE`), matching the exact Z80 `ADC A, A; DAA` loop.
pub fn decomp_cpu0_binary_to_bcd(val: u8) -> (u8, u8) {
    if val == 0 {
        return (0, 0);
    }
    let mut c = val;
    let mut bcd_lo = 0u8;
    let mut bcd_hi = 0u8;

    for _ in 0..8 {
        let carry_in = (c & 0x80) != 0;
        c <<= 1;

        // ADC A, A; DAA on bcd_lo
        let mut sum = (bcd_lo as u16) * 2 + (if carry_in { 1 } else { 0 });
        let mut adjust = 0u16;
        if (bcd_lo & 0x0F) * 2 + (if carry_in { 1 } else { 0 }) > 9 {
            adjust |= 0x06;
        }
        if sum > 0x99 || (sum + adjust) > 0x99 {
            adjust |= 0x60;
        }
        sum += adjust;
        let carry_out = sum > 0xFF;
        bcd_lo = (sum & 0xFF) as u8;

        // ADC A, A; DAA on bcd_hi
        let sum_hi = (bcd_hi as u16) * 2 + (if carry_out { 1 } else { 0 });
        bcd_hi = (sum_hi & 0xFF) as u8;
    }
    (bcd_lo, bcd_hi)
}

/// Intercepts Z80 subroutine entry points on CPU0, CPU1, and CPU2 and executes the decompiled
/// native Rust implementation directly against `Z80` + `Z80Bus`.
/// Returns `true` if a decompiled Rust function handled the subroutine call (and performed `RET`).
pub fn try_run_decompiled_subroutine<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    stats: &mut DecompStats,
) -> bool {
    match (cpu.cpu_id, cpu.pc) {
        // =====================================================================
        // CPU 0/1/2 $0008: `rst_08_table_add` -> HL = HL + 2 * A
        // =====================================================================
        (0, 0x0008) | (1, 0x0008) | (2, 0x0008) => {
            let a0 = cpu.a;
            cpu.add8(a0, false);
            if (cpu.f & FLAG_C) != 0 {
                cpu.h = cpu.inc8(cpu.h);
            }
            let l = cpu.l;
            cpu.add8(l, false);
            cpu.l = cpu.a;
            if (cpu.f & FLAG_C) != 0 {
                cpu.h = cpu.inc8(cpu.h);
            }
            stats.rst_table_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "rst_08_table_add";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 32;
            true
        }

        // =====================================================================
        // CPU 0/1/2 $0010: `rst_10_hl_plus_a` -> HL = HL + A
        // =====================================================================
        (0, 0x0010) | (1, 0x0010) | (2, 0x0010) => {
            let l = cpu.l;
            cpu.add8(l, false);
            cpu.l = cpu.a;
            if (cpu.f & FLAG_C) != 0 {
                cpu.h = cpu.inc8(cpu.h);
            }
            stats.rst_table_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "rst_10_hl_plus_a";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 24;
            true
        }

        // =====================================================================
        // CPU 0 $0018: `dormant_round_rollover`
        // =====================================================================
        (0, 0x0018) => {
            if (cpu.f & FLAG_Z) != 0 {
                cpu.a = 156;
                bus.write_byte(0, cpu.hl(), 156);
            }
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "dormant_round_rollover";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 20;
            true
        }

        // =====================================================================
        // CPU 2 $0018: `cpu2_sound_memset` (Sound CPU RST $18 fast fill)
        // Original Z80:
        //   0018: LD (HL), A; INC HL; DJNZ $0018; RET
        // =====================================================================
        (2, 0x0018) => {
            let mut hl = cpu.hl();
            let mut b = cpu.b;
            let val = cpu.a;
            let mut cycles = 10u64;
            loop {
                bus.write_byte(2, hl, val);
                hl = hl.wrapping_add(1);
                b = b.wrapping_sub(1);
                cycles += 24;
                if b == 0 {
                    break;
                }
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_sound_memset";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += cycles;
            true
        }

        // =====================================================================
        // CPU 2 $0066: `cpu2_nmi_audio_tick` (120 Hz Sound NMI Sequencer & WSG Synth)
        // =====================================================================
        (2, 0x0066) => {
            decomp_cpu2_nmi_audio_tick(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_nmi_audio_tick";
            true
        }

        // =====================================================================
        // CPU 2 $031A: `cpu2_init_music_track`
        // =====================================================================
        (2, 0x031A) => {
            decomp_cpu2_init_music_track(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_init_music_track";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 320;
            true
        }

        // =====================================================================
        // CPU 2 $034E: `cpu2_step_music_track`
        // =====================================================================
        (2, 0x034E) => {
            decomp_cpu2_step_music_track(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_step_music_track";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 260;
            true
        }

        // =====================================================================
        // CPU 2 $0391: `cpu2_step_sfx_track`
        // =====================================================================
        (2, 0x0391) => {
            decomp_cpu2_step_sfx_track(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_step_sfx_track";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 280;
            true
        }

        // =====================================================================
        // CPU 2 $04D5: `cpu2_synthesize_voice`
        // =====================================================================
        (2, 0x04D5) => {
            decomp_cpu2_synthesize_voice(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_synthesize_voice";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 210;
            true
        }

        // =====================================================================
        // CPU 2 $067F: `cpu2_boot_checksum_handshake`
        // Sums all 4 KB of Sound ROM ($0000..$0FFF == $AA), writes $FF to $8A01,
        // and advances PC to the handshake wait loop at $0695.
        // =====================================================================
        (2, 0x067F) => {
            let mut sum = 0u8;
            for &b in &roms.cpu2[0..0x1000] {
                sum = sum.wrapping_add(b);
            }
            cpu.set_hl(0x1000);
            cpu.b = 0;
            cpu.a = sum;
            cpu.sub8(0xAA, false, false);
            if sum == 0xAA {
                cpu.a = 0xFF;
                bus.write_byte(2, 0x8A01, 0xFF);
                cpu.pc = 0x0695;
            } else {
                cpu.a = 0x06;
                bus.write_byte(2, 0x8A01, 0x06);
                cpu.pc = 0x06A1;
            }
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu2_boot_checksum";
            cpu.total_cycles += 98_500;
            true
        }

        // =====================================================================
        // CPU 0 $0C51 & $0C6F: `init_bonus_life_thresholds`
        // Initializes the 6-digit BCD bonus-life score thresholds at $862A (1st),
        // $862D (P1 recurring), and $8630 (P2 recurring) from DIP table $85AA..$85AD.
        // =====================================================================
        (0, 0x0C51) => {
            decomp_set_bcd_threshold(cpu, bus, 0x862A, 0x85AA);
            decomp_set_bcd_threshold(cpu, bus, 0x862D, 0x85AC);
            decomp_set_bcd_threshold(cpu, bus, 0x8630, 0x85AC);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_bonus_thresholds";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 180;
            true
        }

        (0, 0x0C6F) => {
            let ix = cpu.ix;
            let hl = cpu.hl();
            decomp_set_bcd_threshold(cpu, bus, ix, hl);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_set_bcd_threshold";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 60;
            true
        }

        // =====================================================================
        // CPU 0 $0D54: `bcd_copy_3bytes`
        // Copies a 3-byte (6-digit) packed BCD score/threshold from (HL) to (DE).
        // =====================================================================
        (0, 0x0D54) => {
            let mut hl = cpu.hl();
            let mut de = cpu.de();
            for _ in 0..3 {
                let v = bus.read_byte(0, hl);
                bus.write_byte(0, de, v);
                bus.write_byte(0, 0x6830, v);
                cpu.a = v;
                hl = hl.wrapping_add(1);
                de = de.wrapping_add(1);
            }
            cpu.set_hl(hl);
            cpu.set_de(de);
            cpu.b = 0;
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_bcd_copy_3bytes";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 90;
            true
        }

        // =====================================================================
        // CPU 0 $0D60: `bcd_sub_compare_3bytes`
        // Compares 6-digit BCD player score against the bonus-life threshold
        // via 3-byte chained subtraction (`SBC A, (HL)`), setting Carry if
        // score < threshold or clearing Carry when 1UP threshold is reached!
        // =====================================================================
        (0, 0x0D60) => {
            let mut hl = cpu.de();
            let mut de = cpu.hl();
            cpu.and8(cpu.a); // AND A clears Carry
            for _ in 0..3 {
                cpu.a = bus.read_byte(0, de);
                let sub_val = bus.read_byte(0, hl);
                cpu.sub8(sub_val, true, true);
                hl = hl.wrapping_add(1);
                de = de.wrapping_add(1);
            }
            cpu.set_hl(de);
            cpu.set_de(hl);
            cpu.b = 0;
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_bcd_compare_1up";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 85;
            true
        }

        // =====================================================================
        // CPU 0 $0D6C: `award_1up_extra_life`
        // Awards an extra life to Player 1 ($840A) or Player 2 ($840B), updates
        // active HUD life count ($840C), and redraws the HUD life icons ($1845).
        // =====================================================================
        (0, 0x0D6C) => {
            let status_8400 = bus.read_byte(0, 0x8400);
            let lives_addr = if (status_8400 & 0x02) != 0 {
                0x840B
            } else {
                0x840A
            };
            let old_lives = bus.read_byte(0, lives_addr);
            let new_lives = cpu.inc8(old_lives);
            bus.write_byte(0, lives_addr, new_lives);
            bus.write_byte(0, 0x840C, new_lives);
            decomp_draw_player_lives_hud(cpu, bus);
            stats.hud_lives_calls += 1;
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_award_1up_extra_life";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 240;
            true
        }

        // =====================================================================
        // CPU 0 $0D84: `bcd_add_bonus_threshold`
        // Advances the player's next bonus-life threshold ($862D or $8630) by
        // adding the 6-digit BCD interval at $8633..$8635 using `ADC A, (HL); DAA`.
        // =====================================================================
        (0, 0x0D84) => {
            let status_8400 = bus.read_byte(0, 0x8400);
            let mut hl = if (status_8400 & 0x02) != 0 {
                0x8630u16
            } else {
                0x862Du16
            };
            let mut de = 0x8633u16;
            cpu.and8(cpu.a); // AND A clears Carry
            for _ in 0..3 {
                cpu.a = bus.read_byte(0, de);
                let cur = bus.read_byte(0, hl);
                cpu.add8(cur, true);
                cpu.daa();
                bus.write_byte(0, hl, cpu.a);
                de = de.wrapping_add(1);
                hl = hl.wrapping_add(1);
            }
            cpu.set_hl(hl);
            cpu.set_de(de);
            cpu.b = 0;
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_bcd_add_threshold";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 95;
            true
        }

        // =====================================================================
        // CPU 0 $1226: `bcd_add_score_digits`
        // Adds `A` (in BCD) to the 2-byte packed BCD counter at `(HL)` and `(HL-1)`.
        // =====================================================================
        (0, 0x1226) => {
            let hl = cpu.hl();
            cpu.and8(cpu.a);
            let lo = bus.read_byte(0, hl);
            cpu.add8(lo, false);
            cpu.daa();
            bus.write_byte(0, hl, cpu.a);

            let hl_prev = hl.wrapping_sub(1);
            cpu.set_hl(hl_prev);
            cpu.a = bus.read_byte(0, hl_prev);
            cpu.add8(0, true);
            cpu.daa();
            bus.write_byte(0, hl_prev, cpu.a);

            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_bcd_add_score_digits";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 52;
            true
        }

        // =====================================================================
        // CPU 0 $1092 & $10D5: `check_harpoon_tile_blocked`
        // Tests whether the playfield tile at (HL) blocks Dig Dug's harpoon cable
        // ($02, $03, $08, $09, $8D, $7E, or $0C/$8C), returning C=1 if blocked
        // or C=0 if clear tunnel.
        // =====================================================================
        (0, 0x1092) => {
            let t = bus.read_byte(0, cpu.hl());
            if t == 0x02 || t == 0x03 || t == 0x8D || t == 0x7E {
                cpu.a = t;
                cpu.sub8(t, false, false);
                cpu.c = 1;
            } else {
                let t7 = t & 0x7F;
                cpu.a = t7;
                cpu.sub8(0x0C, false, false);
                cpu.c = if t7 == 0x0C { 1 } else { 0 };
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_harpoon_tile";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 48;
            true
        }

        (0, 0x10D5) => {
            let t = bus.read_byte(0, cpu.hl());
            if t == 0x08 || t == 0x09 || t == 0x8D || t == 0x7E {
                cpu.a = t;
                cpu.sub8(t, false, false);
                cpu.c = 1;
            } else {
                let t7 = t & 0x7F;
                cpu.a = t7;
                cpu.sub8(0x0C, false, false);
                cpu.c = if t7 == 0x0C { 1 } else { 0 };
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_harpoon_tile_v";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 48;
            true
        }

        // =====================================================================
        // CPU 0 $0F89: `clear_rock_timers`
        // =====================================================================
        (0, 0x0F89) => {
            for addr in 0x9882..0x988A {
                bus.write_byte(0, addr, 0x00);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            cpu.set_hl(0x988A);
            cpu.b = 0;
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_rock_timers";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 120;
            true
        }

        // =====================================================================
        // CPU 0 $177C: `init_dirt_grid`
        // Initializes the 32x28 underground dirt collision grid at $9000..$937F
        // with $7F (undug dirt) and border markers ($7E, $8D, $8E, $8F).
        // =====================================================================
        (0, 0x177C) => {
            for addr in 0x9000..0x9380u16 {
                let col_mod = (addr & 0x1F) as u8;
                let tile_val = if col_mod < 3 {
                    if col_mod == 1 { 0x8D } else { 0x7E }
                } else {
                    0x7F
                };
                bus.write_byte(0, addr, tile_val);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            bus.write_byte(0, 0x9001, 0x8F);
            bus.write_byte(0, 0x9361, 0x8E);
            cpu.set_hl(0x9380);
            cpu.a = 0x8E;
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_dirt_grid";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 400;
            true
        }

        // =====================================================================
        // CPU 0 $17AE: `init_playfield_vram`
        // Initializes Playfield Tilemap VRAM ($8040..$83BF) with sky ($8D/$7E)
        // and dirt ($7F), sets corners ($8F/$8E), and clears HUD ($8000..$803F).
        // =====================================================================
        (0, 0x17AE) => {
            for addr in 0x8040..0x83C0u16 {
                let col_mod = (addr & 0x1F) as u8;
                let tile_val = if col_mod < 3 {
                    if col_mod == 1 { 0x8D } else { 0x7E }
                } else {
                    0x7F
                };
                bus.write_byte(0, addr, tile_val);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            bus.write_byte(0, 0x8041, 0x8F);
            bus.write_byte(0, 0x83C0, 0x8E);
            bus.write_byte(0, 0x83A1, 0x8F);
            for addr in 0x8000..0x8040u16 {
                bus.write_byte(0, 0x6830, 0x8F);
                bus.write_byte(0, addr, 0x0C);
            }
            cpu.set_hl(0x8040);
            cpu.b = 0;
            cpu.a = 0x8F;
            cpu.sub8(0x8F, false, false);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_playfield_vram";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 450;
            true
        }

        // =====================================================================
        // CPU 0 $1845: `draw_player_lives_hud`
        // Clears the bottom-left HUD area ($800C..$801F, $802C..$803F) and draws
        // up to 9 2x2 Dig Dug life helmet icons based on $840C.
        // =====================================================================
        (0, 0x1845) => {
            decomp_draw_player_lives_hud(cpu, bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_player_lives_hud";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 210;
            true
        }

        // =====================================================================
        // CPU 0 $187F & $188D: `clear_actor_work_ram`
        // Clears 128-byte actor/monster state tables at $84A5..$8524 or $8525..$85A4.
        // =====================================================================
        (0, 0x187F) => {
            for addr in 0x84A5..0x8525u16 {
                bus.write_byte(0, addr, 0x00);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            cpu.set_hl(0x8525);
            cpu.b = 0;
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_actor_ram_p1";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 260;
            true
        }

        (0, 0x188D) => {
            for addr in 0x8525..0x85A5u16 {
                bus.write_byte(0, addr, 0x00);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            cpu.set_hl(0x85A5);
            cpu.b = 0;
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_actor_ram_p2";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 260;
            true
        }

        // =====================================================================
        // CPU 0 $19A3: `advance_round_and_draw_hud` (Namco Rev 2 address)
        // Increments Player 1 ($840D) or Player 2 ($840E) round number,
        // optionally applies the Round 256 -> 156 Kill Screen fix in pure Rust,
        // converts the round number to 2-digit BCD, and writes the "ROUND" label
        // and digits into Foreground Tilemap VRAM ($8023, $8024, $8002..$8008).
        // =====================================================================
        (0, 0x19A3) => {
            let status_8400 = bus.read_byte(0, 0x8400);
            let is_player2 = (status_8400 & 0x02) != 0;
            let round_addr = if is_player2 { 0x840E } else { 0x840D };

            let mut new_round = bus.read_byte(0, round_addr).wrapping_add(1);
            // Round 256 Kill-Screen Fix: roll Round 256 (0) back to Round 156 (0x9C),
            // activating Namco's dormant routine at $0018!
            if new_round == 0 {
                new_round = 156;
            }
            bus.write_byte(0, round_addr, new_round);
            bus.write_byte(0, 0x840F, new_round);
            bus.write_byte(0, 0x8410, new_round);

            let demo_8657 = bus.read_byte(0, 0x8657);
            let bcd_target_addr = if (demo_8657 & 0x02) != 0 {
                let r = bus.read_byte(0, 0x9A05);
                bus.write_byte(0, 0x9A06, r);
                0x9A06
            } else {
                0x840F
            };

            let raw_round = bus.read_byte(0, bcd_target_addr);
            // Modulo 100 for display so rounds 100..255 display their lower 2 digits cleanly
            let display_val = raw_round % 100;
            let (bcd_lo, bcd_hi) = decomp_cpu0_binary_to_bcd(display_val);
            bus.write_byte(0, bcd_target_addr, bcd_lo);
            bus.write_byte(0, bcd_target_addr + 1, bcd_hi);
            cpu.d = bcd_lo;
            cpu.e = 0x02;
            cpu.c = 0x00;

            // Write ones digit tile to VRAM $8023
            let ones_tile = (bcd_lo & 0x0F) + 0x10;
            bus.write_byte(0, 0x8023, ones_tile);

            // Write tens digit tile to VRAM $8024 (or $7F space if leading zero)
            let tens_nibble = (bcd_lo >> 4) & 0x0F;
            let tens_tile = if tens_nibble == 0 {
                0x7F
            } else {
                tens_nibble + 0x10
            };
            bus.write_byte(0, 0x8024, tens_tile);

            // Copy the 7-character "ROUND" text string from ROM $3068 into VRAM $8008..$8002
            for i in 0..7u16 {
                let ch = roms.cpu0[(0x3068 + i) as usize];
                bus.write_byte(0, 0x8008 - i, ch);
            }
            cpu.ix = 0x306F;
            cpu.b = 0;

            // Fall through into `sub_19F7` (sync and clamp round index to 0..15)
            decomp_sync_and_clamp_round(cpu, bus);

            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_advance_round_and_hud";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 260;
            true
        }

        // =====================================================================
        // CPU 0 $19F7: `sync_and_clamp_round` (Namco Rev 2 address)
        // Copies P1/P2 round into $840F/$8410, writes stage layout index $8627,
        // and clamps $840F/$8410 to $0F (15).
        // =====================================================================
        (0, 0x19F7) => {
            decomp_sync_and_clamp_round(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_sync_and_clamp_round";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 90;
            true
        }

        // =====================================================================
        // ENGINE 1: HARPOON HITBOXES, CABLE RETRACTION, INFLATION & POPPING
        // =====================================================================

        // CPU 0 $0FF0, $0FF9, $1002 & $1209: `retract_harpoon_cable_segments`
        // Advances or retracts the 3 harpoon cable sprite segments ($9882..$9887)
        // by +/- 2 pixels along the firing axis based on harpoon timer ($8642).
        (0, 0x0FF0) => {
            decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9882, 0x9883);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_harpoon_seg0_step";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 58;
            true
        }

        (0, 0x0FF9) => {
            decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9884, 0x9885);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_harpoon_seg1_step";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 58;
            true
        }

        (0, 0x1002) => {
            decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_harpoon_tip_step";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 58;
            true
        }

        (0, 0x1209) => {
            let hl = bus.read_word(0, 0x8642);
            cpu.set_hl(hl);
            let a = bus.read_byte(0, hl);
            cpu.and8(a);
            if a != 0 {
                let v1 = cpu.dec8(a);
                let v2 = cpu.dec8(v1);
                bus.write_byte(0, hl, v2);
                cpu.a = v2;
                cpu.sub8(0x22, false, false);
                if v2 >= 0x22 {
                    decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
                } else {
                    cpu.a = v2;
                    cpu.sub8(0x12, false, false);
                    if v2 >= 0x12 {
                        decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9884, 0x9885);
                        decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
                    } else {
                        decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9882, 0x9883);
                        decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9884, 0x9885);
                        decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
                    }
                }
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_retract_harpoon_cable";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 140;
            true
        }

        // CPU 0 $1177: `check_harpoon_hit_monster_16x16`
        // Tests 16x16 normal monster (E=X, D=Y) against harpoon tip (L=X, H=Y)
        // along firing direction C = ($87E4), setting Carry (FLAG_C) on hit!
        (0, 0x1177) => {
            if (cpu.c & 0x02) == 0 {
                // Vertical harpoon shot
                cpu.a = cpu.l;
                cpu.sub8(cpu.e, false, true);
                cpu.add8(0x08, false);
                cpu.sub8(0x0C, false, false);
                if (cpu.f & FLAG_C) != 0 {
                    cpu.a = cpu.h;
                    cpu.sub8(cpu.d, false, true);
                    if (cpu.c & 0x04) != 0 {
                        cpu.a = !cpu.a;
                    }
                    cpu.sub8(0x02, false, true);
                    cpu.sub8(0x0C, false, false);
                }
            } else {
                // Horizontal harpoon shot
                cpu.a = cpu.h;
                cpu.sub8(cpu.d, false, true);
                cpu.add8(0x08, false);
                cpu.sub8(0x0C, false, false);
                if (cpu.f & FLAG_C) != 0 {
                    cpu.a = cpu.l;
                    cpu.sub8(cpu.e, false, true);
                    if (cpu.c & 0x04) == 0 {
                        cpu.a = !cpu.a;
                    }
                    cpu.sub8(0x02, false, true);
                    cpu.sub8(0x0C, false, false);
                }
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_harpoon_hitbox_16x16";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 68;
            true
        }

        // CPU 0 $11A1: `check_harpoon_hit_monster_32x32`
        // Tests 32x32 already-inflated monster (sprite >= $40) against harpoon tip,
        // setting Carry (FLAG_C) on hit!
        (0, 0x11A1) => {
            if (cpu.c & 0x02) == 0 {
                cpu.a = cpu.l;
                cpu.sub8(cpu.e, false, true);
                cpu.add8(0x03, false);
                cpu.sub8(0x12, false, false);
                if (cpu.f & FLAG_C) != 0 {
                    cpu.a = cpu.h;
                    cpu.sub8(cpu.d, false, true);
                    if (cpu.c & 0x04) != 0 {
                        cpu.add8(0x10, false);
                    }
                    cpu.sub8(0x03, false, true);
                    cpu.sub8(0x12, false, false);
                }
            } else {
                cpu.a = cpu.h;
                cpu.sub8(cpu.d, false, true);
                cpu.add8(0x07, false);
                cpu.sub8(0x12, false, false);
                if (cpu.f & FLAG_C) != 0 {
                    cpu.a = cpu.l;
                    cpu.sub8(cpu.e, false, true);
                    if (cpu.c & 0x04) == 0 {
                        cpu.add8(0x10, false);
                    }
                    cpu.sub8(0x07, false, true);
                    cpu.sub8(0x12, false, false);
                }
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_harpoon_hitbox_32x32";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 72;
            true
        }

        // CPU 0 $1251: `setup_monster_pop_score_sprite`
        // Prepares floating score popup sprite attributes ($987E/$997E, $9876/$9976)
        // when a Pooka or Fygar is popped!
        (0, 0x1251) => {
            bus.write_byte(0, 0x987E, 0x47);
            bus.write_byte(0, 0x987F, 0x0A);
            bus.write_byte(0, 0x997E, 0x00);
            cpu.ix = 0x9976;
            cpu.set_hl(0x9876);
            cpu.set_de(0x9877);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_pop_score_sprite";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 64;
            true
        }

        // CPU 0 $126C: `calc_monster_depth_stratum`
        // Computes the underground depth stratum (0..3) from a popped monster's
        // Y coordinate (HL+5) to award 200/300/400/500 pts (or 2x horizontal Fygar!).
        (0, 0x126C) => {
            let hl = cpu.hl().wrapping_add(5);
            cpu.set_de(0x0005);
            cpu.set_hl(hl);
            let status = bus.read_byte(0, 0x8400);
            let mut y = bus.read_byte(0, hl);
            if (status & 0x06) == 0x06 {
                y = y.wrapping_sub(0x10);
            }
            if y >= 0x20 {
                y = y.wrapping_sub(0x08);
            }
            let stratum = (y >> 6) & 0x03;
            cpu.a = stratum;
            cpu.and8(stratum);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_calc_monster_stratum";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 70;
            true
        }

        // CPU 0 $268E, $26F8, $279B, $27A9, $2894: `update_monster_inflation_and_walk_sprites`
        // Complete per-monster state machine for walking ($20/$21 Pooka, $24/$25 Fygar),
        // ghosting eyes ($28/$29 Pooka, $2A/$2B Fygar), crushed ($22/$26), and pump
        // inflation/deflation stages 1..4 ($23/$80/$81/$82 Pooka, $27/$83/$84/$85 Fygar).
        (0, 0x268E) => {
            decomp_update_monster_sprites(cpu, bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_monster_sprites";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 145;
            true
        }

        (0, 0x26F8) => {
            let hl = cpu.hl();
            let state0 = bus.read_byte(0, hl);
            cpu.set_de(0x0006);
            if (state0 & 0x01) == 0 {
                bus.write_byte(0, hl.wrapping_add(6), 0x22);
                bus.write_byte(0, hl.wrapping_add(7), 0x01);
            } else {
                bus.write_byte(0, hl.wrapping_add(6), 0x26);
                bus.write_byte(0, hl.wrapping_add(7), 0x02);
            }
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_set_crushed_sprite";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 65;
            true
        }

        (0, 0x279B) => {
            decomp_shift_monster_x(cpu, bus, -8);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_shift_monster_x_m8";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 55;
            true
        }

        (0, 0x27A9) => {
            decomp_shift_monster_x(cpu, bus, 8);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_shift_monster_x_p8";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 55;
            true
        }

        (0, 0x2894) => {
            let hl_spr = cpu.hl().wrapping_add(cpu.de());
            bus.write_word(0, 0x85F6, hl_spr);
            let hl_anim = hl_spr.wrapping_add(4);
            cpu.set_de(0x0004);
            cpu.set_hl(hl_anim);
            let old_anim = bus.read_byte(0, hl_anim);
            let new_anim = cpu.inc8(old_anim);
            bus.write_byte(0, hl_anim, new_anim);
            cpu.a = new_anim;
            cpu.and8(0x0F);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_step_monster_walk_anim";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 55;
            true
        }

        // =====================================================================
        // ENGINE 2: FALLING ROCK EXCAVATION, TUNNEL CAPS & SPRITE MAPPING
        // =====================================================================

        // CPU 0 $1C10, $1C16, $1C5A, $1C87: `pixel_xy_to_4tile_quad`
        // Converts (D=X, E=Y) pixel coordinates into the 32x32 VRAM/Dirt tilemap
        // address HL and populates the 4 surrounding tile pointers (IX+0..7).
        (0, 0x1C10) => {
            decomp_populate_4tile_quad(cpu, bus, 0, 0x85FA);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_xy_to_4tile_quad_85fa";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 165;
            true
        }

        (0, 0x1C16) => {
            decomp_populate_4tile_quad(cpu, bus, 0, 0x8602);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_xy_to_4tile_quad_8602";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 165;
            true
        }

        (0, 0x1C5A) => {
            decomp_pixel_xy_to_tile_addr(cpu);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_pixel_xy_to_tile_addr";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 95;
            true
        }

        (0, 0x1C87) => {
            let hl = cpu.hl();
            let de = cpu.add16(hl, 0xFFE0);
            cpu.set_de(de);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_tile_next_col";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 32;
            true
        }

        // CPU 0 $22CB & $22FF: `check_rock_underneath_excavation_and_cap_tunnel`
        // Computes the two tile addresses directly beneath a rock ($85EA, $85EC)
        // and rounds off the tunnel cap tiles when both supporting tiles are dug out!
        (0, 0x22CB) => {
            let saved_bc = cpu.bc();
            let saved_hl = cpu.hl();
            let saved_de = cpu.de();

            decomp_populate_4tile_quad(cpu, bus, 0, 0x85FA);
            let tile_bl = bus.read_word(0, 0x85FE);
            let tile_br = bus.read_word(0, 0x8600);
            let below_l = tile_bl.wrapping_add(1);
            let below_r = tile_br.wrapping_add(1);
            bus.write_word(0, 0x85EA, below_l);
            bus.write_word(0, 0x85EC, below_r);

            let tl_val = bus.read_byte(0, below_l);
            cpu.a = tl_val;
            cpu.sub8(0x7F, false, false);
            if tl_val != 0x7F {
                let tr_val = bus.read_byte(0, below_r);
                cpu.a = tr_val;
                cpu.sub8(0x7F, false, false);
                if tr_val != 0x7F {
                    decomp_cap_rock_tunnel_tile(cpu, bus, tile_bl);
                    decomp_cap_rock_tunnel_tile(cpu, bus, tile_br);
                }
            }

            cpu.set_de(saved_de);
            cpu.set_hl(saved_hl);
            cpu.set_bc(saved_bc);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_rock_excavation";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 240;
            true
        }

        (0, 0x22FF) => {
            let hl = cpu.hl();
            decomp_cap_rock_tunnel_tile(cpu, bus, hl);
            cpu.set_hl(hl);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_cap_rock_tunnel_tile";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 68;
            true
        }

        // CPU 0 $290C: `rock_ptr_to_sprite_slot`
        // Maps a rock struct pointer HL ($846D, $8475, ...) to its hardware sprite
        // code address in $983C..$9843.
        (0, 0x290C) => {
            let diff = cpu.hl().wrapping_sub(0x8469);
            let idx = ((diff & 0xFF) as u8) >> 2;
            cpu.a = idx;
            cpu.set_de(idx as u16);
            cpu.set_hl(0x983C + (idx as u16));
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_rock_ptr_to_sprite";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 56;
            true
        }

        // =====================================================================
        // ENGINE 3: POOKA/FYGAR GHOSTING (`MECHA-RU`) & CPU1 DIRT-CARVING AI
        // =====================================================================

        // CPU 0 $243F, $2521, $28A1, $28B7, $28DB: `check_ghost_monster_tunnel_reentry`
        // Handles Pooka/Fygar ghosting (`mecha-ru`) grid alignment and 4-corner
        // open-tunnel detection so ghosting monsters materialize inside tunnels!
        (0, 0x243F) => {
            let hl = bus.read_word(0, 0x85F6);
            let b1 = bus.read_byte(0, hl.wrapping_add(1)) | 0x02;
            bus.write_byte(0, hl.wrapping_add(1), b1);
            cpu.set_hl(hl);
            cpu.set_de(0x0005);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_ghost_set_vert_axis";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 48;
            true
        }

        (0, 0x2521) => {
            let tile = bus.read_byte(0, cpu.hl()) & 0x7F;
            cpu.a = tile;
            cpu.and8(tile);
            if tile < 0x10 || tile == 0x7E {
                cpu.c = cpu.dec8(cpu.c);
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_tile_is_tunnel";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 42;
            true
        }

        (0, 0x28A1) => {
            let x = bus.read_byte(0, cpu.hl().wrapping_add(3));
            if (x & 0x0F) == 0x01 {
                cpu.b |= 0x01;
            } else {
                cpu.b &= !0x01;
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_monster_x_grid";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 54;
            true
        }

        (0, 0x28B7) => {
            let y = bus.read_byte(0, cpu.hl().wrapping_add(4));
            cpu.c = y;
            cpu.d = y & 0x0F;
            let aligned = if y >= 0x20 {
                (y & 0x0F) == 0
            } else {
                y == 0x08
            };
            if aligned {
                cpu.b |= 0x01;
            } else {
                cpu.b &= !0x01;
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_monster_y_grid";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 64;
            true
        }

        (0, 0x28DB) => {
            let base = bus.read_word(0, 0x85F6);
            cpu.d = bus.read_byte(0, base.wrapping_add(4));
            cpu.e = bus.read_byte(0, base.wrapping_add(5));
            decomp_populate_4tile_quad(cpu, bus, 0, 0x85FA);

            let mut c = 4u8;
            for ptr_addr in [0x85FAu16, 0x85FC, 0x85FE, 0x8600] {
                let tile_ptr = bus.read_word(0, ptr_addr);
                cpu.set_hl(tile_ptr);
                let tile = bus.read_byte(0, tile_ptr) & 0x7F;
                if tile < 0x10 || tile == 0x7E {
                    c = c.wrapping_sub(1);
                }
            }
            cpu.c = c;
            cpu.a = c;
            cpu.and8(c);
            if c == 0 {
                cpu.b |= 0x01;
            } else {
                cpu.b &= !0x01;
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_ghost_reentry";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 220;
            true
        }

        // CPU 1 $1353, $1393, $13CB, $13FD, $1404, $157B, $15CB, $15D3:
        // `cpu1_detect_digging_and_carve_dirt`
        // Inspects Dig Dug's 4 surrounding tile pointers ($85D6..$85DC) for undug
        // dirt ($7F), toggles bit 4 of $8401 (digging state / walking melody pitch),
        // and sets bit 2 of $8656 when carving fresh dirt!
        (1, 0x1353) => {
            let mut b = 0u8;
            for ptr_addr in [0x85D6u16, 0x85D8, 0x85DA, 0x85DC] {
                let tile_ptr = bus.read_word(1, ptr_addr);
                if bus.read_byte(1, tile_ptr) == 0x7F {
                    b = b.wrapping_add(1);
                }
            }
            cpu.b = b;
            cpu.set_hl(0x8401);
            let mut s8401 = bus.read_byte(1, 0x8401) & !0x10;
            if b == 0 {
                s8401 |= 0x10;
            }
            bus.write_byte(1, 0x8401, s8401);
            cpu.a = b;
            cpu.and8(b);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_check_digging_4tiles";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 140;
            true
        }

        (1, 0x1393) => {
            let dir = bus.read_byte(1, 0x85AE) & 0x0F;
            if dir != 0x00 && dir != 0x06 {
                cpu.d = cpu.d.wrapping_add(0x06);
                cpu.e = cpu.e.wrapping_add(0x06);
            }
            decomp_populate_4tile_quad(cpu, bus, 1, 0x85D6);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_digdug_xy_to_4tiles";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 175;
            true
        }

        (1, 0x15D3) => {
            cpu.d = bus.read_byte(1, 0x98A2);
            cpu.e = bus.read_byte(1, 0x98A3);
            let dir = bus.read_byte(1, 0x85AF) & 0x0F;
            if dir != 0x00 && dir != 0x06 {
                cpu.d = cpu.d.wrapping_add(0x06);
                cpu.e = cpu.e.wrapping_add(0x06);
            }
            decomp_populate_4tile_quad(cpu, bus, 1, 0x85D6);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_turn_xy_to_4tiles";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 185;
            true
        }

        (1, 0x13CB) => {
            decomp_pixel_xy_to_tile_addr(cpu);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_pixel_xy_to_tile_addr";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 95;
            true
        }

        (1, 0x13FD) => {
            let hl = cpu.hl();
            let de = cpu.add16(hl, 0xFFE0);
            cpu.set_de(de);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_tile_next_col";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 32;
            true
        }

        (1, 0x1404) => {
            let t = bus.read_byte(1, cpu.hl());
            cpu.a = t;
            cpu.sub8(0x7F, false, false);
            if t == 0x7F {
                cpu.b = cpu.inc8(cpu.b);
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_count_undug_dirt";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 34;
            true
        }

        (1, 0x157B) => {
            let tile = cpu.a;
            let saved_f = cpu.f;
            let s8401 = bus.read_byte(1, 0x8401);
            let is_reverse = if (s8401 & 0x80) != 0 {
                true
            } else {
                let opp = bus.read_byte(1, 0x85AE).wrapping_add(4) & 0x07;
                cpu.b = opp;
                let prev = bus.read_byte(1, 0x85AF) & 0x07;
                prev == opp
            };

            cpu.a = tile;
            cpu.f = saved_f;
            cpu.sub8(0x7F, false, false);
            if tile == 0x7F {
                let s8656 = bus.read_byte(1, 0x8656) | 0x04;
                bus.write_byte(1, 0x8656, s8656);
                cpu.a = tile | 0x01;
            } else if is_reverse {
                cpu.a = tile & !0x01;
            } else if tile == 0x00 {
                cpu.and8(0x00);
                cpu.a = tile | 0x01;
            } else if matches!(tile, 0x01 | 0x04 | 0x05 | 0x06 | 0x07 | 0x0A | 0x0B) {
                cpu.sub8(tile, false, false);
                cpu.a = tile | 0x01;
            } else {
                cpu.sub8(0x0B, false, false);
                cpu.a = tile & !0x01;
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_carve_dirt_corner";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 85;
            true
        }

        (1, 0x15CB) => {
            let s8656 = bus.read_byte(1, 0x8656) | 0x04;
            bus.write_byte(1, 0x8656, s8656);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_mark_dirt_dug_score";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 38;
            true
        }

        // =====================================================================
        // CPU 1 $1B9F: `calc_monster_difficulty` (Namco Rev 2 address)
        // Computes Monster Base Speed ($87BD) and 6 AI difficulty parameters
        // ($87A0..$87A5) from DIP switch difficulty ($898D), round step ($8410),
        // and current round ($840D/$840E), including the Rev 2 overflow clamp!
        // =====================================================================
        (1, 0x1B9F) => {
            let demo_8657 = bus.read_byte(1, 0x8657);
            let diff_bank = if (demo_8657 & 0x02) != 0 {
                2u16
            } else {
                (bus.read_byte(1, 0x898D) & 0x03) as u16
            };

            let round_step = bus.read_byte(1, 0x8410).wrapping_sub(1) as u16;
            let de_offset = ((diff_bank << 4) & 0xFF).wrapping_add(round_step & 0xFF);

            // Base monster speed table at CPU1 ROM $1BFE
            let base_speed = roms.cpu1[(0x1BFE + de_offset) as usize];
            bus.write_byte(1, 0x87BD, base_speed);

            // Add (round - 30) speed bonus for rounds >= 30, clamping at 0xFF (Rev 2 fix!)
            let status_8400 = bus.read_byte(1, 0x8400);
            let round_num = if (status_8400 & 0x02) != 0 {
                bus.read_byte(1, 0x840E)
            } else {
                bus.read_byte(1, 0x840D)
            };

            cpu.a = round_num;
            cpu.sub8(0x1E, false, true);
            if round_num >= 30 {
                cpu.add8(base_speed, false);
                let final_speed = if (cpu.f & FLAG_C) != 0 { 0xFF } else { cpu.a };
                bus.write_byte(1, 0x87BD, final_speed);
            }

            // Copy 6 AI difficulty parameters from ROM table $1C3E (stride $40) into $87A0..$87A5
            let mut last_param = 0u8;
            for i in 0..6u16 {
                let table_addr = 0x1C3E + i * 0x40 + de_offset;
                last_param = roms.cpu1[(table_addr & 0x1FFF) as usize];
                bus.write_byte(1, 0x87A0 + i, last_param);
            }

            cpu.a = last_param;
            cpu.f &= !(FLAG_N | FLAG_C);
            cpu.set_hl(0x1DBE);
            cpu.set_de(de_offset);
            cpu.ix = 0x87A6;
            cpu.b = 0;
            stats.monster_speed_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_calc_monster_difficulty";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 220;
            true
        }

        // =====================================================================
        // CPU 1 PLAYER MOVEMENT, SPRITE POSRAM BLIT, TIMERS, FLOWERS & VEGGIES
        // =====================================================================

        // CPU 1 $0315: `update_walking_sound_flag`
        (1, 0x0315) => {
            cpu.a = bus.read_byte(1, 0x8646);
            cpu.and8(0x03);
            if cpu.a != 0 {
                cpu.xor8(cpu.a);
                bus.write_byte(1, 0x9A91, 0x00);
            } else {
                cpu.a = bus.read_byte(1, 0x8401);
                cpu.cb_op(0x7F, cpu.a);
                if (cpu.a & 0x80) != 0 {
                    cpu.xor8(cpu.a);
                    bus.write_byte(1, 0x9A91, 0x00);
                } else {
                    cpu.a = bus.read_byte(1, 0x8403);
                    cpu.cb_op(0x57, cpu.a);
                    if (cpu.a & 0x04) != 0 {
                        cpu.xor8(cpu.a);
                        bus.write_byte(1, 0x9A91, 0x00);
                    } else {
                        cpu.a = bus.read_byte(1, 0x8785);
                        cpu.cb_op(0x7F, cpu.a);
                        let joy = if (cpu.a & 0x80) != 0 {
                            bus.read_byte(1, 0x8970)
                        } else {
                            cpu.a = bus.read_byte(1, 0x8657);
                            cpu.cb_op(0x4F, cpu.a);
                            if (cpu.a & 0x02) != 0 {
                                bus.read_byte(1, 0x8970)
                            } else {
                                bus.read_byte(1, 0x85B0)
                            }
                        };
                        cpu.a = joy;
                        cpu.and8(0x0F);
                        cpu.sub8(0x08, false, false);
                        if cpu.a == 0x08 {
                            cpu.xor8(cpu.a);
                            bus.write_byte(1, 0x9A91, 0x00);
                        } else {
                            cpu.a = 0x01;
                            bus.write_byte(1, 0x9A91, 0x01);
                        }
                    }
                }
            }
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_update_walking_sound";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 85;
            true
        }

        // CPU 1 $0351: `draw_round_flower_indicators`
        (1, 0x0351) => {
            decomp_cpu1_draw_round_flower_indicators(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_draw_round_flowers";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 380;
            true
        }

        // CPU 1 $040E: `set_digdug_pump_sprites`
        (1, 0x040E) => {
            bus.write_byte(1, 0x9820, cpu.e);
            bus.write_byte(1, 0x9822, cpu.d);
            cpu.set_hl(0x9822);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_set_pump_sprites";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 46;
            true
        }

        // CPU 1 $0458: `copy_3_flower_sprites`
        (1, 0x0458) => {
            decomp_cpu1_copy_3_flower_sprites(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_copy_3_flower_sprites";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 160;
            true
        }

        // CPU 1 $046F: `pack_bcd_pair`
        (1, 0x046F) => {
            let de = cpu.de();
            let lo_nibble = bus.read_byte(1, de) >> 4;
            cpu.b = lo_nibble;
            let de_next = de.wrapping_add(1);
            cpu.set_de(de_next);
            let hi_nibble = bus.read_byte(1, de_next) << 4;
            cpu.a = hi_nibble;
            cpu.or8(lo_nibble);
            let hl = cpu.hl();
            bus.write_byte(1, hl, cpu.a);
            cpu.set_hl(hl.wrapping_sub(1));
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_pack_bcd_pair";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 115;
            true
        }

        // CPU 1 $069A: `fill_4_words_stride2`
        (1, 0x069A) => {
            let mut hl = cpu.hl();
            for _ in 0..4 {
                bus.write_byte(1, hl, cpu.a);
                hl = hl.wrapping_add(2);
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_fill_4_words_stride2";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 115;
            true
        }

        // CPU 1 $0E2E: `add_c_to_10_sprites_y`
        (1, 0x0E2E) => {
            let mut hl = 0x98C1u16;
            for _ in 0..10 {
                cpu.a = bus.read_byte(1, hl);
                cpu.add8(cpu.c, false);
                bus.write_byte(1, hl, cpu.a);
                hl = hl.wrapping_add(2);
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_add_c_to_10_sprites_y";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 320;
            true
        }

        // CPU 1 $0E3B: `write_vertical_string`
        (1, 0x0E3B) => {
            std::mem::swap(&mut cpu.d, &mut cpu.h);
            std::mem::swap(&mut cpu.e, &mut cpu.l);
            let de = cpu.de();
            cpu.push(bus, de);
            cpu.ix = cpu.pop(bus);
            cpu.set_de(0x0020);
            loop {
                let ch = bus.read_byte(1, cpu.ix);
                cpu.a = ch;
                cpu.ix = cpu.ix.wrapping_add(1);
                let hl = cpu.hl();
                bus.write_byte(1, hl, ch);
                let next_hl = cpu.add16(hl, 0x0020);
                cpu.set_hl(next_hl);
                cpu.b = cpu.b.wrapping_sub(1);
                if cpu.b == 0 {
                    break;
                }
            }
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_write_vertical_string";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 280;
            true
        }

        // CPU 1 $0E4C: `blit_sprite_posram`
        (1, 0x0E4C) => {
            cpu.set_hl(0x9880);
            cpu.set_de(0x9380);
            cpu.b = 0x40;
            cpu.a = bus.read_byte(1, 0x87F0);
            cpu.cb_op(0x5F, cpu.a);
            if (cpu.a & 0x08) == 0 {
                cpu.a = bus.read_byte(1, 0x8400);
                cpu.cb_op(0x4F, cpu.a);
                let is_cocktail_p2 = if (cpu.a & 0x02) != 0 {
                    cpu.cb_op(0x57, cpu.a);
                    (cpu.a & 0x04) != 0
                } else {
                    false
                };
                let mut hl = 0x9880u16;
                let mut de = 0x9380u16;
                if !is_cocktail_p2 {
                    for _ in 0..0x40 {
                        let x = bus.read_byte(1, hl);
                        bus.write_byte(1, de, x);
                        hl = hl.wrapping_add(1);
                        de = de.wrapping_add(1);
                        cpu.a = bus.read_byte(1, hl);
                        cpu.add8(0x37, false);
                        bus.write_byte(1, de, cpu.a);
                        hl = hl.wrapping_add(1);
                        de = de.wrapping_add(1);
                    }
                } else {
                    for _ in 0..0x40 {
                        let x = bus.read_byte(1, hl);
                        cpu.a = 0xF2;
                        cpu.sub8(x, false, true);
                        bus.write_byte(1, de, cpu.a);
                        de = de.wrapping_add(1);
                        hl = hl.wrapping_add(1);
                        let y = bus.read_byte(1, hl);
                        cpu.a = 0x27;
                        cpu.sub8(y, false, true);
                        bus.write_byte(1, de, cpu.a);
                        hl = hl.wrapping_add(1);
                        de = de.wrapping_add(1);
                    }
                }
                cpu.set_hl(hl);
                cpu.set_de(de);
                cpu.b = 0;
            }
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_blit_sprite_posram";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 1200;
            true
        }

        // CPU 1 $1000: `decode_player_or_demo_dir`
        (1, 0x1000) => {
            cpu.a = bus.read_byte(1, 0x8785);
            cpu.cb_op(0x7F, cpu.a);
            let src_hl = if (cpu.a & 0x80) != 0 {
                0x8970u16
            } else {
                cpu.a = bus.read_byte(1, 0x8657);
                cpu.cb_op(0x4F, cpu.a);
                if (cpu.a & 0x02) != 0 {
                    0x8970u16
                } else {
                    0x85B0u16
                }
            };
            cpu.set_hl(src_hl);
            let raw_in = bus.read_byte(1, src_hl);
            cpu.a = raw_in;
            cpu.and8(0x0E);
            cpu.push(bus, src_hl);
            cpu.a = raw_in;
            cpu.and8(0x0F);
            cpu.sub8(0x08, false, false);
            if cpu.a == 0x08 {
                let s = bus.read_byte(1, 0x85AE) | 0x40;
                bus.write_byte(1, 0x85AE, s);
                let restored = cpu.pop(bus);
                cpu.set_hl(restored);
            } else {
                let prev = bus.read_byte(1, 0x85AE) & 0x3F;
                bus.write_byte(1, 0x85AF, prev);
                let restored = cpu.pop(bus);
                let cur = bus.read_byte(1, restored) & 0x3F;
                bus.write_byte(1, 0x85AE, cur);
                let cur_dir = cur & 0x0F;
                cpu.a = prev & 0x0F;
                cpu.sub8(cur_dir, false, false);
                cpu.set_hl(0x85AE);
                let final_85ae = if (prev & 0x0F) == cur_dir {
                    cur & !0x80
                } else {
                    cur | 0x80
                };
                bus.write_byte(1, 0x85AE, final_85ae);
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_decode_player_dir";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 110;
            true
        }

        // CPU 1 $104E: `update_digdug_sprite_and_flip`
        (1, 0x104E) => {
            decomp_cpu1_update_digdug_sprite_and_flip(cpu, bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_update_digdug_sprite";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 95;
            true
        }

        // CPU 1 $111F: `set_digdug_speed`
        (1, 0x111F) => {
            let s8401 = bus.read_byte(1, 0x8401);
            cpu.cb_op(0x67, s8401);
            cpu.a = if (s8401 & 0x10) != 0 { 0x50 } else { 0x40 };
            bus.write_byte(1, 0x85B1, cpu.a);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_set_digdug_speed";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 48;
            true
        }

        // CPU 1 $1136: `step_digdug_2px`
        (1, 0x1136) => {
            let dir = bus.read_byte(1, 0x85AE);
            cpu.a = dir;
            cpu.set_hl(0x98A3);
            cpu.and8(0x0F);
            let mut step_mode = 0i8;
            if cpu.a == 0x00 {
                step_mode = -1;
            } else {
                cpu.sub8(0x04, false, false);
                if cpu.a == 0x04 {
                    step_mode = 1;
                } else {
                    cpu.set_hl(0x98A2);
                    cpu.sub8(0x02, false, false);
                    if cpu.a == 0x02 {
                        step_mode = 1;
                    } else {
                        cpu.sub8(0x06, false, false);
                        if cpu.a == 0x06 {
                            step_mode = -1;
                        }
                    }
                }
            }
            if step_mode == 1 {
                let hl = cpu.hl();
                let v1 = cpu.inc8(bus.read_byte(1, hl));
                let v2 = cpu.inc8(v1);
                bus.write_byte(1, hl, v2);
                cpu.e = bus.read_byte(1, 0x98A3);
                cpu.d = bus.read_byte(1, 0x98A2);
                cpu.a = cpu.d;
                decomp_cpu1_check_playfield_bounds(cpu);
                cpu.cb_op(0x49, cpu.c);
                if (cpu.c & 0x02) != 0 {
                    let v3 = cpu.dec8(bus.read_byte(1, hl));
                    let v4 = cpu.dec8(v3);
                    bus.write_byte(1, hl, v4);
                }
            } else if step_mode == -1 {
                let hl = cpu.hl();
                let v1 = cpu.dec8(bus.read_byte(1, hl));
                let v2 = cpu.dec8(v1);
                bus.write_byte(1, hl, v2);
                cpu.e = bus.read_byte(1, 0x98A3);
                cpu.d = bus.read_byte(1, 0x98A2);
                cpu.a = cpu.d;
                decomp_cpu1_check_playfield_bounds(cpu);
                cpu.cb_op(0x49, cpu.c);
                if (cpu.c & 0x02) != 0 {
                    let v3 = cpu.inc8(bus.read_byte(1, hl));
                    let v4 = cpu.inc8(v3);
                    bus.write_byte(1, hl, v4);
                }
            }
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_step_digdug_2px";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 120;
            true
        }

        // CPU 1 $114F: `load_digdug_xy_de`
        (1, 0x114F) => {
            cpu.e = bus.read_byte(1, 0x98A3);
            cpu.d = bus.read_byte(1, 0x98A2);
            cpu.a = cpu.d;
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_load_digdug_xy_de";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 44;
            true
        }

        // CPU 1 $1174: `check_playfield_bounds`
        (1, 0x1174) => {
            decomp_cpu1_check_playfield_bounds(cpu);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_check_playfield_bounds";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 75;
            true
        }

        // CPU 1 $118F: `enforce_grid_alignment_on_turn`
        (1, 0x118F) => {
            let saved_hl = cpu.hl();
            let saved_bc = cpu.bc();
            cpu.a = bus.read_byte(1, 0x85AE);
            cpu.and8(0x0F);
            let is_vert = if cpu.a == 0x00 {
                true
            } else {
                cpu.sub8(0x04, false, false);
                cpu.a == 0x04
            };
            let aligned = if is_vert {
                cpu.a = bus.read_byte(1, 0x98A2);
                cpu.and8(0x0F);
                cpu.sub8(0x01, false, false);
                cpu.a == 0x01
            } else {
                cpu.sub8(0x02, false, false);
                let is_horiz = if cpu.a == 0x02 {
                    true
                } else {
                    cpu.sub8(0x06, false, false);
                    cpu.a == 0x06
                };
                if !is_horiz {
                    true
                } else {
                    cpu.a = bus.read_byte(1, 0x98A3);
                    cpu.sub8(0x20, false, false);
                    if cpu.a >= 0x20 {
                        cpu.and8(0x0F);
                        cpu.a == 0x00
                    } else {
                        cpu.sub8(0x08, false, false);
                        cpu.a == 0x08
                    }
                }
            };
            if !aligned {
                cpu.a = bus.read_byte(1, 0x85AF);
                cpu.and8(0x0F);
                bus.write_byte(1, 0x85AE, cpu.a);
                cpu.a &= !0x01;
            } else {
                cpu.a = bus.read_byte(1, 0x8657);
                cpu.cb_op(0x4F, cpu.a);
                if (cpu.a & 0x02) == 0 {
                    cpu.a = bus.read_byte(1, 0x85AE);
                    cpu.and8(0x0F);
                    let cur_dir = cpu.a;
                    cpu.a = bus.read_byte(1, 0x85AF);
                    cpu.and8(0x0F);
                    cpu.sub8(cur_dir, false, false);
                    if cpu.a != cur_dir {
                        bus.write_byte(1, 0x85B3, 0x02);
                    }
                }
                cpu.a |= 0x01;
            }
            cpu.set_bc(saved_bc);
            cpu.set_hl(saved_hl);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_enforce_grid_alignment";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 130;
            true
        }

        // CPU 1 $11EA: `check_rock_collision_ahead`
        (1, 0x11EA) => {
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
                let target_x = bus.read_byte(1, 0x85E1);
                let target_y = bus.read_byte(1, 0x85E0);
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
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_check_rock_ahead";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 190;
            true
        }

        // CPU 1 $1251: `update_round_timers_and_escape`
        (1, 0x1251) => {
            cpu.a = bus.read_byte(1, 0x8657);
            cpu.cb_op(0x4F, cpu.a);
            if (cpu.a & 0x02) != 0 {
                let mut hl = 0x8528u16;
                cpu.set_de(0x0010);
                cpu.b = 0x08;
                cpu.c = 0x08;
                for _ in 0..8 {
                    let v = bus.read_byte(1, hl);
                    cpu.cb_op(0x7E, v);
                    if (v & 0x80) != 0 {
                        cpu.c = cpu.dec8(cpu.c);
                    }
                    hl = cpu.add16(hl, 0x0010);
                }
                cpu.set_hl(hl);
                cpu.b = 0;
                cpu.a = cpu.c;
                cpu.sub8(0x01, false, false);
                if cpu.c == 0x01 {
                    cpu.set_hl(0x8676);
                    bus.write_byte(1, 0x8676, 0x90);
                }
            }

            let timer = bus.read_word(1, 0x8625);
            cpu.set_hl(timer);
            cpu.set_de(0x0960);
            cpu.and8(cpu.a);
            cpu.sbc_hl(0x0960);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.set_hl(0x8653);
                let s = bus.read_byte(1, 0x8653) | 0x02;
                bus.write_byte(1, 0x8653, s);
            }

            let timer = bus.read_word(1, 0x8625);
            cpu.set_hl(timer);
            cpu.a = cpu.h;
            cpu.and8(cpu.a);
            let mut dec_timer = true;
            if cpu.h == 0 {
                cpu.a = cpu.l;
                cpu.sub8(0x96, false, false);
                if cpu.l == 0x96 {
                    cpu.a = bus.read_byte(1, 0x864A);
                    cpu.cb_op(0x6F, cpu.a);
                    if (cpu.a & 0x20) == 0 {
                        cpu.a |= 0x20;
                        bus.write_byte(1, 0x864A, cpu.a);
                        cpu.a = bus.read_byte(1, 0x87F0);
                        cpu.cb_op(0x47, cpu.a);
                        if (cpu.a & 0x01) == 0 {
                            cpu.a = 0x01;
                            bus.write_byte(1, 0x9A89, 0x01);
                        }
                    }
                } else {
                    cpu.and8(cpu.a);
                    if cpu.l == 0x00 {
                        cpu.set_hl(0x8646);
                        let s = bus.read_byte(1, 0x8646) | 0x08;
                        bus.write_byte(1, 0x8646, s);
                        dec_timer = false;
                    }
                }
            }
            if dec_timer {
                let next_t = timer.wrapping_sub(1);
                cpu.set_hl(next_t);
                bus.write_word(1, 0x8625, next_t);
            }

            cpu.set_hl(0x864A);
            let s864a = bus.read_byte(1, 0x864A);
            cpu.cb_op(0x66, s864a);
            if (s864a & 0x10) == 0 {
                let mut hl = 0x8528u16;
                cpu.b = 0x08;
                cpu.c = 0x08;
                cpu.set_de(0x0010);
                for _ in 0..8 {
                    let v = bus.read_byte(1, hl);
                    cpu.cb_op(0x7E, v);
                    if (v & 0x80) != 0 {
                        cpu.c = cpu.dec8(cpu.c);
                    }
                    hl = cpu.add16(hl, 0x0010);
                }
                cpu.set_hl(hl);
                cpu.b = 0;
                cpu.a = cpu.c;
                cpu.and8(cpu.a);
                if cpu.c != 0 {
                    cpu.set_hl(0x8400);
                    cpu.a = bus.read_byte(1, 0x840D);
                    let s8400 = bus.read_byte(1, 0x8400);
                    cpu.cb_op(0x4E, s8400);
                    if (s8400 & 0x02) != 0 {
                        cpu.a = bus.read_byte(1, 0x840E);
                    }
                    cpu.b = 0x02;
                    let mut trigger_escape = false;
                    let mut check_c_lt_b = true;
                    for &thresh in &[0x04u8, 0x08, 0x0C, 0x10, 0x14, 0x18] {
                        cpu.sub8(thresh, false, false);
                        if (cpu.f & FLAG_C) != 0 {
                            break;
                        }
                        if thresh == 0x18 {
                            check_c_lt_b = false;
                            trigger_escape = true;
                            break;
                        }
                        cpu.b = cpu.inc8(cpu.b);
                    }
                    if check_c_lt_b {
                        cpu.a = cpu.c;
                        cpu.sub8(cpu.b, false, false);
                        if (cpu.f & FLAG_C) != 0 {
                            trigger_escape = true;
                        }
                    }
                    if trigger_escape {
                        bus.write_word(1, 0x8625, 0x0104);
                        cpu.set_hl(0x864A);
                        let s = bus.read_byte(1, 0x864A) | 0x10;
                        bus.write_byte(1, 0x864A, s);
                    }
                }
            }
            stats.monster_speed_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_update_timers_and_escape";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 260;
            true
        }

        // CPU 1 $195B & $1961: `set_digdug_sprite_0D_or_0C`
        (1, 0x195B) => {
            cpu.set_hl(0x9822);
            bus.write_byte(1, 0x9822, 0x0D);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_set_digdug_sprite_0d";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 30;
            true
        }

        (1, 0x1961) => {
            cpu.set_hl(0x9822);
            bus.write_byte(1, 0x9822, 0x0C);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_set_digdug_sprite_0c";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 30;
            true
        }

        // CPU 1 $1967: `update_escape_exit_and_veggie`
        (1, 0x1967) => {
            decomp_cpu1_update_escape_exit_and_veggie(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_update_escape_and_veggie";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 280;
            true
        }

        // CPU 1 $1B1E: `setup_veggie_for_round`
        (1, 0x1B1E) => {
            decomp_cpu1_setup_veggie_for_round(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_setup_veggie_for_round";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 125;
            true
        }

        // CPU 1 $0080: `cpu1_vblank_handler`
        (1, 0x0080) => {
            decomp_cpu1_vblank_handler(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_vblank_handler";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 950;
            true
        }

        // CPU 1 $02EA: `clear_player_and_harpoon_sprites`
        (1, 0x02EA) => {
            decomp_cpu1_clear_player_and_harpoon_sprites(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_clear_player_harpoon_spr";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 120;
            true
        }

        // CPU 1 $049B: `attract_cutscene_dispatcher`
        (1, 0x049B) => {
            decomp_cpu1_attract_cutscene_dispatcher(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_attract_cutscene_disp";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 420;
            true
        }

        // CPU 1 $10C7: `advance_digdug_anim_and_move`
        (1, 0x10C7) => {
            decomp_cpu1_advance_digdug_anim_and_move(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_advance_digdug_anim_move";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 190;
            true
        }

        // CPU 1 $130E: `update_digging_and_tunnels`
        (1, 0x130E) => {
            decomp_cpu1_update_digging_and_tunnels(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_update_digging_tunnels";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 320;
            true
        }

        // CPU 1 $140A: `carve_tunnel_front_tiles`
        (1, 0x140A) => {
            decomp_cpu1_carve_tunnel_front_tiles(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_carve_tunnel_front";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 160;
            true
        }

        // CPU 1 $14EF: `carve_tunnel_back_tiles`
        (1, 0x14EF) => {
            decomp_cpu1_carve_tunnel_back_tiles(cpu, bus, roms);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_carve_tunnel_back";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 210;
            true
        }

        // CPU 1 $1DDC: `service_earom_highscore_io`
        (1, 0x1DDC) => {
            decomp_cpu1_service_earom_highscore_io(cpu, bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_service_earom_io";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 80;
            true
        }

        // CPU 1 $1F07: `cpu1_rom_checksum_init`
        (1, 0x1F07) => {
            decomp_cpu1_rom_checksum_init(cpu, bus, roms);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu1_rom_checksum_init";
            cpu.total_cycles += 98_500;
            true
        }

        // =====================================================================
        // CPU 0: I/O Queue, Video Latches, HUD, Score, Harpoon, Collision & Spawners
        // =====================================================================

        (0, 0x00A1) => {
            decomp_queue_06xx_io_command(cpu, bus);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_queue_06xx_io_cmd";
            cpu.total_cycles += 120;
            true
        }

        (0, 0x0BA6) => {
            for addr in 0xA000..0xA004u16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.set_hl(0xA004);
            cpu.b = 0;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_vidlatches_a000";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 66;
            true
        }

        (0, 0x0BB1) => {
            let status = bus.read_byte(0, 0x8400);
            cpu.cb_op(0x4E, status);
            let flip = if (status & 0x02) != 0 {
                cpu.cb_op(0x56, status);
                u8::from((status & 0x04) != 0)
            } else {
                0
            };
            bus.write_byte(0, 0xA007, flip);
            cpu.set_hl(0xA007);
            cpu.set_de(0x8400);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_cocktail_flip";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 64;
            true
        }

        (0, 0x0BCA) => {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83D4, 0x1445, 0x0A);
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83DA, 0x1465, 0x03);
            cpu.set_hl(0x8417);
            cpu.set_bc(0x0300);
            let s8400 = bus.read_byte(0, 0x8400);
            cpu.a = s8400;
            cpu.and8(0x01);
            if cpu.a != 0 {
                decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83C7, 0x1468, 0x03);
            }
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_hud_headers";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 320;
            true
        }

        (0, 0x0BDF) => {
            let mut hl = 0x83C0u16;
            for _ in 0..0x3F {
                let v = bus.read_byte(0, hl);
                cpu.a = v;
                cpu.sub8(0x8C, false, false);
                if v == 0x8C {
                    bus.write_byte(0, hl, 0x0C);
                }
                bus.write_byte(0, 0x6830, cpu.a);
                hl = hl.wrapping_add(1);
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_convert_hud_row_8c";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 800;
            true
        }

        (0, 0x0BF2) => {
            let mut hl = 0x9800u16;
            let mut de = 0x9880u16;
            for _ in 0..0x80 {
                bus.write_byte(0, 0x6830, cpu.a);
                cpu.xor8(cpu.a);
                bus.write_byte(0, hl, 0);
                bus.write_byte(0, de, 0);
                hl = hl.wrapping_add(1);
                de = de.wrapping_add(1);
            }
            cpu.set_hl(hl);
            cpu.set_de(de);
            cpu.b = 0;
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_all_sprite_wram";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 960;
            true
        }

        (0, 0x0C81) => {
            decomp_check_and_award_bonus_life(cpu, bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_award_bonus_life";
            cpu.total_cycles += 240;
            true
        }

        (0, 0x0DA2) => {
            decomp_update_harpoon_and_pump_state(cpu, bus, roms);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_harpoon_and_pump";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x0E9F) => {
            decomp_advance_monster_pump_stage_0e9f(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_advance_monster_pump";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x0F97) => {
            decomp_init_harpoon_sprites_body(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_harpoon_sprites";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x103B) => {
            decomp_check_harpoon_dirt_collision_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_harpoon_dirt_col";
            cpu.total_cycles += 140;
            true
        }

        (0, 0x10E0) => {
            decomp_check_harpoon_monsters_10e0(cpu, bus, roms);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_harpoon_monsters";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x11DA) => {
            let v = bus.read_byte(0, 0x8656) | 0x08;
            bus.write_byte(0, 0x8656, v);
            cpu.set_hl(0x8656);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_set_pump_carve_flag";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 35;
            true
        }

        (0, 0x11E0) => {
            let mut hl = 0x9824u16;
            for _ in 0..0x0C {
                bus.write_byte(0, hl, 0x32);
                hl = hl.wrapping_add(2);
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_fygar_fire_spr";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 220;
            true
        }

        (0, 0x11EC) => {
            let s8400 = bus.read_byte(0, 0x8400);
            cpu.cb_op(0x4E, s8400);
            let round = if (s8400 & 0x02) != 0 {
                let r = bus.read_byte(0, 0x840E);
                bus.write_byte(0, 0x840F, r);
                bus.write_byte(0, 0x8410, r);
                r
            } else {
                let r = bus.read_byte(0, 0x840D);
                bus.write_byte(0, 0x840F, r);
                bus.write_byte(0, 0x8410, r);
                r
            };
            cpu.a = round;
            cpu.set_hl(0x8410);
            decomp_draw_round_indicator_from_19bd(cpu, bus, roms);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_sync_round_and_hud";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x128C) => {
            for addr in 0x9A80..0x9AA0u16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.set_hl(0x9AA0);
            cpu.b = 0;
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_sound_queues";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 320;
            true
        }

        (0, 0x1297) => {
            decomp_swap_memory_blocks(cpu, bus, 0x8525, 0x84A5, 0x0080);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_swap_monster_ram";
            cpu.total_cycles += 800;
            true
        }

        (0, 0x12A4) => {
            decomp_swap_memory_blocks(cpu, bus, 0x8465, 0x8425, 0x0040);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_swap_rock_ram";
            cpu.total_cycles += 400;
            true
        }

        (0, 0x12B1) => {
            decomp_swap_memory_blocks(cpu, bus, 0x8040, 0x9000, 0x0380);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_swap_dirt_vram";
            cpu.total_cycles += 3200;
            true
        }

        (0, 0x12BE) => {
            decomp_swap_memory_blocks(cpu, bus, cpu.hl(), cpu.de(), cpu.bc());
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_swap_memory_blocks";
            cpu.total_cycles += 400;
            true
        }

        (0, 0x12C9) => {
            decomp_setup_player_word_sprites(cpu, bus, roms);
            decomp_setup_player_num_sprite(cpu, bus, 0x6A);
            decomp_setup_game_over_word_sprites(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_p1_gameover_spr";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x12CF | 0x12DC) => {
            decomp_setup_game_over_word_sprites(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_gameover_spr";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x12D3) => {
            decomp_setup_player_word_sprites(cpu, bus, roms);
            decomp_setup_player_num_sprite(cpu, bus, 0x6B);
            decomp_setup_game_over_word_sprites(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_p2_gameover_spr";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x1319) => {
            decomp_setup_player_word_sprites(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_player_word_spr";
            cpu.total_cycles += 210;
            true
        }

        (0, 0x1333) => {
            decomp_setup_player_num_sprite(cpu, bus, 0x6A);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_p1_num_spr";
            cpu.total_cycles += 74;
            true
        }

        (0, 0x1344) => {
            decomp_setup_player_num_sprite(cpu, bus, 0x6B);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_p2_num_spr";
            cpu.total_cycles += 86;
            true
        }

        (0, 0x134B | 0x1CD3) => {
            decomp_clear_text_overlay_sprites(cpu, bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_text_overlay_spr";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x134E) => {
            decomp_check_monster_player_collision_134e(cpu, bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_monster_player_col";
            cpu.total_cycles += 240;
            true
        }

        (0, 0x140B) => {
            let s8400 = bus.read_byte(0, 0x8400);
            cpu.set_hl(0x8400);
            cpu.cb_op(0x4E, s8400);
            let lives = if (s8400 & 0x02) != 0 {
                bus.read_byte(0, 0x840B)
            } else {
                bus.read_byte(0, 0x840A)
            };
            cpu.a = lives;
            bus.write_byte(0, 0x840C, lives);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_sync_active_lives_840c";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 58;
            true
        }

        (0, 0x1425) => {
            let v = bus.read_byte(0, 0x8646);
            cpu.a = v;
            cpu.cb_op(0x47, v);
            if (v & 0x01) != 0 {
                cpu.xor8(cpu.a);
                bus.write_byte(0, 0x9A92, 0x00);
            } else {
                cpu.a = 0x01;
                bus.write_byte(0, 0x9A92, 0x01);
            }
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_music_enable";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 50;
            true
        }

        (0, 0x1432) => {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83D4, 0x1445, 0x0A);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_hud_high_score";
            cpu.total_cycles += 190;
            true
        }

        (0, 0x144F) => {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83DA, 0x1465, 0x03);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_hud_1up_text";
            cpu.total_cycles += 95;
            true
        }

        (0, 0x145B) => {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83C7, 0x1468, 0x03);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_hud_2up_text";
            cpu.total_cycles += 95;
            true
        }

        (0, 0x146B) => {
            decomp_update_all_hud_scores_and_headers(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_all_hud_scores";
            cpu.total_cycles += 480;
            true
        }

        (0, 0x1483) => {
            decomp_blink_active_1up_2up_headers_body(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_blink_1up_2up_headers";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x14B8) => {
            decomp_blank_up_header_body(cpu, bus, 0x83DA);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_blank_1up_header";
            cpu.total_cycles += 88;
            true
        }

        (0, 0x14D8) => {
            decomp_blank_up_header_body(cpu, bus, 0x83C7);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_blank_2up_header";
            cpu.total_cycles += 98;
            true
        }

        (0, 0x14DE) => {
            cpu.set_de(0x8927);
            cpu.ix = 0x83F2;
            decomp_draw_bcd_score_6digits_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_high_score_digits";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x14E9) => {
            cpu.set_de(0x8414);
            cpu.ix = 0x83FC;
            decomp_draw_bcd_score_6digits_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_p1_score_digits";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x14F4) => {
            cpu.set_de(0x8417);
            cpu.ix = 0x83E9;
            decomp_draw_bcd_score_6digits_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_p2_score_digits";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x14FF) => {
            decomp_draw_bcd_score_6digits_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_bcd_score_6digits";
            cpu.total_cycles += 250;
            true
        }

        (0, 0x1551) => {
            let mut hl = 0x98A4u16;
            for _ in 0..0x0C {
                bus.write_byte(0, hl, 0x00);
                hl = hl.wrapping_add(1);
                bus.write_byte(0, hl, 0x50);
                hl = hl.wrapping_add(1);
            }
            cpu.set_hl(hl);
            cpu.b = 0;
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_reset_fygar_fire_pos";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 240;
            true
        }

        (0, 0x155F) => {
            decomp_check_fire_player_collision_155f(cpu, bus);
            stats.harpoon_pump_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_fire_player_col";
            cpu.total_cycles += 120;
            true
        }

        (0, 0x15BF) => {
            decomp_fill_vram_with_7f(cpu, bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_fill_vram_with_7f";
            cpu.total_cycles += 420;
            true
        }

        (0, 0x15CB) => {
            decomp_write_tile_column_from_ix(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_write_tile_col_from_ix";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x15DB) => {
            decomp_draw_credit_text_and_bcd(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_credit_count_hud";
            cpu.total_cycles += 210;
            true
        }

        (0, 0x16E5) => {
            let mut hl = 0x8532u16;
            let mut de = 0x9964u16;
            for _ in 0..8 {
                let v = bus.read_byte(0, hl);
                cpu.a = v;
                bus.write_byte(0, de, v);
                de = de.wrapping_add(2);
                hl = cpu.add16(hl, 0x0010);
            }
            cpu.set_hl(hl);
            cpu.set_de(de);
            cpu.b = 0;
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_sync_monster_flips";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 240;
            true
        }

        (0, 0x17ED) => {
            for addr in 0x8411..0x841Au16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.set_hl(0x841A);
            cpu.b = 0;
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_score_wram";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 140;
            true
        }

        (0, 0x17F8) => {
            decomp_init_round_counters_17f8(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_round_counters";
            cpu.total_cycles += 120;
            true
        }

        (0, 0x183E) => {
            let a = bus.read_byte(0, 0x8409);
            cpu.a = a;
            bus.write_byte(0, 0x840A, a);
            bus.write_byte(0, 0x840B, a);
            bus.write_byte(0, 0x840C, a);
            cpu.set_hl(0x840C);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_lives_from_8409";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 52;
            true
        }

        (0, 0x1872) => {
            let hl = cpu.hl();
            bus.write_byte(0, hl, 0x7F);
            bus.write_byte(0, hl.wrapping_sub(1), 0x7F);
            let hl2 = cpu.add16(hl.wrapping_sub(1), 0x0020);
            bus.write_byte(0, hl2, 0x7F);
            let hl3 = hl2.wrapping_add(1);
            bus.write_byte(0, hl3, 0x7F);
            cpu.set_hl(hl3);
            cpu.set_de(0x0020);
            cpu.a = 0x7F;
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_single_life_icon";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 75;
            true
        }

        (0, 0x1892) => {
            decomp_init_rock_state_array(cpu, bus, 0x8425);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_rock_ram_p1";
            cpu.total_cycles += 280;
            true
        }

        (0, 0x18A7) => {
            decomp_init_rock_state_array(cpu, bus, 0x8465);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_rock_ram_p2";
            cpu.total_cycles += 280;
            true
        }

        (0, 0x18AC) => {
            decomp_carve_round_initial_tunnels_18ac(cpu, bus, roms);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_carve_round_tunnels";
            cpu.total_cycles += 680;
            true
        }

        (0, 0x1928) => {
            decomp_stamp_tunnel_segment_1928(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_stamp_tunnel_segment";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x1962) => {
            let mut hl = cpu.hl();
            for _ in 0..4 {
                hl = hl.wrapping_add(1);
                bus.write_byte(0, hl, cpu.a);
            }
            cpu.set_hl(hl);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_fill_4_bytes_inc_hl";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 62;
            true
        }

        (0, 0x1964) => {
            let mut hl = cpu.hl();
            for _ in 0..3 {
                hl = hl.wrapping_add(1);
                bus.write_byte(0, hl, cpu.a);
            }
            cpu.set_hl(hl);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_fill_3_bytes_inc_hl";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 49;
            true
        }

        (0, 0x1993) => {
            let hl = cpu.add16(cpu.hl(), cpu.de());
            bus.write_byte(0, hl, 0x08);
            bus.write_byte(0, hl.wrapping_add(1), 0x09);
            cpu.set_hl(hl);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_carve_vert_tunnel_pair";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 53;
            true
        }

        (0, 0x1A21) => {
            decomp_convert_byte_at_hl_to_bcd(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_convert_hl_to_bcd";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x1A39) => {
            decomp_spawn_round_monsters_from_rom(cpu, bus, roms);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_spawn_round_monsters";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x1A73) => {
            decomp_sanitize_spawned_monster_slots(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_sanitize_monster_slots";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x1A94) => {
            decomp_init_monster_default_sprites(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_monster_sprites";
            cpu.total_cycles += 280;
            true
        }

        (0, 0x1AC6) => {
            decomp_spawn_round_rocks_from_rom(cpu, bus, roms);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_spawn_round_rocks";
            cpu.total_cycles += 340;
            true
        }

        (0, 0x1B2E) => {
            decomp_init_digdug_player_spawn(cpu, bus);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_digdug_spawn";
            cpu.total_cycles += 140;
            true
        }

        (0, 0x1B7D) => {
            let s8400 = bus.read_byte(0, 0x8400);
            cpu.cb_op(0x4E, s8400);
            let hl = if (s8400 & 0x02) != 0 { 0x840B } else { 0x840A };
            let v = cpu.dec8(bus.read_byte(0, hl));
            bus.write_byte(0, hl, v);
            cpu.a = v;
            cpu.set_hl(hl);
            bus.write_byte(0, 0x840C, v);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_dec_player_lives";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 78;
            true
        }

        (0, 0x1B92) => {
            let mut hl = 0x8465u16;
            let mut ix = 0x98BCu16;
            for _ in 0..8 {
                let x = bus.read_byte(0, hl);
                bus.write_byte(0, ix, x);
                hl = hl.wrapping_add(1);
                ix = ix.wrapping_add(1);
                let y = bus.read_byte(0, hl);
                cpu.a = y;
                bus.write_byte(0, ix, y);
                ix = ix.wrapping_add(1);
                hl = cpu.add16(hl, 0x0007);
            }
            cpu.set_hl(hl);
            cpu.set_de(0x0007);
            cpu.ix = ix;
            cpu.b = 0;
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_copy_rock_coords";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 280;
            true
        }

        (0, 0x1BAF) => {
            let mut hl = 0x8529u16;
            let mut ix = 0x9864u16;
            let mut iy = 0x98E4u16;
            for _ in 0..8 {
                let b0 = bus.read_byte(0, hl);
                bus.write_byte(0, iy, b0);
                iy = iy.wrapping_add(1);
                hl = hl.wrapping_add(1);
                let b1 = bus.read_byte(0, hl);
                bus.write_byte(0, iy, b1);
                hl = hl.wrapping_add(1);
                iy = iy.wrapping_add(1);
                let b2 = bus.read_byte(0, hl);
                bus.write_byte(0, ix, b2);
                hl = hl.wrapping_add(1);
                ix = ix.wrapping_add(1);
                let b3 = bus.read_byte(0, hl);
                cpu.a = b3;
                bus.write_byte(0, ix, b3);
                ix = ix.wrapping_add(1);
                hl = cpu.add16(hl, 0x000D);
            }
            cpu.set_hl(hl);
            cpu.set_de(0x000D);
            cpu.ix = ix;
            cpu.iy = iy;
            cpu.b = 0;
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_copy_monster_wram";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 420;
            true
        }

        (0, 0x1BDE) => {
            let r = bus.read_byte(0, 0x8627);
            cpu.a = r;
            cpu.sub8(0x10, false, false);
            if r >= 0x10 {
                cpu.and8(0x03);
                cpu.add8(0x0C, false);
            }
            cpu.a = cpu.dec8(cpu.a);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_calc_stage_layout_idx";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 48;
            true
        }

        (0, 0x1C94) => {
            decomp_setup_player_ready_sprites_1c94(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_player_ready_spr";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x1CF8) => {
            let seed = bus.read_word(0, 0x85F4);
            let next = seed.wrapping_mul(5).wrapping_add(1);
            bus.write_word(0, 0x85F4, next);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_step_prng_lcg";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 143;
            true
        }

        (0, 0x1D0D) => {
            let saved = (cpu.ix, cpu.iy, cpu.hl(), cpu.de(), cpu.bc(), cpu.af());
            let s8656 = bus.read_byte(0, 0x8656);
            if (s8656 & 0x04) != 0 {
                bus.write_byte(0, 0x8656, s8656 & !0x04);
                let y = bus.read_byte(0, 0x98A3);
                if y < 0x18 {
                    bus.write_byte(0, 0x8648, 0x00);
                } else {
                    let cnt = bus.read_byte(0, 0x8648).wrapping_add(1);
                    if cnt == 0x02 {
                        bus.write_byte(0, 0x8648, 0x00);
                        cpu.l = 0x00;
                        decomp_add_score_from_table_2fb5_body(cpu, bus, roms);
                    } else {
                        bus.write_byte(0, 0x8648, cnt);
                    }
                }
            }
            cpu.ix = saved.0;
            cpu.iy = saved.1;
            cpu.set_hl(saved.2);
            cpu.set_de(saved.3);
            cpu.set_bc(saved.4);
            cpu.set_af(saved.5);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_digging_10pt";
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 210;
            true
        }

        (0, 0x1ECC) => {
            cpu.a = 0x0F;
            cpu.set_hl(0x812A);
            cpu.ix = 0x1DC2;
            decomp_write_tile_column_from_ix(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_push_start_1ecc";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x1ED9) => {
            cpu.a = 0x0D;
            cpu.set_hl(0x8130);
            cpu.ix = 0x1DDF;
            decomp_write_tile_column_from_ix(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_push_start_1ed9";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x1EE6) => {
            cpu.a = 0x0E;
            cpu.set_hl(0x8110);
            cpu.ix = 0x1DD1;
            decomp_write_tile_column_from_ix(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_push_start_1ee6";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x1EF3) => {
            cpu.set_hl(0x812A);
            cpu.a = 0x0F;
            cpu.ix = 0x1DAF;
            decomp_write_tile_column_from_ix(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_push_start_1ef3";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x2D3A) => {
            decomp_monster_try_horizontal_dir_2d3a(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_monster_try_horiz_dir";
            cpu.total_cycles += 140;
            true
        }

        (0, 0x2D67) => {
            decomp_monster_try_vertical_dir_2d67(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_monster_try_vert_dir";
            cpu.total_cycles += 140;
            true
        }

        (0, 0x2DA8) => {
            decomp_monster_check_dir_walkable_2da8(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_monster_check_walkable";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x2E42) => {
            decomp_check_vert_tunnel_tile_2e42_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_vert_tunnel_tile";
            cpu.total_cycles += 55;
            true
        }

        (0, 0x2E84) => {
            decomp_check_horiz_tunnel_tile_2e84_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_horiz_tunnel_tile";
            cpu.total_cycles += 55;
            true
        }

        (0, 0x2EA6) => {
            decomp_check_open_tunnel_tile_2ea6_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_check_open_tunnel_tile";
            cpu.total_cycles += 60;
            true
        }

        (0, 0x2ED4) => {
            decomp_fygar_check_fire_clearance_2ed4(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_fygar_fire_clearance";
            cpu.total_cycles += 190;
            true
        }

        (0, 0x2F7A) => {
            decomp_get_monster_timer_864b_ptr(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_get_monster_timer_ptr";
            cpu.total_cycles += 65;
            true
        }

        (0, 0x2F92) => {
            decomp_get_monster_timer_864b_ptr(cpu, bus);
            bus.write_byte(0, cpu.hl(), 0x03);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_set_monster_timer_3";
            cpu.total_cycles += 75;
            true
        }

        (0, 0x2F98) => {
            decomp_get_monster_timer_864b_ptr(cpu, bus);
            let hl = cpu.hl();
            let v = bus.read_byte(0, hl);
            let nv = cpu.inc8(v);
            bus.write_byte(0, hl, nv);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_inc_monster_timer_864b";
            cpu.total_cycles += 80;
            true
        }

        (0, 0x2F9D) => {
            let iy = cpu.iy;
            bus.write_byte(0, iy, 0x50);
            bus.write_byte(0, iy.wrapping_sub(2), 0x50);
            bus.write_byte(0, iy.wrapping_sub(4), 0x50);
            cpu.a = 0;
            cpu.xor8(0);
            let ix = cpu.ix;
            bus.write_byte(0, ix, 0x00);
            bus.write_byte(0, ix.wrapping_sub(2), 0x00);
            bus.write_byte(0, ix.wrapping_sub(4), 0x00);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_fygar_fire_2f9d";
            cpu.total_cycles += 90;
            true
        }

        (0, 0x2FB3) => {
            cpu.l = 0x00;
            decomp_add_score_from_table_2fb5_body(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_add_10_points_2fb3";
            cpu.total_cycles += 190;
            true
        }

        (0, 0x2FB5) => {
            decomp_add_score_from_table_2fb5_body(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_add_score_table_2fb5";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x2FC8) => {
            decomp_add_score_2fc8_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_add_pending_bcd_score";
            cpu.total_cycles += 160;
            true
        }

        (0, 0x35C9) => {
            cpu.set_de(0x836D);
            for _ in 0..3 {
                decomp_clear_big_score_row_35d5_body(cpu, bus);
            }
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_big_score_banner";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x35D5) => {
            decomp_clear_big_score_row_35d5_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_big_score_row";
            cpu.total_cycles += 140;
            true
        }

        (0, 0x3681) => {
            cpu.and8(0x0F);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.a = 0x27;
            }
            decomp_write_digit_tile_step_left_3687_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_write_leading_digit";
            cpu.total_cycles += 65;
            true
        }

        (0, 0x3687) => {
            decomp_write_digit_tile_step_left_3687_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_write_digit_3687";
            cpu.total_cycles += 55;
            true
        }

        (0, 0x36E9) => {
            decomp_step_vram_de_left_36e9_body(cpu);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_step_vram_de_left";
            cpu.total_cycles += 38;
            true
        }

        (0, 0x36F0) => {
            cpu.a = 0x37;
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_load_space_tile_36f0";
            cpu.total_cycles += 17;
            true
        }

        (0, 0x36F3) => {
            decomp_render_string_descriptor_36f3(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_string_desc";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x370C) => {
            decomp_ascii_to_tile_370c_body(cpu);
            cpu.ret_subroutine(bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_ascii_to_tile_370c";
            cpu.total_cycles += 60;
            true
        }

        (0, 0x3A85) => {
            decomp_decode_dip_switches_3a85(cpu, bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_decode_dip_switches";
            cpu.total_cycles += 120;
            true
        }

        (0, 0x3BB5) => {
            decomp_clear_24_cols_with_37_3bb5_body(cpu, bus);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_24_cols_3bb5";
            cpu.total_cycles += 180;
            true
        }

        (0, 0x3BC0) => {
            let mut a = bus.read_byte(0, 0x898C);
            cpu.a = a;
            cpu.and8(a);
            if a == 0 {
                a = 0x05;
                cpu.a = a;
            }
            cpu.add8(0x10, false);
            bus.write_byte(0, 0x826C, cpu.a);
            cpu.sub8(0x11, false, false);
            cpu.a = 0x37;
            bus.write_byte(0, 0x824C, 0x37);
            cpu.set_hl(0x3F3E);
            decomp_render_string_descriptor_36f3(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_lives";
            cpu.total_cycles += 220;
            true
        }

        (0, 0x3BDE) => {
            let a = bus.read_byte(0, 0x898D);
            cpu.a = a;
            cpu.add8(0x1A, false);
            bus.write_byte(0, 0x82AE, cpu.a);
            cpu.set_hl(0x3F48);
            decomp_render_string_descriptor_36f3(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_rank";
            cpu.total_cycles += 210;
            true
        }

        (0, 0x3BEC) => {
            let a = bus.read_byte(0, 0x898E);
            cpu.a = a;
            cpu.and8(a);
            let hl = if (cpu.f & FLAG_Z) != 0 { 0x3F50 } else { 0x3F5B };
            cpu.set_hl(hl);
            decomp_render_string_descriptor_36f3(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_cabinet";
            cpu.total_cycles += 210;
            true
        }

        (0, 0x3C27) => {
            let mut a = bus.read_byte(0, 0x8989);
            a = cpu.inc8(a);
            cpu.a = a;
            cpu.sub8(0x15, false, false);
            if (cpu.f & FLAG_C) == 0 {
                cpu.a = 0;
                cpu.xor8(0);
                a = 0;
            }
            bus.write_byte(0, 0x8989, a);
            for addr in 0x9A80..0x9AACu16 {
                bus.write_byte(0, addr, 0x00);
            }
            let bc = a as u16;
            let hl = cpu.add16(0x9A80, bc);
            bus.write_byte(0, hl, 0x01);
            cpu.set_bc(bc);
            cpu.set_hl(hl);
            cpu.ret_subroutine(bus);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_sound_test";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x3D07) => {
            for _ in 0..3 {
                decomp_draw_crosshatch_byte_3d15_body(cpu, bus, roms);
            }
            cpu.a = 0x05;
            let hl = cpu.add16(cpu.hl(), 0x0005);
            cpu.set_hl(hl);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_crosshatch_col";
            cpu.total_cycles += 320;
            true
        }

        (0, 0x3D15) => {
            decomp_draw_crosshatch_byte_3d15_body(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_crosshatch_byte";
            cpu.total_cycles += 110;
            true
        }

        (0, 0x3E33) => {
            decomp_clear_screen_and_sprites_3e33_3e3b_body(cpu, bus, 0x8040, 0x04C0);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_screen_3e33";
            cpu.total_cycles += 640;
            true
        }

        (0, 0x3E3B) => {
            decomp_clear_screen_and_sprites_3e33_3e3b_body(cpu, bus, 0x8000, 0x0400);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_screen_3e3b";
            cpu.total_cycles += 640;
            true
        }

        (0, 0x3E5D) => {
            let mut hl = cpu.hl();
            for _ in 0..0x80 {
                bus.write_byte(0, hl, 0x00);
                hl = hl.wrapping_add(1);
            }
            cpu.a = 0;
            cpu.xor8(0);
            cpu.b = 0;
            cpu.set_hl(hl);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_128_bytes_3e5d";
            cpu.total_cycles += 240;
            true
        }

        (0, 0x3E85) => {
            decomp_clear_screen_and_sprites_3e33_3e3b_body(cpu, bus, 0x8000, 0x0400);
            bus.write_byte(0, 0xA000, 0x01);
            bus.write_byte(0, 0xA001, 0x00);
            bus.write_byte(0, 0xA002, 0x00);
            bus.write_byte(0, 0xA003, 0x00);
            cpu.b = 0;
            cpu.set_hl(0xA003);
            cpu.ret_subroutine(bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_screen_reset_vid";
            cpu.total_cycles += 680;
            true
        }

        (0, 0x0066) => {
            decomp_cpu0_nmi_06xx_io(cpu, bus);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_nmi_06xx_io";
            cpu.total_cycles += 85;
            true
        }

        (0, 0x1F00) => {
            decomp_update_rocks_and_monsters_1f00(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.rock_physics_calls += 6;
            stats.ghost_dig_ai_calls += 8;
            stats.monster_speed_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_rocks_and_monsters";
            cpu.total_cycles += 420;
            true
        }

        (0, 0x1F75) => {
            decomp_update_single_rock_1f75(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.rock_physics_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_single_rock";
            cpu.total_cycles += 190;
            true
        }

        (0, 0x2339) => {
            decomp_update_single_monster_2339(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_single_monster_2339";
            cpu.total_cycles += 260;
            true
        }

        (0, 0x2348) => {
            decomp_step_monster_ai_and_motion_2348(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            stats.ghost_dig_ai_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_step_monster_ai_2348";
            cpu.total_cycles += 210;
            true
        }

        (0, 0x0020) => {
            decomp_mod_100_loop_0020(cpu, bus);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_mod_100_loop_0020";
            true
        }

        (0, 0x0C05) => {
            decomp_init_stage_actors_and_hud_0c05(cpu, bus, roms);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_stage_actors_0c05";
            true
        }

        (0, 0x0C15) => {
            decomp_setup_round_tunnels_and_terrain_0c15(cpu, bus, roms);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_round_tunnels_0c15";
            true
        }

        (0, 0x0C22) => {
            decomp_init_round_state_and_clear_counters_0c22(cpu, bus, roms);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_init_round_state_0c22";
            true
        }

        (0, 0x1597) => {
            decomp_draw_title_screen_logo_1597(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_title_logo_1597";
            true
        }

        (0, 0x15A6) => {
            decomp_clear_sound_and_sub_cpu_ram_15a6(cpu, bus, roms);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_clear_playfield_7f_15a6";
            true
        }

        (0, 0x1606) => {
            decomp_draw_push_start_prompt_1606(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_push_start_1606";
            true
        }

        (0, 0x1625) => {
            decomp_draw_character_cast_table_1625(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_cast_table_1625";
            true
        }

        (0, 0x1640) => {
            decomp_setup_cast_sprites_1640(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_setup_cast_text_1640";
            true
        }

        (0, 0x1690) => {
            decomp_animate_title_namco_logo_1690(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_bonus_text_1690";
            true
        }

        (0, 0x16D3) => {
            decomp_draw_bonus_pair_and_sync_flips_16d3(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_bonus_pair_16d3";
            true
        }

        (0, 0x199B) => {
            decomp_abort_attract_demo_199b(cpu, bus, roms);
            stats.round_advance_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_demo_round_199b";
            true
        }

        (0, 0x1DEC) => {
            decomp_update_attract_title_screen_1dec(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_update_attract_title_1dec";
            true
        }

        (0, 0x3575) => {
            decomp_draw_big_highscore_digits_3575(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_big_hs_digits_3575";
            true
        }

        (0, 0x3593) => {
            decomp_draw_single_big_digit_3593(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_draw_single_big_digit_3593";
            true
        }

        (0, 0x35E3) => {
            decomp_render_highscore_table_35e3(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_hs_table_35e3";
            true
        }

        (0, 0x35E7) => {
            decomp_render_highscore_entries_35e7(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_hs_entries_35e7";
            true
        }

        (0, 0x35F8) => {
            decomp_render_single_highscore_row_35f8(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_single_hs_row_35f8";
            true
        }

        (0, 0x3645) => {
            decomp_render_highscore_row_score_3645(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_hs_row_score_3645";
            true
        }

        (0, 0x3661) => {
            decomp_render_two_digit_number_3661(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_two_digit_3661";
            true
        }

        (0, 0x3664) => {
            decomp_render_highscore_row_round_3664(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_hs_row_round_3664";
            true
        }

        (0, 0x36C6) => {
            decomp_render_bcd_bytes_36c6(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_bcd_bytes_36c6";
            true
        }

        (0, 0x36CE) => {
            decomp_render_bcd_byte_pair_36ce(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_bcd_pair_36ce";
            true
        }

        (0, 0x36DC) => {
            decomp_render_bcd_nibble_36dc(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_render_bcd_nibble_36dc";
            true
        }

        (0, 0x3AB8) => {
            decomp_service_poll_inputs_3ab8(cpu, bus, roms);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_poll_inputs_3ab8";
            true
        }

        (0, 0x3AF1) => {
            decomp_service_draw_coin_cred_settings_3af1(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_coin_cred_3af1";
            true
        }

        (0, 0x3B20) => {
            decomp_service_draw_ratio_pair_3b20(cpu, bus);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_ratio_3b20";
            true
        }

        (0, 0x3B4A) => {
            decomp_service_draw_bonus_life_settings_3b4a(cpu, bus, roms);
            stats.hud_lives_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_draw_bonus_3b4a";
            true
        }

        (0, 0x3BFC) => {
            decomp_service_update_sound_test_3bfc(cpu, bus, roms);
            stats.sound_engine_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_sound_test_3bfc";
            true
        }

        (0, 0x3C47) => {
            decomp_service_update_bookkeeping_display_3c47(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_bookkeeping_3c47";
            true
        }

        (0, 0x3C8B) => {
            decomp_service_draw_bcd_counter_3c8b(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_bcd_counter_3c8b";
            true
        }

        (0, 0x3C97) => {
            decomp_service_draw_bcd_byte_3c97(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_bcd_byte_3c97";
            true
        }

        (0, 0x3CA0) => {
            decomp_service_draw_bcd_digit_3ca0(cpu, bus, roms);
            stats.bcd_score_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_bcd_digit_3ca0";
            true
        }

        (0, 0x3CB3) => {
            decomp_service_check_secret_code_3cb3(cpu, bus, roms);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_service_secret_code_3cb3";
            true
        }

        (0, 0x0280) => {
            decomp_cpu0_vblank_handler(cpu, bus, roms);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_vblank_handler_0280";
            true
        }

        (0, 0x03FA) => {
            decomp_vblank_resume_after_005a_03fa(cpu, bus, roms);
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_vblank_resume_03fa";
            true
        }

        (0, 0x3DE3) => {
            decomp_ram_bank_lfsr_test_3de3(cpu, bus);
            stats.dirt_init_calls += 1;
            stats.total_rust_calls += 1;
            stats.calls_this_frame += 1;
            stats.last_hooked_name = "cpu0_ram_bank_lfsr_test_3de3";
            true
        }

        _ => false,
    }
}

fn decomp_adjust_harpoon_cable_segment<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, hl: u16, iy: u16) {
    cpu.set_hl(hl);
    cpu.iy = iy;
    let spr_9806 = bus.read_byte(0, 0x9806);
    if spr_9806 == 0x34 {
        let flip = bus.read_byte(0, 0x9906) & 0x01;
        cpu.a = flip;
        cpu.and8(flip);
        let y = bus.read_byte(0, iy);
        let new_y = if flip == 0 {
            let t = cpu.dec8(y);
            cpu.dec8(t)
        } else {
            let t = cpu.inc8(y);
            cpu.inc8(t)
        };
        cpu.a = new_y;
        bus.write_byte(0, iy, new_y);
    } else {
        let flip = bus.read_byte(0, 0x9906) & 0x02;
        cpu.a = flip;
        cpu.and8(flip);
        let x = bus.read_byte(0, hl);
        let new_x = if flip == 0 {
            let t = cpu.inc8(x);
            cpu.inc8(t)
        } else {
            let t = cpu.dec8(x);
            cpu.dec8(t)
        };
        bus.write_byte(0, hl, new_x);
    }
}

fn decomp_shift_monster_x<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, delta: i8) -> u8 {
    let base = bus.read_word(0, 0x85F6);
    let x_addr = base.wrapping_add(4);
    let x = bus.read_byte(0, x_addr);
    let new_x = x.wrapping_add(delta as u8);
    bus.write_byte(0, x_addr, new_x);
    let y_addr = x_addr.wrapping_add(1);
    let y = bus.read_byte(0, y_addr);
    cpu.set_de(0x0004);
    cpu.set_hl(y_addr);
    cpu.a = y;
    y
}

fn decomp_update_monster_sprites<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let base = cpu.hl();
    let state0 = bus.read_byte(0, base);
    cpu.a = state0;
    cpu.and8(0xFC);

    // 1. Not inflating and not crushed: either Ghosting (`mecha-ru`) or Normal Walking
    if (state0 & 0xFC) == 0 {
        cpu.cb_op(0x4E, state0);
        if (state0 & 0x02) != 0 {
            // Ghosting eyes mode (`mecha-ru`): Pooka ($28/$29) or Fygar ($2A/$2B)
            cpu.cb_op(0x46, state0);
            let spr_addr = cpu.add16(base, 0x0006);
            bus.write_word(0, 0x85F6, spr_addr);
            let anim_addr = cpu.add16(spr_addr, 0x0004);
            let anim = cpu.inc8(bus.read_byte(0, anim_addr));
            bus.write_byte(0, anim_addr, anim);
            cpu.a = anim;
            cpu.and8(0x0F);
            cpu.sub8(0x08, false, false);
            let phase = cpu.a;
            let is_fygar = (state0 & 0x01) != 0;
            let spr_code = match (is_fygar, phase < 8) {
                (false, true) => 0x29,
                (false, false) => 0x28,
                (true, true) => 0x2B,
                (true, false) => 0x2A,
            };
            bus.write_byte(0, spr_addr, spr_code);
            bus.write_byte(0, spr_addr.wrapping_add(1), 0x03);
            cpu.set_de(0x0004);
            cpu.set_hl(base);
            return;
        }

        // Normal walking mode in tunnel
        let s8401 = bus.read_byte(0, 0x8401);
        cpu.a = s8401;
        cpu.cb_op(0x7F, s8401);
        if (s8401 & 0x80) != 0 {
            cpu.set_hl(base);
            return;
        }
        let dir_addr = cpu.add16(base, 0x000E);
        cpu.set_de(0x000E);
        let dir_raw = bus.read_byte(0, dir_addr);
        cpu.a = dir_raw;
        cpu.and8(0x07);
        cpu.sub8(0x02, false, false);
        if cpu.a == 0x02 {
            bus.write_byte(0, base.wrapping_add(0x0D), 0x00);
        } else {
            cpu.sub8(0x06, false, false);
            if cpu.a == 0x06 {
                bus.write_byte(0, base.wrapping_add(0x0D), 0x02);
            }
        }

        cpu.cb_op(0x46, state0);
        if (state0 & 0x01) == 0 {
            // Pooka walking ($20 / $21, color $01)
            let spr_addr = cpu.add16(base, 0x0006);
            bus.write_word(0, 0x85F6, spr_addr);
            let anim_addr = cpu.add16(spr_addr, 0x0004);
            let anim = cpu.inc8(bus.read_byte(0, anim_addr));
            bus.write_byte(0, anim_addr, anim);
            cpu.a = anim;
            cpu.and8(0x0F);
            cpu.sub8(0x04, false, false);
            let spr_code = if cpu.a < 4 { 0x20 } else { 0x21 };
            bus.write_byte(0, spr_addr, spr_code);
            bus.write_byte(0, spr_addr.wrapping_add(1), 0x01);
        } else {
            // Fygar walking ($24 / $25, color $02) unless breathing fire (bit 6 of base+1)
            let b1 = bus.read_byte(0, base.wrapping_add(1));
            cpu.cb_op(0x76, b1);
            if (b1 & 0x40) != 0 {
                cpu.set_hl(base);
                return;
            }
            let spr_addr = cpu.add16(base.wrapping_add(1), 0x0005);
            bus.write_word(0, 0x85F6, spr_addr);
            let anim_addr = cpu.add16(spr_addr, 0x0004);
            let anim = cpu.inc8(bus.read_byte(0, anim_addr));
            bus.write_byte(0, anim_addr, anim);
            cpu.a = anim;
            cpu.and8(0x0F);
            cpu.and8(0x07);
            cpu.sub8(0x04, false, false);
            let spr_code = if cpu.a < 4 { 0x24 } else { 0x25 };
            bus.write_byte(0, spr_addr, spr_code);
            bus.write_byte(0, spr_addr.wrapping_add(1), 0x02);
        }
        cpu.set_de(0x0004);
        cpu.set_hl(base);
        return;
    }

    // 2. Crushed by falling rock (bit 7 of state0)
    if (state0 & 0x80) != 0 {
        let slot_879d = bus.read_byte(0, 0x879D);
        let slot_879e = bus.read_byte(0, 0x879E);
        if slot_879d == slot_879e {
            for addr in 0x9882..0x988A {
                bus.write_byte(0, addr, 0x00);
            }
            let s8401 = bus.read_byte(0, 0x8401) & !0x08;
            bus.write_byte(0, 0x8401, s8401);
            let s8403 = bus.read_byte(0, 0x8403) & !0x04;
            bus.write_byte(0, 0x8403, s8403);
            bus.write_byte(0, 0x863E, 0x00);
        }
        if (state0 & 0x01) == 0 {
            bus.write_byte(0, base.wrapping_add(6), 0x22);
            bus.write_byte(0, base.wrapping_add(7), 0x01);
        } else {
            bus.write_byte(0, base.wrapping_add(6), 0x26);
            bus.write_byte(0, base.wrapping_add(7), 0x02);
        }
        cpu.set_de(0x0006);
        cpu.set_hl(base);
        return;
    }

    // 3. Pump Inflation / Deflation Stages 1..4 (bits 2..5 of state0)
    bus.write_word(0, 0x85F6, base);
    let iy_spr = base.wrapping_add(6);
    let ix_col = base.wrapping_add(7);
    cpu.ix = ix_col;
    cpu.iy = iy_spr;

    let flags2_addr = base.wrapping_add(2);
    let flags2 = bus.read_byte(0, flags2_addr);
    if (flags2 & 0x01) != 0 {
        cpu.set_hl(base);
        return;
    }

    let is_fygar = (state0 & 0x01) != 0;
    bus.write_byte(0, ix_col, if is_fygar { 0x02 } else { 0x01 });

    let cocktail_p2 = (bus.read_byte(0, 0x8400) & 0x06) == 0x06;

    if (state0 & 0x04) != 0 {
        // Stage 1 inflation (16x16 sprite: $23 Pooka, $27 Fygar)
        bus.write_byte(0, iy_spr, if is_fygar { 0x27 } else { 0x23 });
        if (flags2 & 0x20) != 0 {
            bus.write_byte(0, flags2_addr, flags2 & !0x20);
            if cocktail_p2 {
                let y = decomp_shift_monster_x(cpu, bus, -8);
                bus.write_byte(0, base.wrapping_add(5), y.wrapping_sub(0x0C));
            } else {
                let y = decomp_shift_monster_x(cpu, bus, 8);
                bus.write_byte(0, base.wrapping_add(5), y.wrapping_add(0x04));
            }
        }
    } else if (state0 & 0x08) != 0 {
        // Stage 2 inflation (32x32 sprite: $80 Pooka, $83 Fygar)
        bus.write_byte(0, iy_spr, if is_fygar { 0x83 } else { 0x80 });
        if (flags2 & 0x20) != 0 {
            bus.write_byte(0, flags2_addr, flags2 & !0x20);
            let y_addr = base.wrapping_add(5);
            let y = bus.read_byte(0, y_addr).wrapping_add(4);
            bus.write_byte(0, y_addr, y);
        } else if (flags2 & 0x10) != 0 {
            bus.write_byte(0, flags2_addr, flags2 & !0x10);
            if cocktail_p2 {
                let y = decomp_shift_monster_x(cpu, bus, 8);
                bus.write_byte(0, base.wrapping_add(5), y.wrapping_add(0x0C));
            } else {
                let y = decomp_shift_monster_x(cpu, bus, -8);
                bus.write_byte(0, base.wrapping_add(5), y.wrapping_sub(0x04));
            }
        }
    } else if (state0 & 0x10) != 0 {
        // Stage 3 inflation (32x32 sprite: $81 Pooka, $84 Fygar)
        bus.write_byte(0, iy_spr, if is_fygar { 0x84 } else { 0x81 });
        if (flags2 & 0x10) != 0 {
            bus.write_byte(0, flags2_addr, flags2 & !0x10);
            let y_addr = base.wrapping_add(5);
            let y = bus.read_byte(0, y_addr).wrapping_sub(4);
            bus.write_byte(0, y_addr, y);
        }
    } else if (state0 & 0x20) != 0 {
        // Stage 4 POP! (32x32 sprite: $82 Pooka, $85 Fygar) -> clear harpoon cable
        bus.write_byte(0, iy_spr, if is_fygar { 0x85 } else { 0x82 });
        for addr in 0x9882..0x988A {
            bus.write_byte(0, addr, 0x00);
        }
    }

    cpu.set_hl(base);
}

fn decomp_pixel_xy_to_tile_addr(cpu: &mut Z80) {
    let d_shifted = cpu.d.wrapping_sub(0x11) >> 2;
    let l = (d_shifted >> 1) << 5;
    let h = d_shifted >> 4;
    let hl_col = ((h as u16) << 8) | (l as u16);
    let de_row = (cpu.e >> 3) as u16;
    let diff = hl_col.wrapping_sub(de_row);
    cpu.set_hl(0x83A0);
    cpu.set_de(diff);
    cpu.f &= !FLAG_C;
    cpu.sbc_hl(diff);
    cpu.a = h;
}

fn decomp_populate_4tile_quad<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, cpu_id: usize, ix: u16) {
    decomp_pixel_xy_to_tile_addr(cpu);
    cpu.ix = ix;
    let hl = cpu.hl();
    bus.write_word(cpu_id, ix, hl);
    let de = cpu.add16(hl, 0xFFE0);
    bus.write_word(cpu_id, ix.wrapping_add(2), de);
    let hl_plus_1 = hl.wrapping_add(1);
    bus.write_word(cpu_id, ix.wrapping_add(4), hl_plus_1);
    let de_plus_1 = de.wrapping_add(1);
    bus.write_word(cpu_id, ix.wrapping_add(6), de_plus_1);
    cpu.set_hl(hl_plus_1);
    cpu.set_de(de_plus_1);
}

fn decomp_cap_rock_tunnel_tile<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, hl: u16) {
    let above = bus.read_byte(0, hl.wrapping_sub(1));
    cpu.a = above;
    cpu.sub8(0x7F, false, false);
    if above != 0x7F {
        return;
    }
    let cur = bus.read_byte(0, hl);
    cpu.a = cur;
    match cur {
        0x00 | 0x04 | 0x0A | 0x0B => bus.write_byte(0, hl, 0x02),
        0x01 | 0x05 | 0x06 | 0x07 => bus.write_byte(0, hl, 0x03),
        0x08 | 0x09 => bus.write_byte(0, hl, 0x0C),
        _ => {}
    }
}

fn decomp_set_bcd_threshold<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, ix: u16, hl: u16) {
    cpu.ix = ix;
    bus.write_byte(0, ix, 0x00);
    cpu.a = bus.read_byte(0, hl);
    for _ in 0..4 {
        let a = cpu.a;
        cpu.add8(a, false);
    }
    bus.write_byte(0, ix.wrapping_add(1), cpu.a);
    let hl_next = hl.wrapping_add(1);
    cpu.set_hl(hl_next);
    let hi_byte = bus.read_byte(0, hl_next);
    cpu.a = hi_byte;
    bus.write_byte(0, ix.wrapping_add(2), hi_byte);
}

fn decomp_draw_player_lives_hud<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    for i in 0..0x14u16 {
        bus.write_byte(0, 0x800C + i, 0x0C);
        bus.write_byte(0, 0x6830, 0x0C);
        bus.write_byte(0, 0x802C + i, 0x0C);
    }
    let lives = bus.read_byte(0, 0x840C);
    cpu.a = lives;
    cpu.and8(lives);
    if lives == 0 {
        cpu.set_hl(0x8020);
        cpu.set_de(0x8040);
        cpu.b = 0;
        return;
    }
    cpu.sub8(0x0A, false, false);
    let count = lives.min(9);
    let mut hl = 0x801Du16;
    for _ in 0..count {
        // Draw 2x2 life icon (4 tiles of $7F with their HUD CLUT colors)
        bus.write_byte(0, hl, 0x7F);
        bus.write_byte(0, hl.wrapping_sub(1), 0x7F);
        bus.write_byte(0, hl.wrapping_add(0x1F), 0x7F);
        bus.write_byte(0, hl.wrapping_add(0x20), 0x7F);
        hl = hl.wrapping_sub(2);
    }
    cpu.set_hl(hl);
    cpu.set_de(0x0020);
    cpu.b = 0;
    cpu.a = 0x7F;
    cpu.f &= !(FLAG_N | FLAG_C);
}

fn decomp_sync_and_clamp_round<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let status_8400 = bus.read_byte(0, 0x8400);
    let is_player2 = (status_8400 & 0x02) != 0;
    let r = if is_player2 {
        bus.read_byte(0, 0x840E)
    } else {
        bus.read_byte(0, 0x840D)
    };
    bus.write_byte(0, 0x840F, r);
    bus.write_byte(0, 0x8410, r);
    bus.write_byte(0, 0x8627, r);
    cpu.a = r;
    cpu.sub8(0x0F, false, false);
    if r >= 0x0F {
        bus.write_byte(0, 0x840F, 0x0F);
        bus.write_byte(0, 0x8410, 0x0F);
        cpu.a = 0x0F;
        cpu.set_hl(0x8410);
    } else {
        cpu.set_hl(0x840F);
    }
}

// =============================================================================
// CPU 1 (Sub / Player & Sprite AI Z80) — Human Decompiled Helpers
// =============================================================================

fn decomp_cpu1_copy_3_flower_sprites<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let mut hl = cpu.hl();
    for _ in 0..3 {
        let code = bus.read_byte(1, cpu.iy.wrapping_add(3));
        bus.write_byte(1, cpu.ix, code);
        let y_pos = bus.read_byte(1, cpu.iy);
        cpu.a = y_pos;
        bus.write_byte(1, hl, y_pos);
        hl = hl.wrapping_add(2);
        cpu.iy = cpu.iy.wrapping_add(1);
        cpu.ix = cpu.ix.wrapping_add(2);
    }
    cpu.set_hl(hl);
    cpu.b = 0;
}

fn decomp_cpu1_draw_round_flower_indicators<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, cpu.a);
    cpu.a = if (cpu.a & 0x02) != 0 {
        bus.read_byte(1, 0x840E)
    } else {
        bus.read_byte(1, 0x840D)
    };
    cpu.set_de(0x0001);
    loop {
        cpu.and8(cpu.a);
        if cpu.a == 0 {
            break;
        }
        let c = cpu.a & 1;
        cpu.a >>= 1;
        cpu.f = (cpu.f & !FLAG_C) | (if c != 0 { FLAG_C } else { 0 });
        cpu.b = cpu.a;
        if c != 0 {
            cpu.a = cpu.d;
            cpu.add8(cpu.e, false);
            cpu.daa();
            cpu.d = cpu.a;
        }
        cpu.a = cpu.e;
        cpu.add8(cpu.a, false);
        cpu.daa();
        cpu.e = cpu.a;
        cpu.a = cpu.b;
    }

    cpu.set_hl(0x98D5);
    cpu.ix = 0x9855;
    cpu.iy = 0x0428;
    decomp_cpu1_copy_3_flower_sprites(cpu, bus);

    cpu.ix = 0x9854;
    cpu.set_hl(0x98D4);
    cpu.a = cpu.d;
    cpu.and8(0x0F);
    cpu.e = cpu.a;
    cpu.a = cpu.d;
    cpu.and8(0xF0);
    cpu.c = cpu.a;
    cpu.d = 0x00;
    cpu.sub8(0x70, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.e = 0x08;
        cpu.c = 0x60;
    }
    cpu.a = cpu.c;
    cpu.iy = 0x043A;
    cpu.sub8(0x00, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.iy = 0x0452;
        cpu.sub8(0x10, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.iy = 0x044C;
            cpu.sub8(0x20, false, false);
            if (cpu.f & FLAG_Z) == 0 {
                cpu.iy = 0x0446;
                cpu.sub8(0x30, false, false);
                if (cpu.f & FLAG_Z) == 0 {
                    cpu.iy = 0x0440;
                    cpu.sub8(0x40, false, false);
                    if (cpu.f & FLAG_Z) == 0 {
                        cpu.iy = 0x0434;
                        cpu.sub8(0x50, false, false);
                        if (cpu.f & FLAG_Z) == 0 {
                            cpu.iy = 0x042E;
                        }
                    }
                }
            }
        }
    }
    decomp_cpu1_copy_3_flower_sprites(cpu, bus);

    let mut hl = 0x988Au16;
    cpu.a = 0x61;
    for _ in 0..9 {
        bus.write_byte(1, hl, cpu.a);
        bus.write_byte(1, hl.wrapping_add(1), 0x08);
        hl = hl.wrapping_add(2);
        cpu.add8(0x10, false);
    }

    let mut rom_hl = cpu.add16(0x0416, cpu.de());
    let mut dst_de = 0x980Au16;
    for _ in 0..9 {
        let code = bus.read_byte(1, rom_hl);
        bus.write_byte(1, dst_de, code);
        rom_hl = rom_hl.wrapping_add(1);
        dst_de = dst_de.wrapping_add(1);
        cpu.a = 0x19;
        bus.write_byte(1, dst_de, 0x19);
        dst_de = dst_de.wrapping_add(1);
    }
    cpu.set_hl(rom_hl);
    cpu.set_de(dst_de);
    cpu.b = 0;

    cpu.a = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, cpu.a);
    if (cpu.a & 0x02) == 0 {
        return;
    }
    cpu.cb_op(0x57, cpu.a);
    if (cpu.a & 0x04) == 0 {
        return;
    }
    let mut hl = 0x98D4u16;
    for _ in 0..6 {
        cpu.a = 0x10;
        let cur = bus.read_byte(1, hl);
        cpu.add8(cur, false);
        bus.write_byte(1, hl, cpu.a);
        hl = hl.wrapping_add(1);
    }
    cpu.set_hl(hl);
    cpu.b = 0;
}

fn decomp_cpu1_update_digdug_sprite_and_flip<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x8401);
    let s8401 = bus.read_byte(1, 0x8401);
    cpu.cb_op(0x5E, s8401);
    if (s8401 & 0x08) != 0 {
        cpu.a = bus.read_byte(1, 0x85AE);
        cpu.set_hl(0x9822);
        cpu.set_de(0x9922);
        cpu.and8(0x0F);
        if cpu.a == 0x00 {
            bus.write_byte(1, 0x9822, 0x10);
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9922, 0x00);
            return;
        }
        cpu.sub8(0x04, false, false);
        if cpu.a == 0x04 {
            bus.write_byte(1, 0x9822, 0x10);
            cpu.a = 0x01;
            bus.write_byte(1, 0x9922, 0x01);
            return;
        }
        cpu.sub8(0x02, false, false);
        if cpu.a == 0x02 {
            bus.write_byte(1, 0x9822, 0x11);
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9922, 0x00);
            return;
        }
        cpu.sub8(0x06, false, false);
        if cpu.a == 0x06 {
            bus.write_byte(1, 0x9822, 0x11);
            cpu.a = 0x02;
            bus.write_byte(1, 0x9922, 0x02);
        }
        return;
    }

    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.set_hl(0x9822);
    cpu.set_de(0x9922);
    cpu.set_bc(0x0000);
    cpu.and8(0x0F);
    if cpu.a != 0x00 {
        cpu.set_bc(0x0001);
        cpu.sub8(0x04, false, false);
        if cpu.a != 0x04 {
            cpu.set_bc(0x0200);
            cpu.sub8(0x02, false, false);
            if cpu.a != 0x02 {
                cpu.set_bc(0x0202);
                cpu.sub8(0x06, false, false);
                if cpu.a != 0x06 {
                    return;
                }
            }
        }
    }
    bus.write_byte(1, 0x9822, cpu.b);
    cpu.a = cpu.c;
    bus.write_byte(1, 0x9922, cpu.c);
    cpu.a = bus.read_byte(1, 0x8401);
    cpu.cb_op(0x67, cpu.a);
    if (cpu.a & 0x10) != 0 {
        for _ in 0..4 {
            let v = bus.read_byte(1, 0x9822);
            let res = cpu.inc8(v);
            bus.write_byte(1, 0x9822, res);
        }
    }
}

fn decomp_cpu1_check_playfield_bounds(cpu: &mut Z80) {
    if cpu.d < 0x11 || cpu.d >= 0xE2 || cpu.e < 0x08 || cpu.e >= 0xF1 {
        cpu.c |= 0x02;
    } else {
        cpu.c &= !0x02;
    }
}

fn decomp_cpu1_setup_veggie_for_round<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, cpu.a);
    let round_hl = if (cpu.a & 0x02) != 0 { 0x840E } else { 0x840D };
    cpu.a = bus.read_byte(1, round_hl);
    cpu.set_hl(0x87E8);
    cpu.sub8(0x12, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.a = 0x12;
    }
    bus.write_byte(1, 0x87E8, cpu.a);
    cpu.sub8(0x01, false, true);
    cpu.add8(cpu.a, false);
    cpu.add8(cpu.a, false);
    cpu.e = cpu.a;
    cpu.d = 0x00;
    let tbl_hl = cpu.add16(0x1B57, cpu.de());
    let b0 = bus.read_byte(1, tbl_hl);
    bus.write_byte(1, 0x981E, b0);
    let b1 = bus.read_byte(1, tbl_hl.wrapping_add(1));
    bus.write_byte(1, 0x981F, b1);
    let b2 = bus.read_byte(1, tbl_hl.wrapping_add(2));
    bus.write_byte(1, 0x87E7, b2);
    let b3 = bus.read_byte(1, tbl_hl.wrapping_add(3));
    cpu.set_hl(tbl_hl.wrapping_add(3));
    cpu.a = b3;
    bus.write_byte(1, 0x87E6, b3);
}

fn decomp_cpu1_update_escape_exit_and_veggie<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x896C);
    let s896c = bus.read_byte(1, 0x896C);
    cpu.cb_op(0x4E, s896c);
    if (s896c & 0x02) == 0 {
        cpu.cb_op(0x46, s896c);
        let mut check_30_timer = true;
        if (s896c & 0x01) != 0 {
            bus.write_byte(1, 0x83A1, 0x7E);
            cpu.set_hl(0x83A2);
            cpu.a = bus.read_byte(1, 0x83A2);
            cpu.sub8(0x0E, false, false);
            let is_cap = if cpu.a == 0x0E {
                true
            } else {
                cpu.sub8(0x0F, false, false);
                cpu.a == 0x0F
            };
            if is_cap {
                bus.write_byte(1, 0x83A2, 0x7E);
            }
            cpu.set_de(0x0020);
            let hl1 = cpu.add16(0x83A2, 0x0020);
            cpu.a = 0x7E;
            bus.write_byte(1, hl1, 0x7E);
            bus.write_byte(1, hl1.wrapping_sub(1), 0x7E);
            let hl2 = cpu.add16(hl1.wrapping_sub(1), 0x0020);
            bus.write_byte(1, hl2, 0x7E);
            bus.write_byte(1, hl2.wrapping_add(1), 0x7E);
            bus.write_byte(1, 0x9A86, 0x01);
            cpu.set_hl(0x896C);
            let v = bus.read_byte(1, 0x896C) | 0x02;
            bus.write_byte(1, 0x896C, v);
        } else {
            cpu.set_hl(0x87F0);
            let s87f0 = bus.read_byte(1, 0x87F0);
            cpu.cb_op(0x46, s87f0);
            if (s87f0 & 0x01) == 0 {
                bus.write_byte(1, 0x896D, 0x00);
                let mut hl = 0x8528u16;
                cpu.c = 0x00;
                cpu.b = 0x08;
                bus.write_byte(1, 0x6830, cpu.a);
                cpu.set_de(0x0010);
                for _ in 0..8 {
                    let v = bus.read_byte(1, hl);
                    cpu.cb_op(0x7E, v);
                    if (v & 0x80) != 0 {
                        cpu.c = cpu.inc8(cpu.c);
                    }
                    hl = cpu.add16(hl, 0x0010);
                    bus.write_byte(1, 0x6830, cpu.a);
                }
                cpu.set_hl(hl);
                cpu.b = 0;
                cpu.a = cpu.c;
                cpu.sub8(0x07, false, false);
                if cpu.c == 0x07 {
                    cpu.set_hl(0x87F0);
                    let v = bus.read_byte(1, 0x87F0) | 0x01;
                    bus.write_byte(1, 0x87F0, v);
                    check_30_timer = false;
                }
            }
        }
        if check_30_timer {
            cpu.a = bus.read_byte(1, 0x87F0);
            cpu.cb_op(0x47, cpu.a);
            if (cpu.a & 0x01) != 0 {
                cpu.set_hl(0x896D);
                cpu.a = bus.read_byte(1, 0x896D);
                cpu.sub8(0x1E, false, false);
                if cpu.a == 0x1E {
                    cpu.set_hl(0x896C);
                    let v = bus.read_byte(1, 0x896C) | 0x01;
                    bus.write_byte(1, 0x896C, v);
                } else {
                    let next_v = cpu.inc8(cpu.a);
                    bus.write_byte(1, 0x896D, next_v);
                }
            }
        }
    }

    // Load active player's veggie drop count ($87EB) and timer ($87ED)
    cpu.a = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, cpu.a);
    if (cpu.a & 0x02) == 0 {
        let t = bus.read_word(1, 0x87D4);
        cpu.set_hl(t);
        bus.write_word(1, 0x87ED, t);
        cpu.a = bus.read_byte(1, 0x87DA);
        bus.write_byte(1, 0x87EB, cpu.a);
    } else {
        cpu.a = bus.read_byte(1, 0x87D8);
        bus.write_byte(1, 0x87EB, cpu.a);
        let t = bus.read_word(1, 0x87D2);
        cpu.set_hl(t);
        bus.write_word(1, 0x87ED, t);
    }

    cpu.a = bus.read_byte(1, 0x8799);
    cpu.set_hl(0x87EB);
    cpu.cb_op(0x47, cpu.a);
    if (cpu.a & 0x01) != 0 {
        cpu.a &= !0x01;
        bus.write_byte(1, 0x8799, cpu.a);
        let v = cpu.inc8(bus.read_byte(1, 0x87EB));
        bus.write_byte(1, 0x87EB, v);
    }

    cpu.a = bus.read_byte(1, 0x87EB);
    cpu.sub8(0x02, false, false);
    if cpu.a == 0x02 {
        let s87f0 = bus.read_byte(1, 0x87F0) | 0x20;
        bus.write_byte(1, 0x87F0, s87f0);
        decomp_cpu1_setup_veggie_for_round(cpu, bus);
        bus.write_byte(1, 0x87EF, 0x46);
        let s87f0_clear = bus.read_byte(1, 0x87F0) & !0x40;
        bus.write_byte(1, 0x87F0, s87f0_clear);
        bus.write_byte(1, 0x989E, 0x71);
        bus.write_byte(1, 0x989F, 0x80);
        cpu.set_hl(0x87EB);
        let v = cpu.inc8(bus.read_byte(1, 0x87EB));
        bus.write_byte(1, 0x87EB, v);
    } else {
        let mut goto_save = false;
        cpu.a = bus.read_byte(1, 0x87F0);
        cpu.cb_op(0x6F, cpu.a);
        if (cpu.a & 0x20) != 0 {
            let t = bus.read_word(1, 0x87ED).wrapping_add(1);
            cpu.set_hl(t);
            bus.write_word(1, 0x87ED, t);
            cpu.set_de(0x0258);
            cpu.and8(cpu.a);
            cpu.sbc_hl(0x0258);
            if (cpu.f & FLAG_Z) != 0 {
                bus.write_byte(1, 0x989E, 0x00);
                bus.write_byte(1, 0x989F, 0xE0);
                cpu.set_hl(0x87F0);
                let s = bus.read_byte(1, 0x87F0) & !0x20;
                bus.write_byte(1, 0x87F0, s);
                goto_save = true;
            }
        }
        if !goto_save {
            cpu.a = bus.read_byte(1, 0x8401);
            cpu.cb_op(0x7F, cpu.a);
            if (cpu.a & 0x80) != 0 {
                return;
            }
            cpu.a = bus.read_byte(1, 0x8400);
            cpu.cb_op(0x6F, cpu.a);
            if (cpu.a & 0x20) == 0 {
                cpu.a = bus.read_byte(1, 0x8657);
                cpu.cb_op(0x4F, cpu.a);
                if (cpu.a & 0x02) == 0 {
                    return;
                }
            }
            cpu.set_hl(0x989E);
            cpu.set_de(0x98A2);
            let dd_x = bus.read_byte(1, 0x98A2);
            let veg_x = bus.read_byte(1, 0x989E);
            cpu.a = dd_x;
            cpu.sub8(veg_x, false, false);
            if dd_x == veg_x {
                cpu.set_hl(0x989F);
                cpu.set_de(0x98A3);
                let dd_y = bus.read_byte(1, 0x98A3);
                let veg_y = bus.read_byte(1, 0x989F);
                cpu.a = dd_y;
                cpu.sub8(veg_y, false, false);
                if dd_y == veg_y {
                    cpu.a = bus.read_byte(1, 0x87F0) | 0x40;
                    bus.write_byte(1, 0x87F0, cpu.a);
                }
            }
            cpu.a = bus.read_byte(1, 0x87F0);
            cpu.cb_op(0x77, cpu.a);
            if (cpu.a & 0x40) != 0 {
                cpu.set_hl(0x87EF);
                cpu.a = bus.read_byte(1, 0x87EF);
                cpu.and8(cpu.a);
                if cpu.a != 0 {
                    let rem = cpu.dec8(cpu.a);
                    bus.write_byte(1, 0x87EF, rem);
                    cpu.a = rem;
                    cpu.sub8(0x44, false, false);
                    if rem == 0x44 {
                        bus.write_byte(1, 0x9A93, 0x01);
                        let s87a7 = bus.read_byte(1, 0x87A7) | 0x01;
                        bus.write_byte(1, 0x87A7, s87a7);
                        let score_code = bus.read_byte(1, 0x87E6);
                        bus.write_byte(1, 0x9876, score_code);
                        cpu.xor8(cpu.a);
                        bus.write_byte(1, 0x9877, 0x00);
                        bus.write_byte(1, 0x987E, 0x64);
                        bus.write_byte(1, 0x987F, 0x00);
                        cpu.a = bus.read_byte(1, 0x989E);
                        cpu.sub8(0x08, false, true);
                        bus.write_byte(1, 0x98F6, cpu.a);
                        cpu.add8(0x10, false);
                        bus.write_byte(1, 0x98FE, cpu.a);
                        let vy = bus.read_byte(1, 0x989F);
                        cpu.a = vy;
                        bus.write_byte(1, 0x98F7, vy);
                        bus.write_byte(1, 0x98FF, vy);
                        bus.write_byte(1, 0x989E, 0x00);
                        bus.write_byte(1, 0x989F, 0xE0);
                    } else {
                        cpu.sub8(0x02, false, false);
                        if rem == 0x02 {
                            bus.write_byte(1, 0x98F6, 0x00);
                            bus.write_byte(1, 0x98F7, 0x00);
                            bus.write_byte(1, 0x98FE, 0x00);
                            bus.write_byte(1, 0x98FF, 0x00);
                        } else {
                            cpu.sub8(0x01, false, false);
                            if rem == 0x01 {
                                bus.write_byte(1, 0x87EF, 0x00);
                                let s = bus.read_byte(1, 0x87F0) & !0x40;
                                bus.write_byte(1, 0x87F0, s);
                            }
                        }
                    }
                }
            }
        }
    }

    // Save active player's veggie state back to P1 ($87DA/$87D4) or P2 ($87D8/$87D2)
    let s8400 = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, s8400);
    let final_t = bus.read_word(1, 0x87ED);
    cpu.set_hl(final_t);
    cpu.a = bus.read_byte(1, 0x87EB);
    if (s8400 & 0x02) == 0 {
        bus.write_byte(1, 0x87DA, cpu.a);
        bus.write_word(1, 0x87D4, final_t);
    } else {
        bus.write_byte(1, 0x87D8, cpu.a);
        bus.write_word(1, 0x87D2, final_t);
    }
}

// =============================================================================
// CPU 2 (Sound Z80, 4 KB ROM $0000..$0FFF) — 100% Human Decompiled Engine
// =============================================================================

#[inline]
fn cpu2_rst_08_inline(cpu: &mut Z80) {
    let a0 = cpu.a;
    cpu.add8(a0, false);
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
    let l = cpu.l;
    cpu.add8(l, false);
    cpu.l = cpu.a;
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
}

#[inline]
fn cpu2_rst_10_inline(cpu: &mut Z80) {
    let l = cpu.l;
    cpu.add8(l, false);
    cpu.l = cpu.a;
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
}

/// CPU 2 `$040D..$04D4`: Indirect completion callback jump table and handlers
/// invoked when a music track (`sub_034E`) or sound effect (`sub_0391`) reaches `$FF`.
fn decomp_cpu2_completion_callback<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let s34 = bus.read_byte(2, 0x9B34);
    cpu.set_hl(0x040D);
    cpu.a = s34;
    cpu2_rst_08_inline(cpu);
    let tbl_addr = cpu.hl();
    let lo = roms.cpu2[(tbl_addr & 0x0FFF) as usize] as u16;
    let hi = roms.cpu2[(tbl_addr.wrapping_add(1) & 0x0FFF) as usize] as u16;
    let target = (hi << 8) | lo;
    cpu.set_de(tbl_addr.wrapping_add(1));
    cpu.set_hl(target);

    match s34 {
        0x01 => {
            cpu.xor8(cpu.a);
            bus.write_byte(2, 0x9A94, 0);
            bus.write_byte(2, 0x9AAA, 0);
        }
        0x0C => {
            cpu.xor8(cpu.a);
            bus.write_byte(2, 0x9A8D, 0);
            bus.write_byte(2, 0x9AA3, 0);
        }
        0x04 => {
            cpu.xor8(cpu.a);
            for addr in [0x9A86, 0x9A89, 0x9A9C, 0x9A9F, 0x9A93, 0x9AA9] {
                bus.write_byte(2, addr, 0);
            }
            for i in 0..9u16 {
                bus.write_byte(2, 0x9A8A + i, 0);
                bus.write_byte(2, 0x9AA0 + i, 0);
            }
            cpu.set_hl(0x9AA8);
            cpu.set_de(0x9AA9);
            cpu.set_bc(0x0000);
            cpu.f &= FLAG_S | FLAG_Z | FLAG_C;
        }
        0x05 | 0x06 | 0x07 | 0x08 | 0x09 => {
            if s34 == 0x05 || s34 == 0x08 {
                for addr in [0x9A86, 0x9A89, 0x9A9C, 0x9A9F, 0x9A93, 0x9AA9] {
                    bus.write_byte(2, addr, 0);
                }
                bus.write_byte(2, 0x9B1B, 0x10);
                cpu.xor8(cpu.a);
                bus.write_byte(2, 0x9AA7, 0);
            } else if s34 == 0x06 {
                bus.write_byte(2, 0x9B1B, 0x0A);
                cpu.xor8(cpu.a);
                bus.write_byte(2, 0x9AA7, 0);
            } else if s34 == 0x09 {
                bus.write_byte(2, 0x9B1B, 0x0C);
                cpu.xor8(cpu.a);
                bus.write_byte(2, 0x9AA7, 0);
            } else {
                cpu.xor8(cpu.a);
                bus.write_byte(2, 0x9A93, 0);
                bus.write_byte(2, 0x9AA9, 0);
            }
            for i in 0..9u16 {
                bus.write_byte(2, 0x9A8A + i, 0);
                bus.write_byte(2, 0x9AA0 + i, 0);
            }
            cpu.set_hl(0x9AA8);
            cpu.set_de(0x9AA9);
            cpu.set_bc(0x0000);
            cpu.f &= FLAG_S | FLAG_Z | FLAG_C;
        }
        0x0E => {
            for i in 0..4u16 {
                bus.write_byte(2, 0x9A8A + i, 0);
                bus.write_byte(2, 0x9AA0 + i, 0);
            }
            cpu.set_hl(0x9AA3);
            cpu.set_de(0x9AA4);
            cpu.set_bc(0x0000);
            cpu.f &= FLAG_S | FLAG_Z | FLAG_C;
        }
        0x13 => {
            for i in 0..7u16 {
                bus.write_byte(2, 0x9A8A + i, 0);
                bus.write_byte(2, 0x9AA0 + i, 0);
            }
            cpu.set_hl(0x9AA6);
            cpu.set_de(0x9AA7);
            cpu.set_bc(0x0000);
            cpu.f &= FLAG_S | FLAG_Z | FLAG_C;
        }
        _ => {}
    }
}

/// CPU 2 `$04D5..$0630`: `sub_04D5` (`cpu2_synthesize_voice`)
pub fn decomp_cpu2_synthesize_voice<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let s35 = bus.read_byte(2, 0x9B35);

    let tick_addr = 0x9AACu16.wrapping_add(s35 as u16);
    let tick_val = cpu.inc8(bus.read_byte(2, tick_addr));
    bus.write_byte(2, tick_addr, tick_val);

    let ptr_addr = 0x0700u16.wrapping_add((s35 as u16) * 2);
    let stream_base = (roms.cpu2[(ptr_addr & 0x0FFF) as usize] as u16)
        | ((roms.cpu2[(ptr_addr.wrapping_add(1) & 0x0FFF) as usize] as u16) << 8);

    let pos_addr = 0x9ADBu16.wrapping_add(s35 as u16);
    let pos = bus.read_byte(2, pos_addr);
    cpu.set_de(pos_addr);
    cpu.set_hl(stream_base);
    cpu.a = pos;
    cpu2_rst_10_inline(cpu);
    let note_ptr = cpu.hl();
    bus.write_word(2, 0x9B39, note_ptr);

    let note_byte = roms.cpu2[(note_ptr & 0x0FFF) as usize];
    cpu.a = cpu.inc8(note_byte);
    if note_byte == 0xFF {
        let ch = bus.read_byte(2, 0x9B37);
        cpu.a = ch;
        cpu.and8(ch);
        let vol_addr = if ch == 0 {
            0x9B25u16
        } else {
            cpu.a = cpu.dec8(ch);
            if cpu.a == 0 {
                0x9B2Au16
            } else {
                0x9B2Fu16
            }
        };
        cpu.set_hl(vol_addr);
        bus.write_byte(2, vol_addr, 0x00);
        cpu.a = 0x01;
        bus.write_byte(2, 0x9B38, 0x01);
        return;
    }

    let tune_sel = roms.cpu2[(0x07BD + s35 as usize) & 0x0FFF];
    let tune_table = match tune_sel {
        0 => 0x0631u16,
        1 => 0x064Bu16,
        _ => 0x0665u16,
    };

    let semitone = (note_byte >> 4) & 0x0F;
    let octave = note_byte & 0x0F;
    let freq_addr = tune_table.wrapping_add((semitone as u16) * 2);
    let mut freq = (roms.cpu2[(freq_addr & 0x0FFF) as usize] as u16)
        | ((roms.cpu2[(freq_addr.wrapping_add(1) & 0x0FFF) as usize] as u16) << 8);
    if octave > 0 {
        freq >>= octave;
    }
    let freq_c = (freq & 0xFF) as u8;
    let freq_b = (freq >> 8) as u8;

    let ch = bus.read_byte(2, 0x9B37);
    let freq_base = match ch {
        0 => 0x9B21u16,
        1 => 0x9B26u16,
        _ => 0x9B2Bu16,
    };
    bus.write_byte(2, freq_base, freq_c);
    bus.write_byte(2, freq_base + 1, freq_c.rotate_right(4));
    bus.write_byte(2, freq_base + 2, freq_b);
    bus.write_byte(2, freq_base + 3, freq_b.rotate_right(4));

    let vol_addr = match ch {
        0 => 0x9B25u16,
        1 => 0x9B2Au16,
        _ => 0x9B2Fu16,
    };
    let vol = if note_byte == 0xC0 {
        0x00
    } else {
        let a_quirk = 0xFCu8.wrapping_add(s35);
        let cur_tick = bus.read_byte(2, tick_addr);
        let mut env_vol = None;
        if a_quirk == 0 {
            // s35 == 4 -> jumps directly to $05A0
        } else if a_quirk == 1 {
            if cur_tick < 6 {
                env_vol = Some(cur_tick.wrapping_add(cur_tick));
            }
        } else if cur_tick < 6 {
            env_vol = Some(!cur_tick);
        }

        if let Some(v) = env_vol {
            v
        } else {
            let decay_start = roms.cpu2[(0x0826 + s35 as usize) & 0x0FFF];
            if decay_start == 0 || cur_tick < decay_start {
                0x0A
            } else {
                let diff = cur_tick.wrapping_sub(decay_start);
                if diff >= 0x0A {
                    0x00
                } else {
                    0x0Au8.wrapping_sub(diff)
                }
            }
        }
    };
    bus.write_byte(2, vol_addr, vol);

    let wave = roms.cpu2[(0x0793 + s35 as usize) & 0x0FFF];
    bus.write_byte(2, 0x9B30u16.wrapping_add(ch as u16), wave);
    cpu.c = freq_c.wrapping_sub(1);

    let s34 = bus.read_byte(2, 0x9B34);
    let tempo = bus.read_byte(2, 0x9B0Au16.wrapping_add(s34 as u16));
    let dur_mult = roms.cpu2[(note_ptr.wrapping_add(1) & 0x0FFF) as usize];
    let prod = (tempo as u16).wrapping_mul(dur_mult as u16);
    let target_ticks = (prod & 0xFF) as u8;

    cpu.set_de((dur_mult as u16) << 8);
    cpu.b = target_ticks;

    cpu.set_hl(0x9AAC);
    cpu.a = s35;
    cpu2_rst_10_inline(cpu);
    let cur_tick = bus.read_byte(2, tick_addr);
    cpu.a = target_ticks;
    cpu.sub8(cur_tick, false, false);
    if target_ticks != cur_tick {
        return;
    }

    let next_pos = bus.read_byte(2, pos_addr).wrapping_add(2);
    bus.write_byte(2, pos_addr, next_pos);

    cpu.set_hl(0x9AAC);
    cpu.a = s35;
    cpu2_rst_10_inline(cpu);
    bus.write_byte(2, tick_addr, 0x00);
}

fn decomp_cpu2_step_music_voices<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    loop {
        decomp_cpu2_synthesize_voice(cpu, bus, roms);
        cpu.set_hl(0x9B36);
        let rem = cpu.dec8(bus.read_byte(2, 0x9B36));
        bus.write_byte(2, 0x9B36, rem);
        if rem == 0 {
            break;
        }
        let s35 = cpu.inc8(bus.read_byte(2, 0x9B35));
        bus.write_byte(2, 0x9B35, s35);
        let s37 = cpu.inc8(bus.read_byte(2, 0x9B37));
        bus.write_byte(2, 0x9B37, s37);
    }

    let done = bus.read_byte(2, 0x9B38);
    cpu.a = done;
    cpu.and8(done);
    if done == 0 {
        return;
    }

    cpu.xor8(cpu.a);
    bus.write_byte(2, 0x9B38, 0);
    let s34 = bus.read_byte(2, 0x9B34);
    bus.write_byte(2, 0x9A96u16.wrapping_add(s34 as u16), 0);
    decomp_cpu2_completion_callback(cpu, bus, roms);
}

pub fn decomp_cpu2_init_music_track<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let s34 = bus.read_byte(2, 0x9B34);
    let active_addr = 0x9A96u16.wrapping_add(s34 as u16);
    let act = cpu.inc8(bus.read_byte(2, active_addr));
    bus.write_byte(2, active_addr, act);

    let desc_off = (0x0754 + (s34 as usize) * 3) & 0x0FFF;
    let s35 = roms.cpu2[desc_off];
    let s36 = roms.cpu2[desc_off + 1];
    let s37 = roms.cpu2[desc_off + 2];
    bus.write_byte(2, 0x9B35, s35);
    bus.write_byte(2, 0x9B36, s36);
    bus.write_byte(2, 0x9B37, s37);

    for i in 0..(s36 as u16) {
        bus.write_byte(2, 0x9ADBu16.wrapping_add(s35 as u16).wrapping_add(i), 0);
        bus.write_byte(2, 0x9AACu16.wrapping_add(s35 as u16).wrapping_add(i), 0);
    }

    decomp_cpu2_step_music_voices(cpu, bus, roms);
}

pub fn decomp_cpu2_step_music_track<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let s34 = bus.read_byte(2, 0x9B34);
    let desc_off = (0x0754 + (s34 as usize) * 3) & 0x0FFF;
    bus.write_byte(2, 0x9B35, roms.cpu2[desc_off]);
    bus.write_byte(2, 0x9B36, roms.cpu2[desc_off + 1]);
    bus.write_byte(2, 0x9B37, roms.cpu2[desc_off + 2]);

    decomp_cpu2_step_music_voices(cpu, bus, roms);
}

pub fn decomp_cpu2_step_sfx_track<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let s34 = bus.read_byte(2, 0x9B34);
    let desc_off = (0x0754 + (s34 as usize) * 3) & 0x0FFF;
    let s35 = roms.cpu2[desc_off];
    let s36 = roms.cpu2[desc_off + 1];
    let s37 = roms.cpu2[desc_off + 2];
    bus.write_byte(2, 0x9B35, s35);
    bus.write_byte(2, 0x9B36, s36);
    bus.write_byte(2, 0x9B37, s37);

    let active_addr = 0x9A96u16.wrapping_add(s34 as u16);
    let is_active = bus.read_byte(2, active_addr);
    if is_active == 0 {
        bus.write_byte(2, active_addr, 1);
        for i in 0..(s36 as u16) {
            bus.write_byte(2, 0x9ADBu16.wrapping_add(s35 as u16).wrapping_add(i), 0);
            bus.write_byte(2, 0x9AACu16.wrapping_add(s35 as u16).wrapping_add(i), 0);
        }
    }

    loop {
        decomp_cpu2_synthesize_voice(cpu, bus, roms);
        cpu.set_hl(0x9B36);
        let rem = cpu.dec8(bus.read_byte(2, 0x9B36));
        bus.write_byte(2, 0x9B36, rem);
        if rem == 0 {
            break;
        }
        let next_35 = cpu.inc8(bus.read_byte(2, 0x9B35));
        bus.write_byte(2, 0x9B35, next_35);
        let next_37 = cpu.inc8(bus.read_byte(2, 0x9B37));
        bus.write_byte(2, 0x9B37, next_37);
    }

    let done = bus.read_byte(2, 0x9B38);
    cpu.a = done;
    cpu.and8(done);
    if done == 0 {
        return;
    }

    cpu.xor8(cpu.a);
    bus.write_byte(2, 0x9B38, 0);
    let s34 = bus.read_byte(2, 0x9B34);
    bus.write_byte(2, 0x9A96u16.wrapping_add(s34 as u16), 0);

    let req_addr = 0x9A80u16.wrapping_add(s34 as u16);
    cpu.set_hl(req_addr);
    cpu.a = s34;
    cpu.and8(s34);
    if s34 == 0 || s34 == 0x14 {
        if s34 == 0x14 {
            cpu.sub8(0x14, false, false);
        }
        let v = cpu.dec8(bus.read_byte(2, req_addr));
        bus.write_byte(2, req_addr, v);
        return;
    }

    bus.write_byte(2, req_addr, 0);
    decomp_cpu2_completion_callback(cpu, bus, roms);
}

pub fn decomp_cpu2_nmi_audio_tick<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let saved_af = cpu.af();
    let saved_hl = cpu.hl();
    cpu.push(bus, saved_af);
    cpu.push(bus, saved_hl);

    let busy = bus.read_byte(2, 0x9B3C);
    if busy != 0 {
        bus.write_byte(2, 0x9B3C, 0);
        let hl = cpu.pop(bus);
        let af = cpu.pop(bus);
        cpu.set_hl(hl);
        cpu.set_af(af);
        cpu.iff1 = cpu.iff2;
        cpu.ret_subroutine(bus);
        cpu.total_cycles += 78;
        return;
    }

    bus.write_byte(2, 0x9B3C, 1);
    bus.write_byte(2, 0x6822, 1);
    bus.write_byte(2, 0x6822, 0);

    for addr in 0x9B20..0x9B30u16 {
        bus.write_byte(2, addr, 0);
    }

    let attract = (bus.read_byte(2, 0x8657) & 0x02) != 0;
    let mut skip_to_flush = false;
    let mut skip_to_02e7 = false;

    if attract {
        if (bus.read_byte(2, 0x87CF) & 0x10) != 0 {
            for addr in 0x9A80..0x9AA8u16 {
                bus.write_byte(2, addr, 0);
            }
            skip_to_flush = true;
        } else {
            for i in 0..10u16 {
                bus.write_byte(2, 0x9A80 + i, 0);
                bus.write_byte(2, 0x9A96 + i, 0);
            }
            bus.write_byte(2, 0x9A8E, 0);
            bus.write_byte(2, 0x9AA4, 0);
            for i in 0..3u16 {
                bus.write_byte(2, 0x9A90 + i, 0);
                bus.write_byte(2, 0x9AA6 + i, 0);
            }
            bus.write_byte(2, 0x9A94, 0);
            bus.write_byte(2, 0x9AAA, 0);
        }
    } else {
        let coin_add = bus.read_byte(2, 0x9B33);
        if coin_add != 0 {
            let cur = bus.read_byte(2, 0x9A80);
            bus.write_byte(2, 0x9A80, cur.wrapping_add(coin_add));
            bus.write_byte(2, 0x9B33, 0);
        }

        const HIGH_PRIORITY_IDS: [u8; 8] = [0x01, 0x08, 0x02, 0x03, 0x14, 0x07, 0x04, 0x05];
        for &id in &HIGH_PRIORITY_IDS {
            if bus.read_byte(2, 0x9A80 + id as u16) != 0 {
                bus.write_byte(2, 0x9B34, id);
                decomp_cpu2_step_sfx_track(cpu, bus, roms);
                skip_to_02e7 = true;
                break;
            }
        }

        if !skip_to_02e7 {
            if bus.read_byte(2, 0x9A91) != 0 {
                bus.write_byte(2, 0x9B34, 0x11);
                decomp_cpu2_step_sfx_track(cpu, bus, roms);
                bus.write_byte(2, 0x9B3B, 0x50);
            } else {
                let timer = bus.read_byte(2, 0x9B3B).wrapping_sub(1);
                bus.write_byte(2, 0x9B3B, timer);
                if timer == 0 {
                    bus.write_byte(2, 0x9B3B, 1);
                    if bus.read_byte(2, 0x9A92) != 0 {
                        bus.write_byte(2, 0x9B34, 0x12);
                        decomp_cpu2_step_sfx_track(cpu, bus, roms);
                    }
                }
            }
        }
    }

    if !skip_to_flush && !skip_to_02e7 {
        bus.write_byte(2, 0x9B34, 0x0F);
        if bus.read_byte(2, 0x9A8F) != 0 {
            bus.write_byte(2, 0x9A8F, 0);
            decomp_cpu2_init_music_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9AA5) != 0 {
            decomp_cpu2_step_music_track(cpu, bus, roms);
        } else {
            bus.write_byte(2, 0x9B34, 0x10);
            if bus.read_byte(2, 0x9A90) != 0 {
                bus.write_byte(2, 0x9A90, 0);
                decomp_cpu2_init_music_track(cpu, bus, roms);
            } else if bus.read_byte(2, 0x9AA6) != 0 {
                decomp_cpu2_step_music_track(cpu, bus, roms);
            }
        }

        bus.write_byte(2, 0x9B34, 0x0A);
        if bus.read_byte(2, 0x9A8A) != 0 {
            bus.write_byte(2, 0x9A8A, 0);
            decomp_cpu2_init_music_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9AA0) != 0 {
            decomp_cpu2_step_music_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9A8B) != 0 {
            bus.write_byte(2, 0x9B34, 0x0B);
            decomp_cpu2_step_sfx_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9A8C) != 0 {
            bus.write_byte(2, 0x9B34, 0x0C);
            decomp_cpu2_step_sfx_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9A8D) != 0 {
            bus.write_byte(2, 0x9B34, 0x0D);
            decomp_cpu2_step_sfx_track(cpu, bus, roms);
        }

        bus.write_byte(2, 0x9B34, 0x0E);
        if bus.read_byte(2, 0x9A8E) != 0 {
            bus.write_byte(2, 0x9A8E, 0);
            decomp_cpu2_init_music_track(cpu, bus, roms);
        } else if bus.read_byte(2, 0x9AA4) != 0 {
            decomp_cpu2_step_music_track(cpu, bus, roms);
        }

        let mut played_09 = false;
        if bus.read_byte(2, 0x9A89) != 0 {
            if (bus.read_byte(2, 0x8646) & 0x01) != 0 {
                bus.write_byte(2, 0x9A89, 0);
                bus.write_byte(2, 0x9A9F, 0);
            } else {
                bus.write_byte(2, 0x9B34, 0x09);
                decomp_cpu2_step_sfx_track(cpu, bus, roms);
                played_09 = true;
            }
        }
        if !played_09 && bus.read_byte(2, 0x9A86) != 0 {
            if (bus.read_byte(2, 0x8646) & 0x01) != 0 || (bus.read_byte(2, 0x8401) & 0x80) != 0 {
                bus.write_byte(2, 0x9A86, 0);
                bus.write_byte(2, 0x9A9C, 0);
            } else {
                bus.write_byte(2, 0x9B34, 0x06);
                decomp_cpu2_step_sfx_track(cpu, bus, roms);
            }
        }

        if bus.read_byte(2, 0x9A93) != 0 {
            if (bus.read_byte(2, 0x8657) & 0x02) != 0 || (bus.read_byte(2, 0x8401) & 0x20) != 0 {
                bus.write_byte(2, 0x9B34, 0x13);
                decomp_cpu2_step_sfx_track(cpu, bus, roms);
            } else {
                bus.write_byte(2, 0x9A93, 0);
                bus.write_byte(2, 0x9AA9, 0);
            }
        }
    }

    if !skip_to_flush && bus.read_byte(2, 0x9A80) != 0 {
        bus.write_byte(2, 0x9B34, 0x00);
        decomp_cpu2_step_sfx_track(cpu, bus, roms);
    }

    for i in 0..16u16 {
        let v = bus.read_byte(2, 0x9B20 + i);
        bus.write_byte(2, 0x6810 + i, v);
    }
    cpu.set_bc(0x0000);
    cpu.set_de(0x6820);

    let w0 = bus.read_byte(2, 0x9B30);
    bus.write_byte(2, 0x6805, w0);
    let w1 = bus.read_byte(2, 0x9B31);
    bus.write_byte(2, 0x680A, w1);
    let w2 = bus.read_byte(2, 0x9B32);
    bus.write_byte(2, 0x680F, w2);

    bus.write_byte(2, 0x9B3C, 0x00);

    let hl = cpu.pop(bus);
    let af = cpu.pop(bus);
    cpu.set_hl(hl);
    cpu.set_af(af);
    cpu.iff1 = cpu.iff2;
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 950;
}

/// Executes CPU 2 (Sound Z80) 100% in Human Rust without `src/recompiled_cpu2.rs`.
pub fn step_human_cpu2<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    stats: &mut DecompStats,
) -> u32 {
    if cpu.halted {
        cpu.total_cycles += 4;
        return 4;
    }

    if try_run_decompiled_subroutine(cpu, bus, roms, stats) {
        flush_nested_stats(stats);
        return 16;
    }

    match cpu.pc {
        0x0000 => {
            cpu.sp = 0x8B80;
            cpu.pc = 0x0085;
            cpu.total_cycles += 20;
            20
        }
        0x0003 => {
            cpu.pc = 0x0085;
            cpu.total_cycles += 10;
            10
        }
        0x0085..=0x008A => {
            cpu.a = 0x01;
            bus.write_byte(2, 0x6822, 0x01);
            cpu.push(bus, 0x008D);
            cpu.pc = 0x067F;
            cpu.total_cycles += 37;
            37
        }
        0x0695..=0x069B => {
            let v = bus.read_byte(2, 0x8A01);
            cpu.a = v;
            cpu.and8(v);
            if v == 0 {
                cpu.ret_subroutine(bus);
                cpu.total_cycles += 34;
                34
            } else {
                cpu.pc = 0x0695;
                cpu.total_cycles += 29;
                29
            }
        }
        0x06A1 => {
            cpu.total_cycles += 12;
            12
        }
        0x008D..=0x00AF => {
            for addr in 0x8A40..0x8A48u16 {
                bus.write_byte(2, addr, 0x00);
            }
            for addr in 0x9A80..0x9B80u16 {
                bus.write_byte(2, addr, 0x00);
            }
            for i in 0..0x15u16 {
                let b = roms.cpu2[(0x07E7 + i) as usize];
                bus.write_byte(2, 0x9B0A + i, b);
            }
            cpu.set_hl(0x07FC);
            cpu.set_de(0x9B1F);
            cpu.set_bc(0x0000);
            cpu.xor8(cpu.a);
            bus.write_byte(2, 0x6822, 0x00);
            cpu.pc = 0x00B0;
            cpu.total_cycles += 5800;
            5800
        }
        0x00B0 => {
            cpu.sp = 0x8B80;
            cpu.pc = 0x00B3;
            cpu.total_cycles += 10;
            10
        }
        0x00B3 => {
            cpu.pc = 0x00B0;
            cpu.total_cycles += 12;
            12
        }
        _ => {
            cpu.pc = 0x00B0;
            cpu.total_cycles += 4;
            4
        }
    }
}

// =========================================================================
// CPU 0 (MAIN GAME CPU) HUMAN-DECOMPILED SUBROUTINE HELPERS ($0066..$3E94)
// =========================================================================

fn decomp_ex_af_af(cpu: &mut Z80) {
    std::mem::swap(&mut cpu.a, &mut cpu.a_alt);
    std::mem::swap(&mut cpu.f, &mut cpu.f_alt);
}

fn decomp_exx(cpu: &mut Z80) {
    std::mem::swap(&mut cpu.b, &mut cpu.b_alt);
    std::mem::swap(&mut cpu.c, &mut cpu.c_alt);
    std::mem::swap(&mut cpu.d, &mut cpu.d_alt);
    std::mem::swap(&mut cpu.e, &mut cpu.e_alt);
    std::mem::swap(&mut cpu.h, &mut cpu.h_alt);
    std::mem::swap(&mut cpu.l, &mut cpu.l_alt);
}

fn decomp_cpu0_nmi_06xx_io<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x0066..0x00A0: NMI handler for Namco 06XX/51XX/53XX I/O transfer queue ($8900)
    decomp_exx(cpu);
    cpu.ldi(bus);
    if (cpu.f & FLAG_PV) != 0 {
        // BC != 0 -> JP PE, $009E: EXX; RETN
        decomp_exx(cpu);
        cpu.iff1 = cpu.iff2;
        cpu.ret_subroutine(bus);
        return;
    }

    let saved_af = cpu.af();
    let mut hl = bus.read_word(0, 0x8900);
    cpu.set_hl(hl);
    let cmd = bus.read_byte(0, hl);
    cpu.a = cmd;
    cpu.and8(cmd);
    if cmd == 0 {
        // 0x0098: LD A, $10; LD ($7100), A; POP AF; EXX; RETN
        bus.write_byte(0, 0x7100, 0x10);
        cpu.set_af(saved_af);
        decomp_exx(cpu);
        cpu.iff1 = cpu.iff2;
        cpu.ret_subroutine(bus);
        return;
    }

    bus.write_byte(0, hl, 0x00);
    let h_hi = hl & 0xFF00;
    let mut l = (hl & 0xFF) as u8;
    l = cpu.inc8(l);
    l = cpu.inc8(l);
    cpu.c = bus.read_byte(0, h_hi | (l as u16));
    l = cpu.inc8(l);
    cpu.b = bus.read_byte(0, h_hi | (l as u16));
    l = cpu.inc8(l);
    cpu.e = bus.read_byte(0, h_hi | (l as u16));
    l = cpu.inc8(l);
    cpu.d = bus.read_byte(0, h_hi | (l as u16));
    l = cpu.inc8(l);
    let ptr_lo = bus.read_byte(0, h_hi | (l as u16));
    l = cpu.inc8(l);
    l = cpu.inc8(l);
    hl = h_hi | (l as u16);
    bus.write_word(0, 0x8900, hl);
    l = cpu.dec8(l);
    let ptr_hi = bus.read_byte(0, h_hi | (l as u16));
    cpu.h = ptr_hi;
    cpu.l = ptr_lo;

    bus.write_byte(0, 0x7100, 0x10);
    decomp_exx(cpu);
    bus.write_byte(0, 0x7100, cmd);
    cpu.set_af(saved_af);
    cpu.iff1 = cpu.iff2;
    cpu.ret_subroutine(bus);
}

fn decomp_queue_06xx_io_command<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x00A1: Queue or immediately start a Namco 06XX I/O transfer
    decomp_ex_af_af(cpu);
    cpu.iff1 = false;
    cpu.iff2 = false;
    let ctrl = bus.read_byte(0, 0x7100);
    cpu.a = ctrl;
    cpu.and8(0xE0);
    if (cpu.f & FLAG_Z) != 0 {
        // 0x00AA: EX AF, AF'; EXX; LD ($7100), A; EI; RET
        decomp_ex_af_af(cpu);
        decomp_exx(cpu);
        bus.write_byte(0, 0x7100, cpu.a);
        cpu.iff1 = true;
        cpu.iff2 = true;
        cpu.ret_subroutine(bus);
        return;
    }

    let saved_hl = cpu.hl();
    let saved_de = cpu.de();
    let mut hl = bus.read_word(0, 0x8902);
    cpu.a = (hl & 0xFF) as u8;
    cpu.add8(0x08, false);
    hl = (hl & 0xFF00) | (cpu.a as u16);
    bus.write_word(0, 0x8902, hl);

    let mut l = (hl & 0xFF) as u8;
    let h_hi = hl & 0xFF00;
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), (saved_hl >> 8) as u8);
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), (saved_hl & 0xFF) as u8);
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), (saved_de >> 8) as u8);
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), (saved_de & 0xFF) as u8);
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), cpu.b);
    l = cpu.dec8(l);
    bus.write_byte(0, h_hi | (l as u16), cpu.c);
    bus.write_byte(0, 0x6830, cpu.a);
    l = cpu.dec8(l);
    l = cpu.dec8(l);
    let entry_hl = h_hi | (l as u16);
    cpu.set_hl(entry_hl);
    cpu.set_de(saved_de);

    decomp_ex_af_af(cpu);
    bus.write_byte(0, entry_hl, cpu.a);
    let ctrl2 = bus.read_byte(0, 0x7100);
    cpu.a = ctrl2;
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.set_hl(saved_hl);
        cpu.iff1 = true;
        cpu.iff2 = true;
        cpu.ret_subroutine(bus);
    } else {
        let cmd = bus.read_byte(0, entry_hl);
        cpu.a = cmd;
        bus.write_byte(0, entry_hl, 0x00);
        bus.write_word(0, 0x8902, entry_hl);
        cpu.set_hl(saved_hl);
        decomp_ex_af_af(cpu);
        decomp_exx(cpu);
        bus.write_byte(0, 0x7100, cpu.a);
        cpu.iff1 = true;
        cpu.iff2 = true;
        cpu.ret_subroutine(bus);
    }
}

fn decomp_copy_rom_to_vram_dec_hl_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    hl_start: u16,
    ix_start: u16,
    count: u8,
) {
    let mut hl = hl_start;
    let mut ix = ix_start;
    let mut last_a = cpu.a;
    for _ in 0..count {
        last_a = roms.cpu0[ix as usize];
        bus.write_byte(0, hl, last_a);
        hl = hl.wrapping_sub(1);
        ix = ix.wrapping_add(1);
    }
    cpu.a = last_a;
    cpu.b = 0;
    cpu.set_hl(hl);
    cpu.ix = ix;
}

fn decomp_check_and_award_bonus_life<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x0C81: Check and award bonus life
    let s8400 = bus.read_byte(0, 0x8400);
    cpu.a = s8400;
    cpu.cb_op(0x4F, s8400); // BIT 1, A
    let hl_msb = if (s8400 & 0x02) != 0 { 0x8419 } else { 0x8416 };
    cpu.set_hl(hl_msb);
    let msb = bus.read_byte(0, hl_msb);
    cpu.a = msb;
    cpu.sub8(0x90, false, false);
    if msb >= 0x90 {
        cpu.ret_subroutine(bus);
        return;
    }
    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x47, s8657); // BIT 0, A
    if (s8657 & 0x01) != 0 {
        cpu.ret_subroutine(bus);
        return;
    }
    let s87c0 = bus.read_byte(0, 0x87C0);
    cpu.a = s87c0;
    cpu.and8(s87c0);
    if s87c0 == 0 {
        cpu.ret_subroutine(bus);
        return;
    }

    let s8400_2 = bus.read_byte(0, 0x8400);
    cpu.cb_op(0x4E, s8400_2); // BIT 1, (HL)
    let (score_src, thresh_src, flag_addr) = if (s8400_2 & 0x02) != 0 {
        (0x8417u16, 0x8630u16, 0x8403u16)
    } else {
        (0x8414u16, 0x862Du16, 0x8402u16)
    };
    for i in 0..3u16 {
        let v = bus.read_byte(0, score_src + i);
        bus.write_byte(0, 0x8411 + i, v);
        bus.write_byte(0, 0x6830, v);
    }
    for i in 0..3u16 {
        let v = bus.read_byte(0, thresh_src + i);
        bus.write_byte(0, 0x8633 + i, v);
        bus.write_byte(0, 0x6830, v);
        cpu.a = v;
    }
    cpu.b = 0;
    cpu.set_de(0x8636);
    cpu.set_hl(flag_addr);
    bus.write_word(0, 0x85F6, flag_addr);

    let flag_val = bus.read_byte(0, flag_addr);
    cpu.cb_op(0x7E, flag_val); // BIT 7, (HL)
    if (flag_val & 0x80) == 0 {
        // First bonus life check against $862A..$862C (sub_0D60)
        cpu.and8(cpu.a);
        for i in 0..3u16 {
            cpu.a = bus.read_byte(0, 0x8411 + i);
            let sub_v = bus.read_byte(0, 0x862A + i);
            cpu.sub8(sub_v, true, true);
        }
        cpu.set_hl(0x8414);
        cpu.set_de(0x862D);
        cpu.b = 0;
        if (cpu.f & FLAG_C) != 0 {
            cpu.ret_subroutine(bus);
            return;
        }
        let f_ptr = bus.read_word(0, 0x85F6);
        let fv = bus.read_byte(0, f_ptr) | 0x80;
        bus.write_byte(0, f_ptr, fv);
        let snd = bus.read_byte(0, 0x9A87) | 0x01;
        bus.write_byte(0, 0x9A87, snd);

        // sub_0D6C
        let st = bus.read_byte(0, 0x8400);
        let lives_addr = if (st & 0x02) != 0 { 0x840B } else { 0x840A };
        let nl = cpu.inc8(bus.read_byte(0, lives_addr));
        bus.write_byte(0, lives_addr, nl);
        bus.write_byte(0, 0x840C, nl);
        decomp_draw_player_lives_hud(cpu, bus);

        // sub_1226(A=1, HL=$89EA)
        cpu.a = 0x01;
        cpu.and8(0x01);
        let lo = bus.read_byte(0, 0x89EA);
        cpu.add8(lo, false);
        cpu.daa();
        bus.write_byte(0, 0x89EA, cpu.a);
        cpu.set_hl(0x89E9);
        cpu.a = bus.read_byte(0, 0x89E9);
        cpu.add8(0x00, true);
        cpu.daa();
        bus.write_byte(0, 0x89E9, cpu.a);
        cpu.ret_subroutine(bus);
    } else {
        // Subsequent bonus life check ($0CFC)
        cpu.set_hl(0x8411);
        let st = bus.read_byte(0, 0x8400);
        cpu.a = st;
        cpu.cb_op(0x4F, st);
        let bf = bus.read_byte(0, 0x87BF);
        cpu.a = bf;
        if (st & 0x02) != 0 {
            cpu.cb_op(0x4F, bf);
            if (bf & 0x02) != 0 {
                cpu.ret_subroutine(bus);
                return;
            }
        } else {
            cpu.cb_op(0x47, bf);
            if (bf & 0x01) != 0 {
                cpu.ret_subroutine(bus);
                return;
            }
        }
        let c0 = bus.read_byte(0, 0x87C0);
        cpu.a = c0;
        cpu.sub8(0x01, false, false);
        if c0 == 0x01 {
            cpu.ret_subroutine(bus);
            return;
        }
        // sub_0D60(HL=$8411, DE=$8633)
        cpu.and8(cpu.a);
        for i in 0..3u16 {
            cpu.a = bus.read_byte(0, 0x8411 + i);
            let sub_v = bus.read_byte(0, 0x8633 + i);
            cpu.sub8(sub_v, true, true);
        }
        cpu.set_hl(0x8414);
        cpu.set_de(0x8636);
        cpu.b = 0;
        if (cpu.f & FLAG_C) != 0 {
            cpu.ret_subroutine(bus);
            return;
        }

        // sub_0D6C
        let st2 = bus.read_byte(0, 0x8400);
        let lives_addr = if (st2 & 0x02) != 0 { 0x840B } else { 0x840A };
        let nl = cpu.inc8(bus.read_byte(0, lives_addr));
        bus.write_byte(0, lives_addr, nl);
        bus.write_byte(0, 0x840C, nl);
        decomp_draw_player_lives_hud(cpu, bus);

        let snd = bus.read_byte(0, 0x9A87) | 0x01;
        bus.write_byte(0, 0x9A87, snd);

        // sub_1226(A=1, HL=$89EA)
        cpu.a = 0x01;
        cpu.and8(0x01);
        let lo = bus.read_byte(0, 0x89EA);
        cpu.add8(lo, false);
        cpu.daa();
        bus.write_byte(0, 0x89EA, cpu.a);
        cpu.set_hl(0x89E9);
        cpu.a = bus.read_byte(0, 0x89E9);
        cpu.add8(0x00, true);
        cpu.daa();
        bus.write_byte(0, 0x89E9, cpu.a);

        // sub_0C6F(IX=$8633, HL=$85AC)
        decomp_set_bcd_threshold(cpu, bus, 0x8633, 0x85AC);

        // sub_0D84
        let st3 = bus.read_byte(0, 0x8400);
        let mut hl_t = if (st3 & 0x02) != 0 { 0x8630u16 } else { 0x862Du16 };
        let mut de_t = 0x8633u16;
        cpu.and8(cpu.a);
        for _ in 0..3 {
            cpu.a = bus.read_byte(0, de_t);
            let cur = bus.read_byte(0, hl_t);
            cpu.add8(cur, true);
            cpu.daa();
            bus.write_byte(0, hl_t, cpu.a);
            de_t = de_t.wrapping_add(1);
            hl_t = hl_t.wrapping_add(1);
        }
        cpu.set_hl(hl_t);
        cpu.set_de(de_t);
        cpu.b = 0;

        let c0_end = bus.read_byte(0, 0x87C0);
        cpu.a = c0_end;
        cpu.sub8(0x02, false, false);
        if c0_end != 0x02 {
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.set_hl(0x87BF);
        let st_end = bus.read_byte(0, 0x8400);
        cpu.a = st_end;
        cpu.cb_op(0x4F, st_end);
        let cur_bf = bus.read_byte(0, 0x87BF);
        if (st_end & 0x02) != 0 {
            bus.write_byte(0, 0x87BF, cur_bf | 0x02);
        } else {
            bus.write_byte(0, 0x87BF, cur_bf | 0x01);
        }
        cpu.ret_subroutine(bus);
    }
}

fn decomp_update_harpoon_and_pump_state<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x0DA2: Master harpoon & pump state machine
    let s8401 = bus.read_byte(0, 0x8401);
    cpu.a = s8401;
    cpu.cb_op(0x7F, s8401); // BIT 7, A
    if (s8401 & 0x80) != 0 {
        decomp_reset_harpoon_0dfa(cpu, bus);
        cpu.ret_subroutine(bus);
        return;
    }

    let s8403 = bus.read_byte(0, 0x8403);
    cpu.a = s8403;
    cpu.cb_op(0x57, s8403); // BIT 2, A
    if (s8403 & 0x04) != 0 {
        // 0x0E34: Monster currently tethered on harpoon -> pump state machine
        cpu.set_hl(0x8404);
        let s8404 = bus.read_byte(0, 0x8404);
        cpu.cb_op(0x76, s8404); // BIT 6, (HL)
        if (s8404 & 0x40) != 0 {
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.cb_op(0x56, s8404); // BIT 2, (HL)
        if (s8404 & 0x04) == 0 {
            cpu.set_hl(0x8403);
            let v8403 = bus.read_byte(0, 0x8403) & !0x08;
            bus.write_byte(0, 0x8403, v8403);
            let s8657 = bus.read_byte(0, 0x8657);
            cpu.a = s8657;
            cpu.cb_op(0x4F, s8657);
            let btn = if (s8657 & 0x02) != 0 {
                bus.read_byte(0, 0x8970)
            } else {
                bus.read_byte(0, 0x85B0)
            };
            cpu.a = btn;
            cpu.cb_op(0x6F, btn); // BIT 5, A
            if (btn & 0x20) == 0 {
                cpu.set_hl(0x8404);
                let v = bus.read_byte(0, 0x8404) | 0x04;
                bus.write_byte(0, 0x8404, v);
            }
            cpu.a = 0;
            cpu.xor8(0);
            bus.write_byte(0, 0x8641, 0x00);
            cpu.ret_subroutine(bus);
            return;
        }

        // 0x0E5C: Increment pump cycle counter ($8641)
        cpu.set_hl(0x8641);
        let cnt = cpu.inc8(bus.read_byte(0, 0x8641));
        bus.write_byte(0, 0x8641, cnt);
        cpu.a = cnt;
        cpu.and8(0x1F);
        let masked = cpu.a;
        cpu.sub8(0x09, false, false);
        if masked < 0x09 {
            cpu.set_hl(0x8403);
            let v = bus.read_byte(0, 0x8403) & !0x08;
            bus.write_byte(0, 0x8403, v);
            cpu.ret_subroutine(bus);
            return;
        }
        if masked == 0x09 {
            cpu.set_hl(0x8403);
            let v = bus.read_byte(0, 0x8403) | 0x08;
            bus.write_byte(0, 0x8403, v);
            decomp_advance_monster_pump_stage_0e9f(cpu, bus, roms);
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.sub8(0x12, false, false);
        if masked == 0x12 {
            cpu.set_hl(0x8403);
            let v = bus.read_byte(0, 0x8403) & !0x08;
            bus.write_byte(0, 0x8403, v);
            cpu.ret_subroutine(bus);
            return;
        }
        if masked < 0x12 {
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.sub8(0x1F, false, false);
        if masked == 0x1F {
            cpu.a = 0;
            cpu.xor8(0);
            bus.write_byte(0, 0x8641, 0x00);
        }
        let s8657 = bus.read_byte(0, 0x8657);
        cpu.a = s8657;
        cpu.cb_op(0x4F, s8657);
        let btn = if (s8657 & 0x02) != 0 {
            bus.read_byte(0, 0x8970)
        } else {
            bus.read_byte(0, 0x85B0)
        };
        cpu.a = btn;
        cpu.cb_op(0x6F, btn); // BIT 5, A
        if (btn & 0x20) == 0 {
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.set_hl(0x8404);
        let v = bus.read_byte(0, 0x8404) & !0x04;
        bus.write_byte(0, 0x8404, v);
        cpu.ret_subroutine(bus);
        return;
    }

    // Harpoon not tethered: check if harpoon is currently extending (bit 3 of $8401)
    cpu.set_hl(0x8401);
    let v8401 = bus.read_byte(0, 0x8401);
    cpu.cb_op(0x5E, v8401); // BIT 3, (HL)
    if (v8401 & 0x08) == 0 {
        let s8657 = bus.read_byte(0, 0x8657);
        cpu.a = s8657;
        cpu.cb_op(0x4F, s8657);
        let btn = if (s8657 & 0x02) != 0 {
            bus.read_byte(0, 0x8970)
        } else {
            bus.read_byte(0, 0x85B0)
        };
        cpu.a = btn;
        cpu.cb_op(0x67, btn); // BIT 4, A
        if (btn & 0x10) != 0 {
            cpu.ret_subroutine(bus);
            return;
        }
        // Fire new harpoon shot!
        cpu.set_hl(0x8401);
        bus.write_byte(0, 0x8401, v8401 | 0x08);
        let s8657_2 = bus.read_byte(0, 0x8657);
        cpu.a = s8657_2;
        cpu.cb_op(0x4F, s8657_2);
        let init_timer = if (s8657_2 & 0x02) != 0 { 0x36 } else { 0x32 };
        bus.write_byte(0, 0x863E, init_timer);
        decomp_init_harpoon_sprites_body(cpu, bus, roms);
        cpu.a = 0;
        cpu.xor8(0);
        bus.write_byte(0, 0x879E, 0x00);
        cpu.set_hl(0x9A8F);
        bus.write_byte(0, 0x9A8F, 0x01);
        cpu.ret_subroutine(bus);
        return;
    }

    // 0x0DEE: Harpoon currently extending
    cpu.set_hl(0x863E);
    let timer = bus.read_byte(0, 0x863E);
    cpu.a = timer;
    cpu.sub8(0x33, false, false);
    if timer >= 0x33 {
        let nt = cpu.dec8(timer);
        bus.write_byte(0, 0x863E, nt);
        cpu.ret_subroutine(bus);
        return;
    }
    cpu.sub8(0x06, false, false);
    if timer < 0x06 {
        decomp_reset_harpoon_0dfa(cpu, bus);
        cpu.ret_subroutine(bus);
        return;
    }
    bus.write_word(0, 0x8642, 0x863E);
    let tip_y = bus.read_byte(0, 0x9887);
    cpu.a = tip_y;
    cpu.sub8(0x08, false, false);
    if tip_y < 0x08 {
        decomp_reset_harpoon_0dfa(cpu, bus);
        cpu.ret_subroutine(bus);
        return;
    }
    cpu.e = tip_y;
    let tip_x = bus.read_byte(0, 0x9886);
    cpu.a = tip_x;
    cpu.d = tip_x;
    decomp_populate_4tile_quad(cpu, bus, 0, 0x8602);
    decomp_check_harpoon_dirt_collision_body(cpu, bus);
    cpu.a = cpu.c;
    cpu.and8(cpu.c);
    if cpu.c == 0 {
        decomp_reset_harpoon_0dfa(cpu, bus);
        cpu.ret_subroutine(bus);
        return;
    }
    cpu.set_hl(0x8404);
    let v8404 = bus.read_byte(0, 0x8404) & !0x40;
    bus.write_byte(0, 0x8404, v8404);
    decomp_step_harpoon_cable_1209_body(cpu, bus);
    let new_tip_y = bus.read_byte(0, 0x9887);
    cpu.a = new_tip_y;
    cpu.sub8(0xF2, false, false);
    if new_tip_y >= 0xF2 {
        decomp_reset_harpoon_0dfa(cpu, bus);
        cpu.ret_subroutine(bus);
        return;
    }
    decomp_check_harpoon_monsters_10e0_body(cpu, bus, roms);
    cpu.ret_subroutine(bus);
}

fn decomp_reset_harpoon_0dfa<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    for addr in 0x9882..0x988Au16 {
        bus.write_byte(0, addr, 0x00);
        bus.write_byte(0, 0x6830, cpu.a);
    }
    cpu.b = 0;
    let v8401 = bus.read_byte(0, 0x8401) & !0x08;
    bus.write_byte(0, 0x8401, v8401);
    cpu.set_hl(0x863E);
    bus.write_byte(0, 0x863E, 0x00);
}

fn decomp_init_harpoon_sprites_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let v8408 = bus.read_byte(0, 0x8408) | 0x01;
    bus.write_byte(0, 0x8408, v8408);
    let dir = bus.read_byte(0, 0x85AE);
    bus.write_byte(0, 0x87E4, dir);
    cpu.cb_op(0x4F, dir);
    let src = if (dir & 0x02) != 0 { 0x0FBBu16 } else { 0x0FC3u16 };
    for i in 0..8u16 {
        let b = roms.cpu0[(src + i) as usize];
        bus.write_byte(0, 0x9802 + i, b);
    }
    cpu.f &= !(FLAG_N | FLAG_H | FLAG_PV);
    let pal = bus.read_byte(0, 0x9922);
    for i in 0..4u16 {
        bus.write_byte(0, 0x9902 + i * 2, pal);
    }
    let c = bus.read_byte(0, 0x98A2);
    let a = bus.read_byte(0, 0x98A3);
    for i in 0..4u16 {
        bus.write_byte(0, 0x9882 + i * 2, c);
        bus.write_byte(0, 0x9883 + i * 2, a);
    }
    cpu.a = a;
    cpu.b = 0;
    cpu.c = c;
    cpu.set_de(0x980A);
    cpu.set_hl(0x988A);
    cpu.ix = 0x9902;
    cpu.iy = 0x9882;
}

fn decomp_check_harpoon_tile_1092_10d5<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, vertical: bool) {
    let t = bus.read_byte(0, cpu.hl());
    let matches_dir = if !vertical {
        t == 0x02 || t == 0x03 || t == 0x8D || t == 0x7E
    } else {
        t == 0x08 || t == 0x09 || t == 0x8D || t == 0x7E
    };
    if matches_dir {
        cpu.a = t;
        cpu.sub8(t, false, false);
        cpu.c = 1;
    } else {
        let t7 = t & 0x7F;
        cpu.a = t7;
        cpu.sub8(0x0C, false, false);
        cpu.c = if t7 == 0x0C { 1 } else { 0 };
    }
}

fn decomp_check_harpoon_dirt_collision_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let dir_raw = bus.read_byte(0, 0x87E4);
    cpu.a = dir_raw;
    cpu.and8(0x07);
    let dir = cpu.a;
    let (mut hl, mut de, vertical) = match dir {
        0 => (bus.read_word(0, 0x8602), bus.read_word(0, 0x8604), false),
        2 => {
            cpu.sub8(0x02, false, false);
            (bus.read_word(0, 0x8604), bus.read_word(0, 0x8608), true)
        }
        4 => {
            cpu.sub8(0x04, false, false);
            (bus.read_word(0, 0x8606), bus.read_word(0, 0x8608), false)
        }
        _ => {
            cpu.sub8(0x04, false, false);
            (bus.read_word(0, 0x8602), bus.read_word(0, 0x8606), true)
        }
    };
    cpu.set_hl(hl);
    cpu.set_de(de);

    let len = bus.read_byte(0, 0x863E);
    cpu.a = len;
    cpu.sub8(0x2C, false, false);
    if len >= 0x2C {
        let t = bus.read_byte(0, hl);
        cpu.a = t;
        cpu.sub8(0x7F, false, false);
        if t == 0x7F {
            cpu.c = 0x00;
            return;
        }
    } else {
        decomp_check_harpoon_tile_1092_10d5(cpu, bus, vertical);
        cpu.a = cpu.c;
        cpu.sub8(0x00, false, false);
        if cpu.c == 0x00 {
            return;
        }
    }

    std::mem::swap(&mut hl, &mut de);
    cpu.set_hl(hl);
    cpu.set_de(de);
    let len2 = bus.read_byte(0, 0x863E);
    cpu.a = len2;
    cpu.sub8(0x2C, false, false);
    if len2 >= 0x2C {
        let t = bus.read_byte(0, hl);
        cpu.a = t;
        cpu.sub8(0x7F, false, false);
        cpu.c = if t == 0x7F { 0x00 } else { 0x01 };
    } else {
        decomp_check_harpoon_tile_1092_10d5(cpu, bus, vertical);
    }
}

fn decomp_step_harpoon_cable_1209_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let hl = bus.read_word(0, 0x8642);
    cpu.set_hl(hl);
    let a = bus.read_byte(0, hl);
    cpu.a = a;
    cpu.and8(a);
    if a != 0 {
        let v1 = cpu.dec8(a);
        let v2 = cpu.dec8(v1);
        bus.write_byte(0, hl, v2);
        cpu.a = v2;
        cpu.sub8(0x22, false, false);
        if v2 >= 0x22 {
            decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
        } else {
            cpu.a = v2;
            cpu.sub8(0x12, false, false);
            if v2 >= 0x12 {
                decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9884, 0x9885);
                decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
            } else {
                decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9882, 0x9883);
                decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9884, 0x9885);
                decomp_adjust_harpoon_cable_segment(cpu, bus, 0x9886, 0x9887);
            }
        }
    }
}

fn decomp_advance_monster_pump_stage_0e9f<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x0E9F: Advance monster inflation stage (bits 2 -> 3 -> 4 -> 5 POP!) and award pop score
    let step = cpu.dec8(bus.read_byte(0, 0x8410));
    let de_step = step as u16;
    let tbl_hl = cpu.add16(0x32DF, de_step);
    let deflate_time = roms.cpu0[tbl_hl as usize];
    let mon_base = bus.read_word(0, 0x87C8);
    let timer_hl = cpu.add16(mon_base, 0x000C);
    bus.write_byte(0, timer_hl, deflate_time);

    let flags2_hl = mon_base.wrapping_add(2);
    let flags2 = bus.read_byte(0, flags2_hl);
    cpu.cb_op(0x6E, flags2); // BIT 5, (HL)
    if (flags2 & 0x20) != 0 {
        // 0x11CD: Note `LD HL, ($8403)` loads 16-bit word at $8403..$8404!
        let hl_ind = bus.read_word(0, 0x8403);
        cpu.set_hl(hl_ind);
        let v = bus.read_byte(0, hl_ind);
        cpu.cb_op(0x5E, v); // BIT 3, (HL)
        if (v & 0x08) != 0 {
            return;
        }
        bus.write_byte(0, hl_ind, v & !0x04);
        let hl_m2 = hl_ind.wrapping_sub(2);
        cpu.set_hl(hl_m2);
        let vm2 = bus.read_byte(0, hl_m2) | 0x08;
        bus.write_byte(0, hl_m2, vm2);
        return;
    }

    bus.write_byte(0, 0x9A90, 0x01);
    cpu.set_hl(mon_base);
    let state0 = bus.read_byte(0, mon_base);
    cpu.a = state0;
    cpu.add8(state0, false);
    if (cpu.f & FLAG_C) != 0 {
        return;
    }
    cpu.and8(0x38);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(0, mon_base, state0 | 0x04);
        return;
    }
    cpu.cb_op(0x56, state0); // BIT 2, (HL)
    if (state0 & 0x04) != 0 {
        let ns = (state0 & !0x04) | 0x08;
        bus.write_byte(0, mon_base, ns);
        cpu.set_hl(flags2_hl);
        bus.write_byte(0, flags2_hl, flags2 | 0x10);
        return;
    }
    cpu.cb_op(0x5E, state0); // BIT 3, (HL)
    if (state0 & 0x08) != 0 {
        let ns = (state0 & !0x08) | 0x10;
        bus.write_byte(0, mon_base, ns);
        cpu.set_hl(flags2_hl);
        bus.write_byte(0, flags2_hl, flags2 | 0x10);
        return;
    }
    cpu.cb_op(0x66, state0); // BIT 4, (HL)
    if (state0 & 0x10) == 0 {
        return;
    }

    // Stage 4 POP!
    let ns = (state0 & !(0x10 | 0x40)) | 0x20;
    bus.write_byte(0, mon_base, ns);
    let b3_hl = mon_base.wrapping_add(3);
    let b3 = bus.read_byte(0, b3_hl) | 0x80;
    bus.write_byte(0, b3_hl, b3);
    bus.write_byte(0, 0x9A8B, 0x01);
    let v8404 = bus.read_byte(0, 0x8404) | 0x40;
    bus.write_byte(0, 0x8404, v8404);
    let t_hl = cpu.add16(mon_base, 0x000C);
    bus.write_byte(0, t_hl, 0x16);

    cpu.set_hl(mon_base);
    cpu.cb_op(0x46, ns); // BIT 0, (HL) -> 0=Pooka, 1=Fygar
    let mut horiz_fygar = false;
    if (ns & 0x01) != 0 {
        let r10 = bus.read_byte(0, 0x8410);
        cpu.a = r10;
        cpu.sub8(0x0A, false, false);
        if r10 < 0x0A {
            let nr10 = cpu.inc8(r10);
            bus.write_byte(0, 0x8410, nr10);
        }
        let dir = bus.read_byte(0, 0x85AE);
        cpu.a = dir;
        cpu.cb_op(0x4F, dir); // BIT 1, A
        if (dir & 0x02) != 0 {
            horiz_fygar = true;
        }
    }

    // CALL $126C: compute depth stratum 0..3
    let y_hl = cpu.add16(mon_base, 0x0005);
    let s8400 = bus.read_byte(0, 0x8400);
    let mut y = bus.read_byte(0, y_hl);
    if (s8400 & 0x06) == 0x06 {
        y = y.wrapping_sub(0x10);
    }
    if y >= 0x20 {
        y = y.wrapping_sub(0x08);
    }
    let stratum = (y >> 6) & 0x03;

    let table_l = if !horiz_fygar {
        let s1 = stratum.wrapping_add(1);
        s1.wrapping_mul(3)
    } else {
        stratum.wrapping_mul(3).wrapping_add(0x0F)
    };
    cpu.l = table_l;
    decomp_add_score_from_table_2fb5_body(cpu, bus, roms);

    // CALL $1251
    bus.write_byte(0, 0x987E, 0x47);
    bus.write_byte(0, 0x987F, 0x0A);
    bus.write_byte(0, 0x997E, 0x00);
    cpu.ix = 0x9976;
    cpu.set_hl(0x9876);
    cpu.set_de(0x9877);

    let spr_code = if !horiz_fygar {
        match stratum {
            0 => 0x39,
            1 => 0x3A,
            2 => 0x3B,
            _ => 0x3C,
        }
    } else {
        match stratum {
            0 => 0x3B,
            1 => 0x3D,
            2 => 0x3E,
            _ => 0x3F,
        }
    };
    bus.write_byte(0, 0x9876, spr_code);
    cpu.a = 0x0A;
    cpu.f = FLAG_Z | FLAG_N;
    bus.write_byte(0, 0x9877, 0x0A);
    bus.write_byte(0, 0x9976, 0x00);
    cpu.set_hl(0x98F7);
    bus.write_byte(0, 0x98F7, 0x01);
}

fn decomp_check_harpoon_monsters_10e0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    decomp_check_harpoon_monsters_10e0_body(cpu, bus, roms);
    cpu.ret_subroutine(bus);
}

fn decomp_check_harpoon_monsters_10e0_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x10E0: Scan 8 monster slots ($8525, stride $10) for harpoon tip hit
    let mut ptr = 0x8525u16;
    bus.write_word(0, 0x85F6, ptr);
    let mut b = 0x08u8;
    let c = bus.read_byte(0, 0x87E4);
    cpu.c = c;
    loop {
        cpu.ix = ptr;
        let st0 = bus.read_byte(0, ptr);
        cpu.cb_op(0x7E, st0);
        let st3 = bus.read_byte(0, ptr.wrapping_add(3));
        if (st0 & 0x80) == 0 {
            cpu.cb_op(0x7E, st3);
        }
        if (st0 & 0x80) == 0 && (st3 & 0x80) == 0 {
            let tip = bus.read_word(0, 0x9886);
            cpu.set_hl(tip);
            cpu.e = bus.read_byte(0, ptr.wrapping_add(4));
            cpu.d = bus.read_byte(0, ptr.wrapping_add(5));
            let spr = bus.read_byte(0, ptr.wrapping_add(6));
            cpu.a = spr;
            cpu.sub8(0x40, false, false);
            if spr < 0x40 {
                decomp_hitbox_16x16_body(cpu);
            } else {
                let s8400 = !bus.read_byte(0, 0x8400);
                cpu.a = s8400;
                cpu.and8(0x06);
                if (cpu.f & FLAG_Z) != 0 {
                    cpu.a = 0x10;
                    cpu.add8(cpu.l, false);
                    cpu.l = cpu.a;
                    cpu.a = 0x10;
                    cpu.add8(cpu.h, false);
                    cpu.h = cpu.a;
                }
                decomp_hitbox_32x32_body(cpu);
            }
            if (cpu.f & FLAG_C) != 0 {
                // Hit monster! ($1135)
                cpu.b = b;
                cpu.a = b;
                bus.write_byte(0, 0x879E, b);
                bus.write_word(0, 0x87C8, cpu.ix);
                let v8403 = bus.read_byte(0, 0x8403) | 0x04;
                bus.write_byte(0, 0x8403, v8403);
                let v8401 = bus.read_byte(0, 0x8401) & !0x08;
                bus.write_byte(0, 0x8401, v8401);
                let v_mon = bus.read_byte(0, ptr) & !0x40;
                bus.write_byte(0, ptr, v_mon);
                let ptr_0c = cpu.add16(ptr, 0x000C);
                bus.write_word(0, 0x85F6, ptr_0c);
                let step = cpu.dec8(bus.read_byte(0, 0x8410));
                let tbl_hl = cpu.add16(0x32DF, step as u16);
                let def_t = roms.cpu0[tbl_hl as usize];
                bus.write_byte(0, cpu.ix.wrapping_add(0x0C), def_t);
                let v8403_2 = bus.read_byte(0, 0x8403) & !0x08;
                bus.write_byte(0, 0x8403, v8403_2);
                decomp_advance_monster_pump_stage_0e9f(cpu, bus, roms);
                let v8403_after = bus.read_byte(0, 0x8403);
                cpu.a = v8403_after;
                cpu.cb_op(0x57, v8403_after); // BIT 2, A
                if (v8403_after & 0x04) == 0 {
                    return;
                }
                decomp_step_harpoon_cable_1209_body(cpu, bus);
                decomp_step_harpoon_cable_1209_body(cpu, bus);
                return;
            }
        }
        ptr = cpu.add16(ptr, 0x0010);
        bus.write_word(0, 0x85F6, ptr);
        b = b.wrapping_sub(1);
        if b == 0 {
            cpu.b = 0;
            cpu.set_de(0x0010);
            cpu.set_hl(ptr);
            return;
        }
    }
}

fn decomp_hitbox_16x16_body(cpu: &mut Z80) {
    cpu.cb_op(0x49, cpu.c);
    if (cpu.c & 0x02) == 0 {
        cpu.a = cpu.l;
        cpu.sub8(cpu.e, false, true);
        cpu.add8(0x08, false);
        cpu.sub8(0x0C, false, false);
        if (cpu.f & FLAG_C) != 0 {
            cpu.a = cpu.h;
            cpu.sub8(cpu.d, false, true);
            cpu.cb_op(0x51, cpu.c);
            if (cpu.c & 0x04) != 0 {
                cpu.a = !cpu.a;
            }
            cpu.sub8(0x02, false, true);
            cpu.sub8(0x0C, false, false);
        }
    } else {
        cpu.a = cpu.h;
        cpu.sub8(cpu.d, false, true);
        cpu.add8(0x08, false);
        cpu.sub8(0x0C, false, false);
        if (cpu.f & FLAG_C) != 0 {
            cpu.a = cpu.l;
            cpu.sub8(cpu.e, false, true);
            cpu.cb_op(0x51, cpu.c);
            if (cpu.c & 0x04) == 0 {
                cpu.a = !cpu.a;
            }
            cpu.sub8(0x02, false, true);
            cpu.sub8(0x0C, false, false);
        }
    }
}

fn decomp_hitbox_32x32_body(cpu: &mut Z80) {
    cpu.cb_op(0x49, cpu.c);
    if (cpu.c & 0x02) == 0 {
        cpu.a = cpu.l;
        cpu.sub8(cpu.e, false, true);
        cpu.add8(0x03, false);
        cpu.sub8(0x12, false, false);
        if (cpu.f & FLAG_C) != 0 {
            cpu.a = cpu.h;
            cpu.sub8(cpu.d, false, true);
            cpu.cb_op(0x51, cpu.c);
            if (cpu.c & 0x04) != 0 {
                cpu.add8(0x10, false);
            }
            cpu.sub8(0x03, false, true);
            cpu.sub8(0x12, false, false);
        }
    } else {
        cpu.a = cpu.h;
        cpu.sub8(cpu.d, false, true);
        cpu.add8(0x07, false);
        cpu.sub8(0x12, false, false);
        if (cpu.f & FLAG_C) != 0 {
            cpu.a = cpu.l;
            cpu.sub8(cpu.e, false, true);
            cpu.cb_op(0x51, cpu.c);
            if (cpu.c & 0x04) == 0 {
                cpu.add8(0x10, false);
            }
            cpu.sub8(0x07, false, true);
            cpu.sub8(0x12, false, false);
        }
    }
}

fn decomp_draw_round_indicator_from_19bd<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // Shared tail $19BD..$1A20 (entered from 0x11EC with HL=$8410)
    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x4F, s8657); // BIT 1, A
    if (s8657 & 0x02) != 0 {
        let r = bus.read_byte(0, 0x9A05);
        bus.write_byte(0, 0x9A06, r);
        cpu.set_hl(0x9A06);
    } else {
        let hl = cpu.hl().wrapping_sub(1);
        cpu.set_hl(hl);
    }
    decomp_draw_round_indicator_from_19c5(cpu, bus, roms);
}

fn decomp_draw_round_indicator_from_19c5<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    decomp_convert_byte_at_hl_to_bcd(cpu, bus);
    let hl_bcd = cpu.hl();
    let bcd = bus.read_byte(0, hl_bcd);
    let lo = (bcd & 0x0F).wrapping_add(0x10);
    bus.write_byte(0, 0x8023, lo);
    let a = bcd.rotate_right(4);
    cpu.a = a;
    cpu.and8(0x0F);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = 0x7F;
    } else {
        cpu.add8(0x10, false);
    }
    bus.write_byte(0, 0x8024, cpu.a);

    // Copy 7 bytes from ROM $3068..$306E to VRAM $8008..$8002
    let mut hl = 0x8008u16;
    let mut ix = 0x3068u16;
    for _ in 0..7 {
        let ch = roms.cpu0[ix as usize];
        cpu.a = ch;
        bus.write_byte(0, hl, ch);
        hl = hl.wrapping_sub(1);
        ix = ix.wrapping_add(1);
    }
    cpu.b = 0;
    cpu.set_hl(hl);
    cpu.ix = ix;

    // Fall through to sub_19F7 (`decomp_sync_and_clamp_round`)
    decomp_sync_and_clamp_round(cpu, bus);
    cpu.ret_subroutine(bus);
}

fn decomp_swap_memory_blocks<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    hl_start: u16,
    de_start: u16,
    count: u16,
) {
    let mut hl = hl_start;
    let mut de = de_start;
    let mut bc = count;
    while bc > 0 {
        let a = bus.read_byte(0, de);
        let h_val = bus.read_byte(0, hl);
        bus.write_byte(0, de, h_val);
        bus.write_byte(0, hl, a);
        hl = hl.wrapping_add(1);
        de = de.wrapping_add(1);
        bc = bc.wrapping_sub(1);
    }
    cpu.set_bc(0);
    cpu.set_de(de);
    cpu.set_hl(hl);
    cpu.a = 0;
    cpu.or8(0);
    cpu.ret_subroutine(bus);
}

fn decomp_setup_player_word_sprites<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    for i in 0..6u16 {
        let b = roms.cpu0[(0x130D + i) as usize];
        bus.write_byte(0, 0x98FA + i, b);
    }
    for i in 0..6u16 {
        let b = roms.cpu0[(0x1313 + i) as usize];
        bus.write_byte(0, 0x987A + i, b);
    }
    let mut hl = 0x9978u16;
    for _ in 0..4 {
        bus.write_byte(0, hl, 0x00);
        hl = hl.wrapping_add(2);
    }
    cpu.set_bc(0x0000);
    cpu.set_de(0x9880);
    cpu.set_hl(hl);
    cpu.f &= !(FLAG_N | FLAG_H | FLAG_PV);
}

fn decomp_setup_player_num_sprite<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, code: u8) {
    bus.write_byte(0, 0x9878, code);
    bus.write_byte(0, 0x9879, 0x0C);
    bus.write_byte(0, 0x98F8, 0x89);
    bus.write_byte(0, 0x98F9, 0x70);
    cpu.set_hl(0x98F9);
}

fn decomp_setup_game_over_word_sprites<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    for i in 0..8u16 {
        let b = roms.cpu0[(0x1305 + i) as usize];
        bus.write_byte(0, 0x98F0 + i, b);
    }
    for i in 0..8u16 {
        let b = roms.cpu0[(0x12FD + i) as usize];
        bus.write_byte(0, 0x9870 + i, b);
    }
    let mut hl = 0x9970u16;
    for _ in 0..4 {
        bus.write_byte(0, hl, 0x00);
        hl = hl.wrapping_add(2);
    }
    cpu.set_bc(0x0000);
    cpu.set_de(0x9878);
    cpu.set_hl(hl);
    cpu.f &= !(FLAG_N | FLAG_H | FLAG_PV);
}

fn decomp_clear_text_overlay_sprites<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    for addr in 0x98F0..0x9900u16 {
        bus.write_byte(0, addr, 0x00);
    }
    for addr in 0x9870..0x9880u16 {
        bus.write_byte(0, addr, 0x00);
    }
    for i in 0..6u16 {
        bus.write_byte(0, 0x985E + i, 0x00);
        bus.write_byte(0, 0x98DE + i, 0x00);
    }
    cpu.a = 0x00;
    cpu.b = 0x00;
    cpu.set_hl(0x9864);
    cpu.set_de(0x98E4);
    cpu.ret_subroutine(bus);
}

fn decomp_check_monster_player_collision_134e<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x134E: Check all 8 monsters ($8529, stride $10) for Manhattan distance < $0A to Dig Dug ($98A2/$98A3)
    let mut hl = 0x8529u16;
    let mut b = 0x08u8;
    loop {
        bus.write_word(0, 0x85F6, hl);
        let mx = bus.read_byte(0, hl);
        let px = bus.read_byte(0, 0x98A2);
        cpu.c = mx;
        cpu.a = px;
        cpu.sub8(mx, false, false);
        if px < mx {
            cpu.a = mx;
            cpu.sub8(px, false, true);
        } else {
            cpu.sub8(mx, false, true);
        }
        let dx = cpu.a;
        bus.write_byte(0, 0x861A, dx);

        let y_hl = hl.wrapping_add(1);
        let my = bus.read_byte(0, y_hl);
        let py = bus.read_byte(0, 0x98A3);
        cpu.c = my;
        cpu.a = py;
        cpu.sub8(my, false, false);
        if py < my {
            cpu.a = my;
            cpu.sub8(py, false, true);
        } else {
            cpu.sub8(my, false, true);
        }
        let dy = cpu.a;
        let sum = cpu.add16(dx as u16, dy as u16);
        cpu.set_de(dy as u16);
        cpu.set_hl(sum);
        let h_hi = (sum >> 8) as u8;
        cpu.a = h_hi;
        cpu.and8(h_hi);
        let mut hit = false;
        if h_hi == 0 {
            let l_lo = (sum & 0xFF) as u8;
            cpu.a = l_lo;
            cpu.sub8(0x0A, false, false);
            if l_lo < 0x0A {
                let base = hl.wrapping_sub(4);
                cpu.set_hl(base);
                let st0 = bus.read_byte(0, base);
                cpu.a = st0;
                cpu.and8(0xBC);
                if (st0 & 0xBC) == 0 {
                    hit = true;
                }
            }
        }
        if hit {
            let v864a = bus.read_byte(0, 0x864A) & !0x01;
            bus.write_byte(0, 0x864A, v864a);
            cpu.b = b;
            decomp_trigger_player_hit_from_13a7(cpu, bus);
            cpu.ret_subroutine(bus);
            return;
        }
        cpu.set_de(0x0010);
        hl = cpu.add16(hl, 0x0010);
        b = cpu.dec8(b);
        cpu.b = b;
        if b == 0 {
            cpu.set_hl(hl);
            cpu.ret_subroutine(bus);
            return;
        }
    }
}

fn decomp_check_fire_player_collision_155f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x155F: Check if Fygar fire sprite at (IX+0, IY+0) is within |dx| < $0B and |dy| < $0B of Dig Dug
    let fx = bus.read_byte(0, cpu.ix);
    let px = bus.read_byte(0, 0x98A2);
    cpu.c = fx;
    cpu.a = px;
    cpu.sub8(fx, false, false);
    if px < fx {
        cpu.d = px;
        cpu.a = fx;
        cpu.sub8(px, false, true);
    } else {
        cpu.sub8(fx, false, true);
    }
    cpu.sub8(0x0B, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.ret_subroutine(bus);
        return;
    }

    let fy = bus.read_byte(0, cpu.iy);
    let py = bus.read_byte(0, 0x98A3);
    cpu.c = fy;
    cpu.a = py;
    cpu.sub8(fy, false, false);
    if py < fy {
        cpu.d = py;
        cpu.a = fy;
        cpu.sub8(py, false, true);
    } else {
        cpu.sub8(fy, false, true);
    }
    cpu.sub8(0x0B, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.ret_subroutine(bus);
        return;
    }

    let f6 = bus.read_word(0, 0x85F6);
    cpu.set_de(0x0004);
    let nf6 = cpu.add16(f6, 0x0004);
    bus.write_word(0, 0x85F6, nf6);
    let v864a = bus.read_byte(0, 0x864A) | 0x01;
    bus.write_byte(0, 0x864A, v864a);
    decomp_trigger_player_hit_from_13a7(cpu, bus);
    cpu.ret_subroutine(bus);
}

fn decomp_trigger_player_hit_from_13a7<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // Shared tail $13A7..$13FE
    cpu.set_hl(0x8401);
    let s8401 = bus.read_byte(0, 0x8401);
    cpu.cb_op(0x7E, s8401); // BIT 7, (HL)
    if (s8401 & 0x80) != 0 {
        return;
    }
    bus.write_byte(0, 0x8401, s8401 | 0x80);
    let s8402 = bus.read_byte(0, 0x8402) & !0x01;
    bus.write_byte(0, 0x8402, s8402);

    let s864a = bus.read_byte(0, 0x864A);
    cpu.cb_op(0x46, s864a); // BIT 0, (HL)
    if (s864a & 0x01) == 0 {
        let base = bus.read_word(0, 0x85F6).wrapping_sub(4);
        let st0 = bus.read_byte(0, base) & !0x02;
        bus.write_byte(0, base, st0);
        cpu.cb_op(0x46, st0);
        let saved_f = cpu.f;
        cpu.set_de(0x0006);
        let spr_hl = cpu.add16(base, 0x0006);
        cpu.f = saved_f;
        if (st0 & 0x01) == 0 {
            bus.write_byte(0, spr_hl, 0x20);
            bus.write_byte(0, spr_hl.wrapping_add(1), 0x01);
        } else {
            bus.write_byte(0, spr_hl, 0x24);
            bus.write_byte(0, spr_hl.wrapping_add(1), 0x02);
        }
    }
    bus.write_byte(0, 0x861C, 0xA0);
    for addr in 0x9A80..0x9AA0u16 {
        bus.write_byte(0, addr, 0x00);
    }
    cpu.b = 0;
    bus.write_byte(0, 0x9876, 0x32);
    bus.write_byte(0, 0x987E, 0x32);
    bus.write_byte(0, 0x9A84, 0x01);
    cpu.set_hl(0x9822);
    let dir = bus.read_byte(0, 0x85AE);
    cpu.a = dir;
    cpu.cb_op(0x4F, dir); // BIT 1, A
    let death_spr = if (dir & 0x02) != 0 { 0x4D } else { 0x4C };
    bus.write_byte(0, 0x9822, death_spr);
}

fn decomp_update_all_hud_scores_and_headers<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x146B: CALL $1432; CALL $1483; CALL $14DE; if BIT 0, ($8400) != 0 CALL $14F4; CALL $14E9; RET
    decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83D4, 0x1445, 0x0A);
    decomp_blink_active_1up_2up_headers_body(cpu, bus, roms);
    cpu.set_de(0x8927);
    cpu.ix = 0x83F2;
    decomp_draw_bcd_score_6digits_body(cpu, bus);
    let s8400 = bus.read_byte(0, 0x8400);
    cpu.a = s8400;
    cpu.cb_op(0x47, s8400); // BIT 0, A
    if (s8400 & 0x01) != 0 {
        cpu.set_de(0x8417);
        cpu.ix = 0x83E9;
        decomp_draw_bcd_score_6digits_body(cpu, bus);
    }
    cpu.set_de(0x8414);
    cpu.ix = 0x83FC;
    decomp_draw_bcd_score_6digits_body(cpu, bus);
    cpu.ret_subroutine(bus);
}

fn decomp_blink_active_1up_2up_headers_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let s8400 = bus.read_byte(0, 0x8400);
    cpu.a = s8400;
    cpu.cb_op(0x47, s8400); // BIT 0, A
    if (s8400 & 0x01) == 0 {
        // 1-player mode: blank 2UP ($14D8), blink 1UP
        decomp_blank_up_header_body(cpu, bus, 0x83C7);
        let f23 = bus.read_byte(0, 0x8423);
        cpu.a = f23;
        cpu.cb_op(0x6F, f23); // BIT 5, A
        let saved_f = cpu.f;
        if (f23 & 0x20) != 0 {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83DA, 0x1465, 0x03);
        }
        cpu.a = f23;
        cpu.f = saved_f;
        if (f23 & 0x20) == 0 {
            decomp_blank_up_header_body(cpu, bus, 0x83DA);
        }
        return;
    }
    cpu.cb_op(0x4F, s8400); // BIT 1, A
    if (s8400 & 0x02) != 0 {
        // 2-player mode, Player 2 active: draw steady 1UP ($144F), blink 2UP
        decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83DA, 0x1465, 0x03);
        let f23 = bus.read_byte(0, 0x8423);
        cpu.a = f23;
        cpu.cb_op(0x6F, f23); // BIT 5, A
        let saved_f = cpu.f;
        if (f23 & 0x20) != 0 {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83C7, 0x1468, 0x03);
        }
        cpu.a = f23;
        cpu.f = saved_f;
        if (f23 & 0x20) == 0 {
            decomp_blank_up_header_body(cpu, bus, 0x83C7);
        }
    } else {
        // 2-player mode, Player 1 active: draw steady 2UP ($145B), blink 1UP
        decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83C7, 0x1468, 0x03);
        let f23 = bus.read_byte(0, 0x8423);
        cpu.a = f23;
        cpu.cb_op(0x6F, f23); // BIT 5, A
        let saved_f = cpu.f;
        if (f23 & 0x20) != 0 {
            decomp_copy_rom_to_vram_dec_hl_body(cpu, bus, roms, 0x83DA, 0x1465, 0x03);
        }
        cpu.a = f23;
        cpu.f = saved_f;
        if (f23 & 0x20) == 0 {
            decomp_blank_up_header_body(cpu, bus, 0x83DA);
        }
    }
}

fn decomp_blank_up_header_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, hl_start: u16) {
    let mut hl = hl_start;
    let s87cc = bus.read_byte(0, 0x87CC);
    cpu.a = s87cc;
    cpu.cb_op(0x4F, s87cc); // BIT 1, A
    let tile = if (s87cc & 0x02) == 0 {
        0x8Cu8
    } else {
        cpu.cb_op(0x57, s87cc); // BIT 2, A
        if (s87cc & 0x04) != 0 {
            0x8Cu8
        } else {
            0x0Cu8
        }
    };
    cpu.a = tile;
    bus.write_byte(0, hl, tile);
    hl = hl.wrapping_sub(1);
    bus.write_byte(0, hl, tile);
    hl = hl.wrapping_sub(1);
    bus.write_byte(0, hl, tile);
    cpu.set_hl(hl);
}

fn decomp_draw_bcd_score_6digits_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let mut hl = 0x841Du16;
    let mut de = cpu.de();
    for _ in 0..3 {
        let byte = bus.read_byte(0, de);
        cpu.a = byte;
        cpu.and8(0x0F);
        bus.write_byte(0, hl, cpu.a);
        hl = hl.wrapping_add(1);
        let mut hi = byte;
        for _ in 0..4 {
            hi = cpu.cb_op(0x3F, hi); // SRL A
        }
        cpu.a = hi;
        bus.write_byte(0, hl, hi);
        hl = hl.wrapping_add(1);
        de = de.wrapping_add(1);
    }
    cpu.set_de(de);

    // Leading zero suppression on $8422 down to $841F (max 4 digits)
    hl = 0x8422;
    let mut b = 4u8;
    loop {
        let d = bus.read_byte(0, hl);
        cpu.a = d;
        cpu.and8(d);
        if (cpu.f & FLAG_Z) == 0 {
            break;
        }
        let s87cc = bus.read_byte(0, 0x87CC);
        cpu.a = s87cc;
        cpu.cb_op(0x4F, s87cc); // BIT 1, A
        let blank = if (s87cc & 0x02) == 0 {
            0x8Cu8
        } else {
            cpu.cb_op(0x57, s87cc); // BIT 2, A
            if (s87cc & 0x04) != 0 {
                0x8Cu8
            } else {
                0x0Cu8
            }
        };
        cpu.a = blank;
        bus.write_byte(0, hl, blank);
        hl = hl.wrapping_sub(1);
        b = b.wrapping_sub(1);
        if b == 0 {
            break;
        }
    }

    // Render 6 digits from $8422 down to $841D into (IX) descending
    hl = 0x8422;
    let mut ix = cpu.ix;
    for _ in 0..6 {
        let d = bus.read_byte(0, hl);
        cpu.a = d;
        cpu.sub8(0x8C, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.sub8(0x0C, false, false);
            if (cpu.f & FLAG_Z) == 0 {
                cpu.add8(0x10, false);
            }
        }
        bus.write_byte(0, ix, cpu.a);
        hl = hl.wrapping_sub(1);
        ix = ix.wrapping_sub(1);
    }
    cpu.b = 0;
    cpu.set_hl(hl);
    cpu.ix = ix;
}

fn decomp_fill_vram_with_7f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x15BF: Fill BC bytes at (HL) with $7F, writing A to $6830 each step
    let mut hl = cpu.hl();
    let mut bc = cpu.bc();
    while bc > 0 {
        bus.write_byte(0, hl, 0x7F);
        bus.write_byte(0, 0x6830, cpu.a);
        hl = hl.wrapping_add(1);
        bc = bc.wrapping_sub(1);
        let c = (bc & 0xFF) as u8;
        let b = (bc >> 8) as u8;
        cpu.a = c;
        cpu.or8(b);
    }
    cpu.set_bc(0);
    cpu.set_hl(hl);
    cpu.ret_subroutine(bus);
}

fn decomp_write_tile_column_from_ix<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    _roms: &DigDugRoms,
) {
    // 0x15CB: Loop A times (preserving B!):
    // PUSH AF; LD DE, $0020; LD A, (IX+0); LD (HL), A; INC IX; ADD HL, DE; POP AF; DEC A; JR NZ
    let mut count = cpu.a;
    let mut hl = cpu.hl();
    let mut ix = cpu.ix;
    let de = 0x0020u16;
    cpu.set_de(de);
    loop {
        let saved_f = cpu.f;
        let tile = bus.read_byte(0, ix);
        bus.write_byte(0, hl, tile);
        ix = ix.wrapping_add(1);
        hl = cpu.add16(hl, de);
        cpu.f = saved_f;
        count = cpu.dec8(count);
        if (cpu.f & FLAG_Z) != 0 {
            break;
        }
    }
    cpu.a = count;
    cpu.set_hl(hl);
    cpu.ix = ix;
    cpu.ret_subroutine(bus);
}

fn decomp_draw_credit_text_and_bcd<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x15DB: PUSH AF; copy 6 bytes $3062..$3067 -> $8016..$801B ("CREDIT"),
    // draw BCD credits ($85A5) at $8013 (ones) and $8014 (tens or $7F); POP AF; RET
    let saved_a = cpu.a;
    let saved_f = cpu.f;
    for i in 0..6u16 {
        let b = roms.cpu0[(0x3062 + i) as usize];
        bus.write_byte(0, 0x8016 + i, b);
    }
    let cred = bus.read_byte(0, 0x85A5);
    let ones = (cred & 0x0F).wrapping_add(0x10);
    bus.write_byte(0, 0x8013, ones);
    let tens_nib = (cred >> 4) & 0x0F;
    let tens = if tens_nib == 0 {
        0x7F
    } else {
        tens_nib.wrapping_add(0x10)
    };
    bus.write_byte(0, 0x8014, tens);
    cpu.b = cred;
    cpu.c = 0;
    cpu.set_de(0x801C);
    cpu.set_hl(0x3068);
    cpu.a = saved_a;
    cpu.f = saved_f;
    cpu.ret_subroutine(bus);
}

fn decomp_init_round_counters_17f8<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x17F8: Initialize round counters ($840D..$8410) at game start / attract mode
    let s87cf = bus.read_byte(0, 0x87CF);
    cpu.a = s87cf;
    cpu.cb_op(0x5F, s87cf); // BIT 3, A
    let s8787 = bus.read_byte(0, 0x8787);
    let normal_init = if (s87cf & 0x08) != 0 {
        true
    } else {
        cpu.a = s8787;
        cpu.cb_op(0x4F, s8787); // BIT 1, A
        (s8787 & 0x02) != 0
    };
    if !normal_init {
        let mut r = bus.read_byte(0, 0x840D);
        cpu.a = r;
        cpu.sub8(0x00, false, false);
        if r != 0 {
            r = cpu.dec8(r);
            bus.write_byte(0, 0x840D, r);
        }
        cpu.c = r;
        let r2 = bus.read_byte(0, 0x840E);
        cpu.a = r2;
        cpu.sub8(0x00, false, false);
        if r2 == 0 {
            bus.write_byte(0, 0x840E, 0x01);
        }
        bus.write_byte(0, 0x840F, r);
        bus.write_byte(0, 0x8410, r);
        cpu.set_hl(0x8410);
    } else {
        for addr in 0x840D..0x8411u16 {
            bus.write_byte(0, addr, 0x00);
        }
        cpu.b = 0;
        cpu.set_hl(0x8411);
        cpu.a = 0x01;
        bus.write_byte(0, 0x840E, 0x01);
    }
    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x4F, s8657); // BIT 1, A
    if (s8657 & 0x02) == 0 {
        cpu.ret_subroutine(bus);
        return;
    }
    for addr in 0x840D..0x8411u16 {
        bus.write_byte(0, addr, 0x02);
    }
    cpu.b = 0;
    cpu.set_hl(0x8411);
    cpu.ret_subroutine(bus);
}

fn decomp_init_rock_state_array<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, base: u16) {
    // 0x1892 (base=$8425) / 0x18A7 (base=$8465): Initialize 8 rock state slots (8 bytes each)
    let mut hl = base;
    for _ in 0..8 {
        bus.write_byte(0, hl, 0x00);
        hl = hl.wrapping_add(1);
        bus.write_byte(0, hl, 0x90);
        for _ in 0..6 {
            hl = hl.wrapping_add(1);
            bus.write_byte(0, hl, 0x00);
        }
        hl = hl.wrapping_add(1);
    }
    cpu.a = 0;
    cpu.b = 0;
    cpu.set_hl(hl);
    cpu.ret_subroutine(bus);
}

fn decomp_carve_round_initial_tunnels_18ac<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x18AC: Carve initial monster tunnels and Dig Dug's center vertical shaft
    let r = bus.read_byte(0, 0x8627);
    cpu.a = r;
    cpu.sub8(0x10, false, false);
    if r >= 0x10 {
        cpu.and8(0x03);
        cpu.add8(0x0C, false);
    }
    let a = cpu.dec8(cpu.a);
    cpu.a = a;
    cpu.c = a;
    for _ in 0..4 {
        let cur = cpu.a;
        cpu.add8(cur, false);
    }
    let de_off = cpu.a as u16;
    cpu.set_de(de_off);
    let mut tbl_hl = cpu.add16(0x3168, de_off);
    for b in (1..=8u8).rev() {
        cpu.b = b;
        let lo = roms.cpu0[tbl_hl as usize] as u16;
        let hi = roms.cpu0[(tbl_hl + 1) as usize] as u16;
        let vram_ptr = (hi << 8) | lo;
        cpu.set_de(vram_ptr);
        cpu.a = hi as u8;
        cpu.and8(hi as u8);
        if hi != 0 {
            cpu.set_hl(vram_ptr);
            decomp_stamp_tunnel_segment_1928(cpu, bus);
        }
        tbl_hl = tbl_hl.wrapping_add(2);
        bus.write_byte(0, 0x6830, cpu.a);
    }
    cpu.b = 0;
    cpu.set_hl(0x81D0);
    cpu.c = 0x02;
    cpu.a = 0;
    cpu.xor8(0);
    decomp_stamp_tunnel_segment_1928(cpu, bus);

    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x4F, s8657); // BIT 1, A
    let mut carve_shaft = (s8657 & 0x02) != 0;
    if !carve_shaft {
        cpu.set_hl(0x8785);
        let s8400 = bus.read_byte(0, 0x8400);
        cpu.a = s8400;
        cpu.cb_op(0x4F, s8400);
        let v85 = bus.read_byte(0, 0x8785);
        if (s8400 & 0x02) != 0 {
            cpu.cb_op(0x4E, v85);
            if (v85 & 0x02) == 0 {
                cpu.ret_subroutine(bus);
                return;
            }
        } else {
            cpu.cb_op(0x46, v85);
            if (v85 & 0x01) == 0 {
                cpu.ret_subroutine(bus);
                return;
            }
        }
        carve_shaft = true;
    }
    if carve_shaft {
        for i in 0..0x0Fu16 {
            let b = roms.cpu0[(0x190A + i) as usize];
            bus.write_byte(0, 0x8202 + i, b);
        }
        for i in 0..0x0Fu16 {
            let b = roms.cpu0[(0x1919 + i) as usize];
            bus.write_byte(0, 0x8222 + i, b);
        }
        cpu.set_bc(0x0000);
        cpu.set_de(0x8231);
        cpu.set_hl(0x1928);
        cpu.f &= !(FLAG_N | FLAG_H | FLAG_PV);
    }
    cpu.ret_subroutine(bus);
}

fn decomp_stamp_tunnel_segment_1928<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x1928: Stamp a vertical (B >= 5) or horizontal (B < 5) pre-dug tunnel into VRAM at HL
    let saved_bc = cpu.bc();
    cpu.a = cpu.b;
    cpu.sub8(0x05, false, false);
    if cpu.b >= 5 {
        let orig_hl = cpu.hl();
        let mut hl = orig_hl;
        bus.write_byte(0, hl, 0x00);
        for _ in 0..3 {
            hl = hl.wrapping_add(1);
            bus.write_byte(0, hl, 0x02);
        }
        cpu.a = cpu.c;
        cpu.sub8(0x00, false, false);
        if cpu.c == 0 {
            for _ in 0..4 {
                hl = hl.wrapping_add(1);
                bus.write_byte(0, hl, 0x02);
            }
        }
        hl = hl.wrapping_add(1);
        bus.write_byte(0, hl, 0x02);
        hl = hl.wrapping_add(1);
        bus.write_byte(0, hl, 0x04);

        cpu.set_de(0x0020);
        hl = cpu.add16(orig_hl, 0x0020);
        bus.write_byte(0, hl, 0x01);
        for _ in 0..4 {
            hl = hl.wrapping_add(1);
            bus.write_byte(0, hl, 0x03);
        }
        cpu.a = cpu.c;
        cpu.sub8(0x00, false, false);
        if cpu.c == 0 {
            for _ in 0..4 {
                hl = hl.wrapping_add(1);
                bus.write_byte(0, hl, 0x03);
            }
        }
        hl = hl.wrapping_add(1);
        bus.write_byte(0, hl, 0x05);
        cpu.set_hl(hl);
        cpu.set_bc(saved_bc);
    } else {
        cpu.set_de(0x0020);
        let mut hl = cpu.hl();
        bus.write_byte(0, hl, 0x0A);
        bus.write_byte(0, hl.wrapping_add(1), 0x0B);
        cpu.a = cpu.c;
        cpu.sub8(0x00, false, false);
        let cols = if cpu.c == 0 { 6 } else { 4 };
        for _ in 0..cols {
            hl = cpu.add16(hl, 0x0020);
            bus.write_byte(0, hl, 0x08);
            bus.write_byte(0, hl.wrapping_add(1), 0x09);
        }
        hl = cpu.add16(hl, 0x0020);
        bus.write_byte(0, hl, 0x06);
        hl = hl.wrapping_add(1);
        bus.write_byte(0, hl, 0x07);
        cpu.set_hl(hl);
        cpu.set_bc(saved_bc);
    }
}

fn decomp_convert_byte_at_hl_to_bcd<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let hl = cpu.hl();
    let mut a = bus.read_byte(0, hl);
    let mut d = 0x00u8;
    let mut e = 0x01u8;
    let mut b = cpu.b;
    cpu.a = a;
    cpu.and8(a);
    while cpu.a != 0 {
        let carry_out = (cpu.a & 0x01) != 0;
        let carry_in = if (cpu.f & FLAG_C) != 0 { 0x80 } else { 0x00 };
        cpu.a = (cpu.a >> 1) | carry_in;
        cpu.f = (cpu.f & (FLAG_S | FLAG_Z | FLAG_PV))
            | (cpu.a & (crate::z80::FLAG_X | crate::z80::FLAG_Y))
            | if carry_out { FLAG_C } else { 0 };
        b = cpu.a;
        if carry_out {
            cpu.a = d;
            cpu.add8(e, false);
            cpu.daa();
            d = cpu.a;
        }
        cpu.a = e;
        cpu.add8(e, false);
        cpu.daa();
        e = cpu.a;
        a = b;
        cpu.a = a;
        cpu.and8(a);
    }
    bus.write_byte(0, hl, d);
    cpu.b = b;
    cpu.d = d;
    cpu.e = e;
}

fn decomp_spawn_round_monsters_from_rom<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let r = bus.read_byte(0, 0x8627);
    cpu.a = r;
    cpu.sub8(0x10, false, false);
    if r >= 0x10 {
        cpu.and8(0x03);
        cpu.add8(0x0C, false);
    }
    let a = cpu.dec8(cpu.a);
    cpu.a = a;
    for _ in 0..3 {
        let cur = cpu.a;
        cpu.add8(cur, false);
    }
    let offset = cpu.a as u16;
    let de = 0x0010u16;
    let mut ix = 0x3258u16.wrapping_add(offset);
    let mut hl = 0x8529u16;
    let mut last_c = cpu.c;
    for _ in 0..8 {
        let c = roms.cpu0[ix as usize];
        last_c = c;
        cpu.a = c;
        cpu.and8(c);
        if c == 0 {
            ix = ix.wrapping_add(1);
            let status_addr = hl.wrapping_sub(1);
            let st = bus.read_byte(0, status_addr) | 0x80;
            bus.write_byte(0, status_addr, st);
        } else {
            for _ in 0..4 {
                let cur = cpu.a;
                cpu.add8(cur, false);
            }
            let x = cpu.inc8(cpu.a);
            cpu.a = x;
            bus.write_byte(0, hl, x);
            cpu.a = c;
            cpu.and8(0xF0);
            bus.write_byte(0, hl.wrapping_add(1), cpu.a);
            ix = ix.wrapping_add(1);
        }
        hl = cpu.add16(hl, de);
    }
    cpu.b = 0;
    cpu.c = last_c;
    cpu.set_de(de);
    cpu.set_hl(hl);
    cpu.ix = ix;
    cpu.ret_subroutine(bus);
}

fn decomp_sanitize_spawned_monster_slots<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let de = 0x0010u16;
    let mut hl = 0x8528u16;
    for _ in 0..8 {
        let v = bus.read_byte(0, hl);
        cpu.cb_op(0x7E, v); // BIT 7, (HL)
        if (v & 0x80) != 0 {
            bus.write_byte(0, hl.wrapping_add(1), 0x00);
            bus.write_byte(0, hl.wrapping_add(2), 0x00);
        }
        let base = hl.wrapping_sub(3);
        let b0 = bus.read_byte(0, base);
        cpu.a = b0;
        cpu.and8(0x01);
        bus.write_byte(0, base, cpu.a);
        hl = cpu.add16(hl, de);
    }
    cpu.b = 0;
    cpu.set_de(de);
    cpu.set_hl(hl);
    cpu.ret_subroutine(bus);
}

fn decomp_init_monster_default_sprites<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let de = 0x0010u16;
    let mut hl = 0x852Bu16;
    for b in (1..=8u8).rev() {
        cpu.a = b;
        cpu.sub8(0x05, false, false);
        if b >= 5 {
            bus.write_byte(0, hl, 0x20);
            bus.write_byte(0, hl.wrapping_add(1), 0x01);
            bus.write_byte(0, hl.wrapping_add(2), 0x00);
        } else {
            bus.write_word(0, 0x85F6, hl);
            let base = hl.wrapping_sub(6);
            let v = bus.read_byte(0, base) | 0x01;
            bus.write_byte(0, base, v);
            bus.write_byte(0, hl, 0x24);
            bus.write_byte(0, hl.wrapping_add(1), 0x02);
            bus.write_byte(0, hl.wrapping_add(2), 0x00);
        }
        hl = cpu.add16(hl, de);
    }
    cpu.b = 0;
    cpu.set_de(de);
    cpu.set_hl(hl);
    cpu.ret_subroutine(bus);
}

fn decomp_spawn_round_rocks_from_rom<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let r = bus.read_byte(0, 0x8627);
    cpu.a = r;
    cpu.sub8(0x10, false, false);
    if r >= 0x10 {
        cpu.and8(0x03);
        cpu.add8(0x0C, false);
    }
    let a = cpu.dec8(cpu.a);
    cpu.a = a;
    for _ in 0..3 {
        let cur = cpu.a;
        cpu.add8(cur, false);
    }
    let offset = cpu.a as u16;
    let mut ix = 0x30F0u16.wrapping_add(offset);
    let mut hl = 0x8467u16;
    let mut iy = 0x8465u16;
    let de = 0x0008u16;
    for _ in 0..8 {
        let st = bus.read_byte(0, hl);
        let raw = roms.cpu0[ix as usize];
        if (st & 0x01) != 0 || raw == 0 {
            bus.write_byte(0, hl, 0xFF);
            bus.write_byte(0, iy, 0x00);
            bus.write_byte(0, iy.wrapping_add(1), 0x60);
        } else {
            let x = (raw << 4).wrapping_add(1);
            let y = raw & 0xF0;
            bus.write_byte(0, iy, x);
            bus.write_byte(0, iy.wrapping_add(1), y);
        }
        ix = ix.wrapping_add(1);
        iy = iy.wrapping_add(de);
        hl = hl.wrapping_add(de);
    }
    let mut spr_hl = 0x983Cu16;
    let mut spr_de = 0x993Cu16;
    for _ in 0..8 {
        bus.write_byte(0, spr_hl, 0x1E);
        bus.write_byte(0, spr_hl.wrapping_add(1), 0x04);
        spr_hl = spr_hl.wrapping_add(2);
        bus.write_byte(0, spr_de, 0x00);
        spr_de = spr_de.wrapping_add(2);
    }
    cpu.a = 0;
    cpu.and8(0);
    cpu.b = 0;
    cpu.set_hl(spr_hl);
    cpu.set_de(spr_de);
    cpu.ix = ix;
    cpu.iy = iy;
    cpu.ret_subroutine(bus);
}

fn decomp_init_digdug_player_spawn<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    bus.write_byte(0, 0x9822, 0x02);
    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x9823, 0x00);
    bus.write_byte(0, 0x9880, 0x00);
    bus.write_byte(0, 0x9881, 0x00);
    bus.write_byte(0, 0x9800, 0x38);
    bus.write_byte(0, 0x9801, 0x06);

    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x4F, s8657); // BIT 1, A
    let mut center_spawn = (s8657 & 0x02) != 0;
    if !center_spawn {
        cpu.set_hl(0x8785);
        let s8400 = bus.read_byte(0, 0x8400);
        cpu.a = s8400;
        cpu.cb_op(0x4F, s8400);
        let h_val = bus.read_byte(0, 0x8785);
        if (s8400 & 0x02) != 0 {
            cpu.cb_op(0x4E, h_val);
            if (h_val & 0x02) != 0 {
                center_spawn = true;
            }
        } else {
            cpu.cb_op(0x46, h_val);
            if (h_val & 0x01) != 0 {
                center_spawn = true;
            }
        }
    }
    if !center_spawn {
        bus.write_byte(0, 0x98A3, 0x08);
        bus.write_byte(0, 0x98A2, 0xB1);
        cpu.a = 0xB1;
    } else {
        bus.write_byte(0, 0x98A2, 0x71);
        bus.write_byte(0, 0x98A3, 0x80);
        cpu.set_hl(0x8401);
        let v = bus.read_byte(0, 0x8401) | 0x10;
        bus.write_byte(0, 0x8401, v);
        cpu.a = 0x80;
    }
    cpu.ret_subroutine(bus);
}

fn decomp_setup_player_ready_sprites_1c94<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    for i in 0..6u16 {
        bus.write_byte(0, 0x98FA + i, roms.cpu0[(0x130D + i) as usize]);
        bus.write_byte(0, 0x987A + i, roms.cpu0[(0x1313 + i) as usize]);
    }
    for i in 0..4u16 {
        bus.write_byte(0, 0x9978 + i * 2, 0x00);
    }
    let s8400 = bus.read_byte(0, 0x8400);
    cpu.a = s8400;
    cpu.cb_op(0x4F, s8400);
    let num_code = if (s8400 & 0x02) != 0 { 0x6B } else { 0x6A };
    bus.write_byte(0, 0x9878, num_code);
    bus.write_byte(0, 0x9879, 0x0C);
    bus.write_byte(0, 0x98F8, 0x89);
    bus.write_byte(0, 0x98F9, 0x70);

    for i in 0..6u16 {
        bus.write_byte(0, 0x98DE + i, roms.cpu0[(0x1CC7 + i) as usize]);
        bus.write_byte(0, 0x985E + i, roms.cpu0[(0x1CCD + i) as usize]);
    }
    let mut hl = 0x995Eu16;
    for _ in 0..3 {
        bus.write_byte(0, hl, 0x00);
        hl = hl.wrapping_add(2);
    }
    cpu.set_bc(0x0000);
    cpu.set_de(0x9864);
    cpu.set_hl(hl);
    cpu.f &= !(FLAG_N | FLAG_H | FLAG_PV);
    cpu.ret_subroutine(bus);
}

fn decomp_add_score_from_table_2fb5_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.h = 0x00;
    let de_table = 0x302Bu16;
    let mut hl = cpu.add16(cpu.hl(), de_table);
    let mut de = 0x8411u16;
    for i in 0..3 {
        let b = roms.cpu0[hl as usize];
        cpu.a = b;
        bus.write_byte(0, de, b);
        if i < 2 {
            hl = hl.wrapping_add(1);
            de = de.wrapping_add(1);
        }
    }
    cpu.set_hl(hl);
    cpu.set_de(de);
    decomp_add_score_2fc8_body(cpu, bus);
}

fn decomp_add_score_2fc8_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let s8657 = bus.read_byte(0, 0x8657);
    cpu.a = s8657;
    cpu.cb_op(0x4F, s8657); // BIT 1, A
    if (s8657 & 0x02) != 0 {
        return;
    }
    let s9a81 = bus.read_byte(0, 0x9A81);
    cpu.a = s9a81;
    cpu.sub8(0x00, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        return;
    }
    let mut de = 0x8411u16;
    let mut hl = 0x89EFu16;
    cpu.and8(cpu.a);
    for _ in 0..3 {
        let a = bus.read_byte(0, de);
        let m = bus.read_byte(0, hl);
        let c_in = (cpu.f & FLAG_C) != 0;
        cpu.a = a;
        cpu.add8(m, c_in);
        cpu.daa();
        bus.write_byte(0, hl, cpu.a);
        hl = hl.wrapping_add(1);
        de = de.wrapping_add(1);
    }
    let m3 = bus.read_byte(0, hl);
    let c_in = (cpu.f & FLAG_C) != 0;
    cpu.a = 0x00;
    cpu.add8(m3, c_in);
    cpu.daa();
    bus.write_byte(0, hl, cpu.a);

    let s8400 = bus.read_byte(0, 0x8400);
    cpu.a = s8400;
    cpu.cb_op(0x4F, s8400); // BIT 1, A
    de = if (s8400 & 0x02) != 0 { 0x8417 } else { 0x8414 };
    hl = 0x8411;
    cpu.and8(cpu.a);
    for _ in 0..3 {
        let a = bus.read_byte(0, de);
        let m = bus.read_byte(0, hl);
        let c_in = (cpu.f & FLAG_C) != 0;
        cpu.a = a;
        cpu.add8(m, c_in);
        cpu.daa();
        bus.write_byte(0, de, cpu.a);
        de = de.wrapping_add(1);
        hl = hl.wrapping_add(1);
    }
    de = de.wrapping_sub(1);
    std::mem::swap(&mut de, &mut hl);
    bus.write_word(0, 0x8627, hl);
    de = 0x8929;
    let mut b = 3u8;
    loop {
        let hi_b = bus.read_byte(0, de);
        let sc_b = bus.read_byte(0, hl);
        cpu.a = hi_b;
        cpu.sub8(sc_b, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            if (cpu.f & FLAG_C) == 0 {
                cpu.b = b;
                cpu.set_hl(hl);
                cpu.set_de(de);
                return;
            }
            let mut dst_hl = 0x8929u16;
            let mut src_de = bus.read_word(0, 0x8627);
            for _ in 0..3 {
                let v = bus.read_byte(0, src_de);
                cpu.a = v;
                bus.write_byte(0, dst_hl, v);
                dst_hl = dst_hl.wrapping_sub(1);
                src_de = src_de.wrapping_sub(1);
            }
            cpu.b = 0;
            cpu.set_hl(dst_hl);
            cpu.set_de(src_de);
            return;
        }
        hl = hl.wrapping_sub(1);
        de = de.wrapping_sub(1);
        b = b.wrapping_sub(1);
        if b == 0 {
            cpu.b = 0;
            cpu.set_hl(hl);
            cpu.set_de(de);
            return;
        }
    }
}

fn decomp_monster_try_horizontal_dir_2d3a<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let base = bus.read_word(0, 0x85F6);
    let st0 = bus.read_byte(0, base);
    cpu.cb_op(0x46, st0); // BIT 0, (HL)
    let mut target_x = bus.read_byte(0, 0x98A2);
    let x_hl = cpu.add16(base, 0x0004);
    let mx = bus.read_byte(0, x_hl);
    if (st0 & 0x01) == 0 {
        let f0 = bus.read_byte(0, 0x87F0);
        cpu.cb_op(0x46, f0);
        if (f0 & 0x01) != 0 {
            target_x = 0x00;
            cpu.a = 0;
            cpu.xor8(0);
        }
        cpu.a = target_x;
        cpu.sub8(mx, false, false);
        if target_x == mx {
            cpu.set_hl(x_hl);
            cpu.c = 0x00;
            return;
        }
    } else {
        cpu.a = target_x;
        cpu.sub8(mx, false, false);
    }
    let dir = if target_x >= mx { 0x02u8 } else { 0x06u8 };
    decomp_finish_dir_choice_2d90(cpu, bus, x_hl.wrapping_add(1), dir);
}

fn decomp_monster_try_vertical_dir_2d67<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let base = bus.read_word(0, 0x85F6);
    let st0 = bus.read_byte(0, base);
    cpu.cb_op(0x46, st0); // BIT 0, (HL)
    let mut target_y = bus.read_byte(0, 0x98A3);
    let y_hl = cpu.add16(base, 0x0005);
    let my = bus.read_byte(0, y_hl);
    if (st0 & 0x01) == 0 {
        let f0 = bus.read_byte(0, 0x87F0);
        cpu.cb_op(0x46, f0);
        if (f0 & 0x01) != 0 {
            target_y = 0x08;
        }
        cpu.a = target_y;
        cpu.sub8(my, false, false);
        if target_y == my {
            cpu.set_hl(y_hl);
            cpu.c = 0x00;
            return;
        }
    } else {
        cpu.a = target_y;
        cpu.sub8(my, false, false);
    }
    let dir = if target_y >= my {
        0x04u8
    } else {
        cpu.a = 0;
        cpu.xor8(0);
        0x00u8
    };
    decomp_finish_dir_choice_2d90(cpu, bus, y_hl, dir);
}

fn decomp_finish_dir_choice_2d90<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, y_hl: u16, dir: u8) {
    cpu.set_de(0x000A);
    let dir_hl = cpu.add16(y_hl, 0x000A);
    bus.write_byte(0, dir_hl, dir);
    cpu.a = dir;
    cpu.and8(0x07);
    let b = cpu.a;
    cpu.b = b;
    let prev_hl = dir_hl.wrapping_sub(1);
    cpu.set_hl(prev_hl);
    let prev = bus.read_byte(0, prev_hl);
    cpu.a = prev;
    cpu.add8(0x04, false);
    cpu.and8(0x07);
    cpu.sub8(b, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.c = 0x00;
    } else {
        decomp_monster_check_dir_walkable_2da8(cpu, bus);
    }
}

fn decomp_monster_check_dir_walkable_2da8<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let dir_ptr = bus.read_word(0, 0x861A);
    let raw_dir = bus.read_byte(0, dir_ptr);
    cpu.a = raw_dir;
    cpu.and8(0x07);
    let dir = cpu.a;
    let vertical = match dir {
        0 => {
            let t0 = bus.read_word(0, 0x8602);
            bus.write_word(0, 0x8612, t0);
            bus.write_word(0, 0x8616, t0.wrapping_sub(1));
            let t1 = bus.read_word(0, 0x8604);
            bus.write_word(0, 0x8614, t1);
            bus.write_word(0, 0x8618, t1.wrapping_sub(1));
            true
        }
        2 => {
            cpu.sub8(0x02, false, false);
            cpu.set_de(0xFFE0);
            let t0 = bus.read_word(0, 0x8604);
            bus.write_word(0, 0x8612, t0);
            let t0_n = cpu.add16(t0, 0xFFE0);
            bus.write_word(0, 0x8616, t0_n);
            let t1 = bus.read_word(0, 0x8608);
            bus.write_word(0, 0x8614, t1);
            let t1_n = cpu.add16(t1, 0xFFE0);
            bus.write_word(0, 0x8618, t1_n);
            false
        }
        4 => {
            cpu.sub8(0x04, false, false);
            let t0 = bus.read_word(0, 0x8606);
            bus.write_word(0, 0x8612, t0);
            bus.write_word(0, 0x8616, t0.wrapping_add(1));
            let t1 = bus.read_word(0, 0x8608);
            bus.write_word(0, 0x8614, t1);
            let t1_n = t1.wrapping_add(1);
            bus.write_word(0, 0x8618, t1_n);
            cpu.set_hl(t1_n);
            let f0 = bus.read_byte(0, 0x87F0);
            cpu.a = f0;
            cpu.cb_op(0x47, f0);
            if (f0 & 0x01) != 0 {
                cpu.c = 0x00;
                return;
            }
            true
        }
        _ => {
            cpu.sub8(0x04, false, false);
            cpu.set_de(0x0020);
            let t0 = bus.read_word(0, 0x8602);
            bus.write_word(0, 0x8612, t0);
            let t0_n = cpu.add16(t0, 0x0020);
            bus.write_word(0, 0x8616, t0_n);
            let t1 = bus.read_word(0, 0x8606);
            bus.write_word(0, 0x8614, t1);
            let t1_n = cpu.add16(t1, 0x0020);
            bus.write_word(0, 0x8618, t1_n);
            false
        }
    };

    for (i, ptr_addr) in [0x8612u16, 0x8614, 0x8616, 0x8618].iter().enumerate() {
        let hl = bus.read_word(0, *ptr_addr);
        cpu.set_hl(hl);
        if vertical {
            decomp_check_vert_tunnel_tile_2e42_body(cpu, bus);
        } else {
            decomp_check_horiz_tunnel_tile_2e84_body(cpu, bus);
        }
        if i < 3 {
            cpu.a = cpu.c;
            cpu.sub8(0x00, false, false);
            if cpu.c == 0x00 {
                return;
            }
        }
    }
}

fn decomp_check_vert_tunnel_tile_2e42_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let mut a = bus.read_byte(0, cpu.hl());
    cpu.a = a;
    for &t in &[0x02u8, 0x03, 0x7E, 0x8E, 0x8F] {
        cpu.sub8(t, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.c = 0x01;
            return;
        }
    }
    a &= 0x7F;
    cpu.a = a;
    cpu.sub8(0x0C, false, false);
    cpu.c = if (cpu.f & FLAG_Z) != 0 { 0x01 } else { 0x00 };
}

fn decomp_check_horiz_tunnel_tile_2e84_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let mut a = bus.read_byte(0, cpu.hl());
    cpu.a = a;
    for &t in &[0x8Du8, 0x7E, 0x08, 0x09] {
        cpu.sub8(t, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.c = 0x01;
            return;
        }
    }
    a &= 0x7F;
    cpu.a = a;
    cpu.sub8(0x0C, false, false);
    cpu.c = if (cpu.f & FLAG_Z) != 0 { 0x01 } else { 0x00 };
}

fn decomp_check_open_tunnel_tile_2ea6_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let a = bus.read_byte(0, cpu.hl());
    cpu.a = a;
    for &t in &[0x8Cu8, 0x7E, 0x09, 0x08, 0x0C, 0x02, 0x8D, 0x03] {
        cpu.sub8(t, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x01;
            return;
        }
    }
    cpu.a = 0;
    cpu.xor8(0);
}

fn decomp_fygar_check_fire_clearance_2ed4<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let saved_hl = cpu.hl();
    let dir_ptr = bus.read_word(0, 0x861A).wrapping_sub(1);
    let dir = bus.read_byte(0, dir_ptr) & 0x07;
    cpu.a = dir;
    cpu.and8(0x07);
    cpu.sub8(0x02, false, false);
    if dir == 0x02 {
        let p0 = bus.read_word(0, 0x8602);
        let p1 = bus.read_word(0, 0x8606);
        bus.write_word(0, 0x8636, p0);
        bus.write_word(0, 0x8638, p1);
    } else {
        cpu.sub8(0x06, false, false);
        if dir == 0x06 {
            let p0 = bus.read_word(0, 0x8604);
            let p1 = bus.read_word(0, 0x8608);
            bus.write_word(0, 0x8636, p0);
            bus.write_word(0, 0x8638, p1);
        } else {
            cpu.b = 0x03;
            cpu.set_hl(saved_hl);
            return;
        }
    }

    let mut b = 0x03u8;
    loop {
        let d_now = bus.read_byte(0, dir_ptr) & 0x07;
        cpu.a = d_now;
        cpu.and8(0x07);
        let step_de = if d_now == 0x02 {
            cpu.sub8(0x02, false, false);
            0xFFC0u16
        } else if d_now == 0x06 {
            cpu.sub8(0x06, false, false);
            0x0040u16
        } else {
            cpu.a = 0;
            cpu.xor8(0);
            cpu.b = b;
            cpu.set_hl(saved_hl);
            return;
        };
        cpu.set_de(step_de);
        let t0 = cpu.add16(bus.read_word(0, 0x8636), step_de);
        bus.write_word(0, 0x8636, t0);
        let t1 = cpu.add16(bus.read_word(0, 0x8638), step_de);
        bus.write_word(0, 0x8638, t1);

        cpu.set_hl(t0);
        decomp_check_open_tunnel_tile_2ea6_body(cpu, bus);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.a & 0x01) == 0 {
            break;
        }
        cpu.set_hl(t1);
        decomp_check_open_tunnel_tile_2ea6_body(cpu, bus);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.a & 0x01) == 0 {
            break;
        }
        b = b.wrapping_sub(1);
        if b == 0 {
            break;
        }
    }
    cpu.b = b;
    let base = bus.read_word(0, 0x85F6);
    cpu.set_de(0x0008);
    let range_hl = cpu.add16(base, 0x0008);
    bus.write_byte(0, range_hl, b);
    cpu.set_hl(saved_hl);
}

fn decomp_get_monster_timer_864b_ptr<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    // 0x2F7A: HL = $864B + ((($85F6) - $8525) >> 4)
    let ptr = bus.read_word(0, 0x85F6);
    cpu.set_hl(ptr);
    cpu.set_de(0x8525);
    cpu.and8(cpu.a);
    cpu.sbc_hl(0x8525);
    let idx = (cpu.l.rotate_right(4)) & 0x0F;
    cpu.a = idx;
    cpu.and8(0x0F);
    let de = idx as u16;
    cpu.set_de(de);
    let hl = cpu.add16(0x864B, de);
    cpu.set_hl(hl);
}

fn decomp_clear_big_score_row_35d5_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let de_saved = cpu.de();
    for _ in 0..0x18 {
        bus.write_byte(0, cpu.de(), 0x40);
        decomp_step_vram_de_left_36e9_body(cpu);
    }
    cpu.b = 0;
    cpu.set_de(de_saved.wrapping_add(1));
}

fn decomp_write_digit_tile_step_left_3687_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.add8(0x10, false);
    cpu.or8(cpu.c);
    bus.write_byte(0, cpu.de(), cpu.a);
    decomp_step_vram_de_left_36e9_body(cpu);
}

fn decomp_step_vram_de_left_36e9_body(cpu: &mut Z80) {
    // 0x36E9: LD A, E; SUB $20; LD E, A; RET NC; DEC D; RET
    cpu.a = cpu.e;
    cpu.sub8(0x20, false, true);
    cpu.e = cpu.a;
    if (cpu.f & FLAG_C) != 0 {
        cpu.d = cpu.dec8(cpu.d);
    }
}

fn decomp_render_string_descriptor_36f3<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    _roms: &DigDugRoms,
) {
    let mut hl = cpu.hl();
    let c = bus.read_byte(0, hl);
    hl = hl.wrapping_add(1);
    let mut b = bus.read_byte(0, hl);
    hl = hl.wrapping_add(1);
    let e = bus.read_byte(0, hl);
    hl = hl.wrapping_add(1);
    let d = bus.read_byte(0, hl);
    hl = hl.wrapping_add(1);
    cpu.c = c;
    cpu.b = b;
    cpu.set_de(((d as u16) << 8) | (e as u16));
    loop {
        cpu.a = bus.read_byte(0, hl);
        decomp_ascii_to_tile_370c_body(cpu);
        cpu.sub8(0xFF, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.add8(cpu.c, false);
            bus.write_byte(0, cpu.de(), cpu.a);
            decomp_step_vram_de_left_36e9_body(cpu);
        }
        hl = hl.wrapping_add(1);
        b = b.wrapping_sub(1);
        cpu.b = b;
        if b == 0 {
            break;
        }
    }
    cpu.set_hl(hl);
    cpu.ret_subroutine(bus);
}

fn decomp_ascii_to_tile_370c_body(cpu: &mut Z80) {
    // 0x370C: Convert ASCII byte in A to Dig Dug tile index
    cpu.sub8(0x30, false, true);
    cpu.sub8(0x0A, false, false);
    if (cpu.f & FLAG_C) != 0 {
        return;
    }
    cpu.sub8(0x07, false, true);
    cpu.sub8(0x23, false, false);
    if (cpu.f & FLAG_C) != 0 {
        return;
    }
    cpu.add8(0x3E, false);
    cpu.sub8(0x27, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    cpu.add8(0xFC, false);
    cpu.sub8(0x24, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    cpu.add8(0xF4, false);
    cpu.sub8(0x25, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    cpu.add8(0x08, false);
    cpu.sub8(0x26, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    cpu.a = 0xFF;
}

fn decomp_decode_dip_switches_3a85<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let b_ce = bus.read_byte(0, 0x87CE);
    let d0 = (b_ce << 1) & 0x0E;
    bus.write_byte(0, 0x898A, d0);
    let d1 = (b_ce >> 2) & 0x0E;
    bus.write_byte(0, 0x898B, d1);
    let d2 = (b_ce.rotate_left(2).wrapping_add(1)) & 0x03;
    bus.write_byte(0, 0x898C, d2);

    let b_cf = bus.read_byte(0, 0x87CF);
    let bit0 = b_cf & 0x01;
    let bit1 = (b_cf >> 1) & 0x01;
    let d3 = (bit0 << 1) | bit1;
    bus.write_byte(0, 0x898D, d3);
    let d4 = b_cf & 0x04;
    bus.write_byte(0, 0x898E, d4);
    let d5 = (b_cf.rotate_left(2)) & 0x03;
    bus.write_byte(0, 0x898F, d5);

    cpu.a = d5;
    cpu.and8(0x03);
    cpu.c = b_cf >> 2;
    cpu.set_hl(0x87CF);
    cpu.set_de(0x898F);
    cpu.ret_subroutine(bus);
}

fn decomp_clear_24_cols_with_37_3bb5_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let de = 0xFFE0u16;
    cpu.set_de(de);
    let mut hl = cpu.hl();
    for _ in 0..0x18 {
        bus.write_byte(0, hl, 0x37);
        hl = cpu.add16(hl, de);
    }
    cpu.b = 0;
    cpu.set_hl(hl);
}

fn decomp_draw_crosshatch_byte_3d15_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    _roms: &DigDugRoms,
) {
    let de = cpu.de();
    let mut a = bus.read_byte(0, de);
    let mut hl = cpu.hl();
    for _ in 0..8 {
        cpu.a = a;
        cpu.add8(a, false);
        a = cpu.a;
        if (cpu.f & FLAG_C) != 0 {
            bus.write_byte(0, hl, 0x18);
        }
        hl = hl.wrapping_add(1);
    }
    cpu.c = 0;
    cpu.f = (cpu.f & !(FLAG_S | FLAG_N)) | FLAG_Z;
    cpu.set_de(de.wrapping_add(1));
    cpu.set_hl(hl.wrapping_add(1));
}

fn decomp_clear_screen_and_sprites_3e33_3e3b_body<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    start_hl: u16,
    bc_count: u16,
) {
    let mut hl = start_hl;
    let mut b = (bc_count >> 8) as u8;
    let mut c = (bc_count & 0xFF) as u8;
    loop {
        loop {
            bus.write_byte(0, hl, 0x37);
            hl = hl.wrapping_add(1);
            c = c.wrapping_sub(1);
            if c == 0 {
                break;
            }
        }
        b = b.wrapping_sub(1);
        if b == 0 {
            break;
        }
    }
    bus.write_byte(0, 0xA003, 0x01);
    for base in [0x8B80u16, 0x9380, 0x9B80] {
        for i in 0..0x80u16 {
            bus.write_byte(0, base + i, 0x00);
        }
    }
    cpu.a = 0;
    cpu.xor8(0);
    cpu.b = 0;
    cpu.c = 0;
    cpu.set_hl(0x9C00);
}

fn decomp_update_rocks_and_monsters_1f00<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x1F00: Dispatch 1 or 2 rocks and 4 monsters per frame based on ($8423) & 3
    let f23_word = bus.read_word(0, 0x8423);
    cpu.set_hl(f23_word);
    let phase = (f23_word as u8) & 0x03;
    cpu.a = phase;
    cpu.and8(0x03);
    let mon_start = match phase {
        3 => {
            cpu.sub8(0x03, false, false);
            cpu.set_hl(0x8475);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            cpu.set_hl(0x847D);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            0x8565u16
        }
        2 => {
            cpu.sub8(0x02, false, false);
            cpu.set_hl(0x8485);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            cpu.set_hl(0x848D);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            0x8525u16
        }
        1 => {
            cpu.sub8(0x01, false, false);
            cpu.set_hl(0x8495);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            0x8565u16
        }
        _ => {
            cpu.sub8(0x01, false, false);
            cpu.set_hl(0x846D);
            decomp_update_single_rock_1f75(cpu, bus, roms);
            0x8525u16
        }
    };

    let mut hl = mon_start;
    let de = 0x0010u16;
    let saved_c = cpu.c;
    for b in (1..=4u8).rev() {
        bus.write_byte(0, 0x8629, b);
        cpu.set_hl(hl);
        cpu.a = b;
        cpu.and8(b);
        cpu.sbc_hl(0x8525);
        let idx = (cpu.l >> 4) & 0x0F;
        cpu.c = idx;
        cpu.a = 0x08;
        cpu.and8(0x08);
        cpu.sub8(idx, false, true);
        bus.write_byte(0, 0x879D, cpu.a);

        cpu.set_hl(hl);
        decomp_update_single_monster_2339(cpu, bus, roms);
        cpu.set_de(de);
        cpu.set_bc(((b as u16) << 8) | (saved_c as u16));
        hl = cpu.add16(hl, de);
    }
    cpu.b = 0;
    cpu.c = saved_c;
    cpu.set_de(de);
    cpu.set_hl(hl);
}

fn decomp_update_single_rock_1f75<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x1F75..0x22CA: Complete falling rock state machine for rock slot at HL
    let rock_base = cpu.hl();
    let st_addr = rock_base.wrapping_add(2);
    let st = bus.read_byte(0, st_addr);
    cpu.cb_op(0x66, st); // BIT 4, (HL)
    if (st & 0x10) != 0 {
        cpu.set_hl(rock_base);
        return;
    }
    cpu.cb_op(0x7E, st); // BIT 7, (HL)
    if (st & 0x80) != 0 {
        decomp_rock_shatter_21bc(cpu, bus, roms, rock_base, st_addr);
        return;
    }

    let rx = bus.read_byte(0, rock_base);
    cpu.d = rx;
    cpu.a = rx;
    cpu.and8(rx);
    let mut unsupported = false;
    if rx != 0 {
        let ry = bus.read_byte(0, rock_base.wrapping_add(1));
        cpu.e = ry;
        cpu.a = ry;
        cpu.sub8(0xF0, false, false);
        if ry < 0xF0 {
            let saved_bc = cpu.bc();
            decomp_populate_4tile_quad(cpu, bus, 0, 0x85FA);
            let tile_bl = bus.read_word(0, 0x85FE);
            let tile_br = bus.read_word(0, 0x8600);
            let below_l = tile_bl.wrapping_add(1);
            let below_r = tile_br.wrapping_add(1);
            bus.write_word(0, 0x85EA, below_l);
            bus.write_word(0, 0x85EC, below_r);
            let tl_val = bus.read_byte(0, below_l);
            cpu.a = tl_val;
            cpu.sub8(0x7F, false, false);
            if tl_val != 0x7F {
                let tr_val = bus.read_byte(0, below_r);
                cpu.a = tr_val;
                cpu.sub8(0x7F, false, false);
                if tr_val != 0x7F {
                    decomp_cap_rock_tunnel_tile(cpu, bus, tile_bl);
                    decomp_cap_rock_tunnel_tile(cpu, bus, tile_br);
                }
            }
            cpu.set_bc(saved_bc);
            cpu.set_de(((rx as u16) << 8) | (ry as u16));

            let ea_ptr = bus.read_word(0, 0x85EA);
            let ea_t = bus.read_byte(0, ea_ptr);
            if matches!(ea_t, 0x09 | 0x08 | 0x0C | 0x03 | 0x01 | 0x05 | 0x06 | 0x07) {
                let ec_ptr = bus.read_word(0, 0x85EC);
                let ec_t = bus.read_byte(0, ec_ptr);
                if matches!(ec_t, 0x09 | 0x08 | 0x0C | 0x02 | 0x00 | 0x04 | 0x0A | 0x0B) {
                    unsupported = true;
                }
            }
        }
    }

    let mut cur_st = bus.read_byte(0, st_addr);
    if !unsupported {
        cur_st &= !0x02;
        bus.write_byte(0, st_addr, cur_st);
    } else {
        let v_f0 = bus.read_byte(0, 0x87F0) | 0x02;
        bus.write_byte(0, 0x87F0, v_f0);
        cur_st |= 0x03;
        bus.write_byte(0, st_addr, cur_st);
    }

    cpu.cb_op(0x46, cur_st); // BIT 0, (HL)
    if (cur_st & 0x01) == 0 {
        cpu.set_hl(rock_base);
        return;
    }
    cpu.cb_op(0x4E, cur_st); // BIT 1, (HL)
    if (cur_st & 0x02) == 0 {
        decomp_rock_shatter_21bc(cpu, bus, roms, rock_base, st_addr);
        return;
    }
    cpu.cb_op(0x56, cur_st); // BIT 2, (HL)
    if (cur_st & 0x04) == 0 {
        bus.write_byte(0, rock_base.wrapping_add(4), 0x0C);
        bus.write_byte(0, st_addr, cur_st | 0x04);
        cpu.set_hl(rock_base);
        return;
    }

    // 0x200E: Wobble countdown at rock_base + 4
    let timer_hl = rock_base.wrapping_add(4);
    cpu.set_hl(timer_hl);
    let t = bus.read_byte(0, timer_hl);
    cpu.a = t;
    if t == 0x0C || t == 0x06 {
        let spr_hl = decomp_rock_ptr_to_sprite_290c(cpu, timer_hl);
        bus.write_byte(0, spr_hl, 0x1F);
        cpu.set_hl(timer_hl);
    } else if t == 0x09 || t == 0x03 {
        let spr_hl = decomp_rock_ptr_to_sprite_290c(cpu, timer_hl);
        bus.write_byte(0, spr_hl, 0x1E);
        cpu.set_hl(timer_hl);
    } else if t < 0x0A {
        let mut m_hl = 0x8533u16;
        for _ in 0..8 {
            let d = bus.read_byte(0, m_hl);
            if (d & 0x07) == 0 {
                bus.write_byte(0, m_hl, (d & 0x07).wrapping_add(0x04));
            }
            m_hl = m_hl.wrapping_add(0x0010);
        }
        cpu.b = 0;
        cpu.set_de(0x0010);
        cpu.set_hl(timer_hl);
    }

    cpu.a = t;
    cpu.and8(t);
    if t != 0 {
        cpu.sub8(0x01, false, false);
        if t == 0x01 {
            bus.write_byte(0, 0x9A8D, 0x01);
        }
        let nt = cpu.dec8(t);
        bus.write_byte(0, timer_hl, nt);
        cpu.set_hl(rock_base);
        return;
    }

    // 0x2063: Rock actively falling! Check collision with Dig Dug and all 8 monsters
    cpu.ix = rock_base;
    let s8646 = bus.read_byte(0, 0x8646);
    cpu.a = s8646;
    cpu.cb_op(0x47, s8646); // BIT 0, A
    if (s8646 & 0x01) != 0 {
        let py = bus.read_byte(0, 0x98A3);
        cpu.a = py;
        cpu.add8(0x08, false);
        let npy = cpu.a.min(0xEE);
        bus.write_byte(0, 0x98A3, npy);
    } else {
        let rx_now = bus.read_byte(0, rock_base);
        let px = bus.read_byte(0, 0x98A2);
        let dx = rx_now.abs_diff(px);
        if dx < 0x07 {
            let ry_now = bus.read_byte(0, rock_base.wrapping_add(1));
            let py = bus.read_byte(0, 0x98A3);
            let dy = py.wrapping_sub(ry_now);
            if dy < 0x12 {
                bus.write_byte(0, 0x9922, 0x00);
                let v46 = bus.read_byte(0, 0x8646) | 0x01;
                bus.write_byte(0, 0x8646, v46);
                bus.write_byte(0, 0x9A8A, 0x01);
                bus.write_byte(0, rock_base.wrapping_add(3), 0xFF);
                let npy = py.wrapping_sub(0x02);
                bus.write_byte(0, 0x98A3, npy);
            }
        }
    }

    // 0x20B6: Check rock against all 8 monsters ($8525, stride $10)
    let mut m_base = 0x8525u16;
    let mut m_pos = 0x8529u16;
    bus.write_word(0, 0x85F6, m_base);
    bus.write_word(0, 0x861A, m_pos);
    let de = 0x0010u16;
    for _ in 0..8 {
        let st0 = bus.read_byte(0, m_base);
        if (st0 & 0x80) != 0 {
            let my = bus.read_byte(0, m_pos.wrapping_add(1)).wrapping_add(0x08);
            bus.write_byte(0, m_pos.wrapping_add(1), my);
        } else if (st0 & 0x20) == 0 {
            let spr = bus.read_byte(0, m_base.wrapping_add(6));
            let rx_now = bus.read_byte(0, rock_base);
            let ry_now = bus.read_byte(0, rock_base.wrapping_add(1));
            let mut crushed = false;
            if spr >= 0x3F {
                let s8400 = bus.read_byte(0, 0x8400);
                let mut mx = bus.read_byte(0, m_pos);
                if (s8400 & 0x06) == 0x06 {
                    mx = mx.wrapping_sub(0x10);
                }
                let x_ok = if rx_now < mx {
                    mx.wrapping_sub(rx_now) < 0x09
                } else {
                    rx_now.wrapping_sub(mx) < 0x19
                };
                if x_ok {
                    let mut my = bus.read_byte(0, m_pos.wrapping_add(1));
                    if (s8400 & 0x06) == 0x06 {
                        my = my.wrapping_sub(0x10);
                    }
                    if my.wrapping_sub(ry_now) < 0x0C {
                        let new_my = ry_now.wrapping_add(0x08);
                        bus.write_byte(0, m_pos.wrapping_add(1), new_my);
                        let cur_mx = bus.read_byte(0, m_pos);
                        let new_mx = if (s8400 & 0x06) == 0x06 {
                            cur_mx.wrapping_sub(0x08)
                        } else {
                            cur_mx.wrapping_add(0x08)
                        };
                        bus.write_byte(0, m_pos, new_mx);
                        if (st0 & 0x01) == 0 {
                            bus.write_byte(0, m_base.wrapping_add(6), 0x22);
                            bus.write_byte(0, m_base.wrapping_add(7), 0x01);
                        } else {
                            bus.write_byte(0, m_base.wrapping_add(6), 0x26);
                            bus.write_byte(0, m_base.wrapping_add(7), 0x02);
                        }
                        crushed = true;
                    }
                }
            } else {
                let mx = bus.read_byte(0, m_pos);
                if rx_now.abs_diff(mx) < 0x0C {
                    let my = bus.read_byte(0, m_pos.wrapping_add(1));
                    if my.wrapping_sub(ry_now) < 0x0C {
                        crushed = true;
                    }
                }
            }
            if crushed {
                let ns0 = (st0 & 0xC3) | 0x80;
                bus.write_byte(0, m_base, ns0);
                let d0d = bus.read_byte(0, m_base.wrapping_add(0x0D)) & !0x0C;
                bus.write_byte(0, m_base.wrapping_add(0x0D), d0d);
                bus.write_byte(0, 0x9A8A, 0x01);
                let vc5 = bus.read_byte(0, 0x87C5) | 0x01;
                bus.write_byte(0, 0x87C5, vc5);
                bus.write_byte(0, rock_base.wrapping_add(3), 0xFF);
                let my = bus.read_byte(0, m_pos.wrapping_add(1)).wrapping_sub(0x02);
                bus.write_byte(0, m_pos.wrapping_add(1), my);
            }
        }
        m_base = m_base.wrapping_add(de);
        m_pos = m_pos.wrapping_add(de);
        bus.write_word(0, 0x85F6, m_base);
        bus.write_word(0, 0x861A, m_pos);
    }

    // Advance rock Y by +8 pixels
    let y_addr = rock_base.wrapping_add(1);
    let old_ry = bus.read_byte(0, y_addr);
    cpu.a = 0x08;
    cpu.add8(old_ry, false);
    bus.write_byte(0, y_addr, cpu.a);
    cpu.b = 0;
    cpu.set_de(de);
    cpu.set_hl(y_addr);
}

fn decomp_rock_ptr_to_sprite_290c(cpu: &mut Z80, hl_in: u16) -> u16 {
    let diff = hl_in.wrapping_sub(0x8469);
    let idx = ((diff & 0xFF) as u8) >> 2;
    cpu.a = idx;
    cpu.set_de(idx as u16);
    0x983C + (idx as u16)
}

fn decomp_rock_shatter_21bc<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    rock_base: u16,
    st_addr: u16,
) {
    // 0x21BC..0x22CA: Rock impact, shatter animation, and crushed-monster score award
    let st = bus.read_byte(0, st_addr);
    cpu.cb_op(0x5E, st); // BIT 3, (HL)
    if (st & 0x08) == 0 {
        bus.write_byte(0, st_addr, st | 0x88);
        bus.write_byte(0, rock_base.wrapping_add(4), 0x14);
        cpu.set_de(0x0003);
        let hit_hl = cpu.add16(rock_base, 0x0003);
        let hit_flag = bus.read_byte(0, hit_hl);
        cpu.cb_op(0x46, hit_flag);
        if (hit_flag & 0x01) != 0 {
            let y_hl = rock_base.wrapping_add(1);
            cpu.set_hl(y_hl);
            let s8646 = bus.read_byte(0, 0x8646);
            cpu.a = s8646;
            cpu.cb_op(0x47, s8646);
            if (s8646 & 0x01) == 0 {
                let ry = bus.read_byte(0, y_hl);
                cpu.a = ry;
                cpu.sub8(0x08, false, true);
                bus.write_byte(0, y_hl, cpu.a);
            }
        } else {
            cpu.set_hl(hit_hl);
        }
        cpu.a = 0x01;
        bus.write_byte(0, 0x9A8C, 0x01);
        return;
    }

    // 0x226B: Shatter countdown at rock_base + 4
    let timer_hl = rock_base.wrapping_add(4);
    cpu.set_hl(timer_hl);
    let t = bus.read_byte(0, timer_hl);
    cpu.a = t;
    cpu.and8(t);
    if t == 0 {
        cpu.a = 0;
        cpu.xor8(0);
        bus.write_byte(0, rock_base, 0x00);
        bus.write_byte(0, rock_base.wrapping_add(1), 0x90);
        bus.write_byte(0, rock_base.wrapping_add(2), 0x11);
        for off in 3..=7u16 {
            bus.write_byte(0, rock_base.wrapping_add(off), 0x00);
        }
        bus.write_byte(0, 0x98F4, 0x00);
        bus.write_byte(0, 0x98F5, 0x00);
        bus.write_byte(0, 0x98F8, 0x00);
        bus.write_byte(0, 0x98F9, 0x00);
        let v99 = bus.read_byte(0, 0x8799) | 0x01;
        bus.write_byte(0, 0x8799, v99);
        cpu.set_hl(0x87F0);
        let vf0 = bus.read_byte(0, 0x87F0) & !0x02;
        bus.write_byte(0, 0x87F0, vf0);
        return;
    }

    let nt = cpu.dec8(t);
    bus.write_byte(0, timer_hl, nt);
    cpu.a = t;
    cpu.sub8(0x04, false, false);
    if t == 0x04 {
        bus.write_byte(0, 0x87C5, 0x00);
        let mut m_hl = 0x8525u16;
        let mut c = 0u8;
        for _ in 0..8 {
            bus.write_word(0, 0x85F6, m_hl);
            let st0 = bus.read_byte(0, m_hl);
            cpu.cb_op(0x7E, st0);
            if (st0 & 0x80) != 0 {
                c = c.wrapping_add(1);
                bus.write_byte(0, m_hl, 0x00);
                let b3 = bus.read_byte(0, m_hl.wrapping_add(3)) | 0x80;
                bus.write_byte(0, m_hl.wrapping_add(3), b3);
                bus.write_byte(0, m_hl.wrapping_add(4), 0x00);
                bus.write_byte(0, m_hl.wrapping_add(5), 0x00);
            }
            m_hl = cpu.add16(m_hl, 0x0010);
            bus.write_word(0, 0x85F6, m_hl);
        }
        bus.write_byte(0, 0x8647, 0x3C);
        bus.write_byte(0, 0x861C, 0xC8);
        cpu.set_hl(0x8646);
        let v46 = bus.read_byte(0, 0x8646);
        cpu.cb_op(0x46, v46);
        if (v46 & 0x01) != 0 {
            bus.write_byte(0, 0x8646, v46 | 0x02);
        }
        cpu.c = c;
        cpu.a = c;
        cpu.and8(c);
        if c == 0 {
            cpu.b = 0;
            cpu.set_de(0x0010);
            cpu.set_hl(rock_base);
            return;
        }
        cpu.b = 0x3E;
        cpu.add8(0x3E, false);
        bus.write_byte(0, 0x9874, cpu.a);
        bus.write_byte(0, 0x9878, 0x47);
        bus.write_byte(0, 0x9875, 0x0A);
        bus.write_byte(0, 0x9879, 0x0A);
        bus.write_byte(0, 0x9974, 0x00);
        bus.write_byte(0, 0x9978, 0x00);

        let rx = bus.read_byte(0, rock_base);
        let lx = rx.wrapping_sub(0x08);
        bus.write_byte(0, 0x98F4, lx);
        bus.write_byte(0, 0x98F8, lx.wrapping_add(0x10));
        bus.write_byte(0, rock_base, 0x00);
        let ry = bus.read_byte(0, rock_base.wrapping_add(1));
        bus.write_byte(0, 0x98F5, ry);
        bus.write_byte(0, 0x98F9, ry);
        bus.write_byte(0, rock_base.wrapping_add(1), 0x00);

        cpu.l = c.wrapping_mul(3).wrapping_add(0x15);
        decomp_add_score_from_table_2fb5_body(cpu, bus, roms);
        return;
    }

    let c5 = bus.read_byte(0, 0x87C5);
    cpu.cb_op(0x46, c5);
    if (c5 & 0x01) != 0 {
        cpu.set_hl(rock_base);
        return;
    }
    if t == 0x12 {
        let spr_hl = decomp_rock_ptr_to_sprite_290c(cpu, timer_hl);
        bus.write_byte(0, spr_hl, 0x74);
    } else if t == 0x09 {
        let spr_hl = decomp_rock_ptr_to_sprite_290c(cpu, timer_hl);
        bus.write_byte(0, spr_hl, 0x75);
    } else if t == 0x05 {
        let spr_hl = decomp_rock_ptr_to_sprite_290c(cpu, timer_hl);
        bus.write_byte(0, spr_hl, 0x32);
    } else {
        cpu.sub8(0x05, false, false);
    }
    cpu.set_hl(rock_base);
}

fn decomp_update_single_monster_2339<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_hl = cpu.hl();
    decomp_update_monster_sprites(cpu, bus);
    cpu.set_hl(saved_hl);
    let s8401 = bus.read_byte(0, 0x8401);
    cpu.a = s8401;
    cpu.cb_op(0x7F, s8401); // BIT 7, A
    if (s8401 & 0x80) != 0 {
        return;
    }
    decomp_step_monster_ai_and_motion_2348(cpu, bus, roms);
}

fn decomp_step_monster_ai_and_motion_2348<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // 0x2348..0x2D39: Complete per-monster AI, deflation, ghosting (`mecha-ru`), and Fygar fire state machine
    let mon_base = cpu.hl();
    let st0 = bus.read_byte(0, mon_base);
    cpu.a = st0;
    cpu.and8(0xBC);
    if (st0 & 0xBC) != 0 {
        decomp_monster_deflate_or_pop_2531(cpu, bus, roms, mon_base);
        cpu.set_hl(mon_base);
        return;
    }

    cpu.cb_op(0x4E, st0); // BIT 1, (HL) -> 1 = ghosting (`mecha-ru`), 0 = normal tunnel walking
    if (st0 & 0x02) == 0 {
        decomp_monster_normal_tunnel_ai_291f(cpu, bus, roms, mon_base);
        cpu.set_hl(mon_base);
        return;
    }

    // 0x2354..0x2520: Ghosting (`mecha-ru`) movement and tunnel re-entry
    bus.write_word(0, 0x85F6, mon_base);
    let b0b_hl = cpu.add16(mon_base, 0x000B);
    let b0b = bus.read_byte(0, b0b_hl);
    cpu.a = b0b;
    cpu.and8(b0b);
    if b0b != 0 {
        cpu.sub8(0x01, false, false);
        if b0b == 0x01 {
            let b1 = bus.read_byte(0, mon_base.wrapping_add(1)) & !0x01;
            bus.write_byte(0, mon_base.wrapping_add(1), b1);
        }
        let nb0b = cpu.dec8(b0b);
        bus.write_byte(0, b0b_hl, nb0b);
    }

    let b3_hl = mon_base.wrapping_add(3);
    let b3 = bus.read_byte(0, b3_hl);
    cpu.cb_op(0x7E, b3); // BIT 7, (HL)
    if (b3 & 0x80) != 0 {
        cpu.set_hl(mon_base);
        return;
    }

    // Fractional speed accumulator at mon_base + 9
    let spd = bus.read_byte(0, 0x87BD);
    cpu.b = spd;
    let acc_hl = cpu.add16(mon_base, 0x0009);
    let acc = bus.read_byte(0, acc_hl);
    cpu.a = acc;
    cpu.sub8(spd, false, true);
    bus.write_byte(0, acc_hl, cpu.a);
    if (cpu.f & FLAG_C) == 0 {
        cpu.set_hl(mon_base);
        return;
    }

    let x_hl = cpu.add16(mon_base, 0x0004);
    let mx = bus.read_byte(0, x_hl);
    cpu.a = mx;
    cpu.sub8(0x01, false, false);
    if mx == 0x01 {
        let nb3 = bus.read_byte(0, b3_hl) | 0x80;
        bus.write_byte(0, b3_hl, nb3);
        cpu.set_hl(mon_base);
        return;
    }

    // Step PRNG ($1CF8)
    let seed = bus.read_word(0, 0x85F4).wrapping_mul(5).wrapping_add(1);
    bus.write_word(0, 0x85F4, seed);

    let b1_hl = mon_base.wrapping_add(1);
    let b1 = bus.read_byte(0, b1_hl) & !0x02;
    bus.write_byte(0, b1_hl, b1);
    let my = bus.read_byte(0, x_hl.wrapping_add(1));
    cpu.b = mx;
    cpu.c = my;

    let b3_now = bus.read_byte(0, b3_hl);
    let f0 = bus.read_byte(0, 0x87F0);
    if (b3_now & 0x40) == 0 && (f0 & 0x01) == 0 {
        let px = bus.read_byte(0, 0x98A2);
        let py = bus.read_byte(0, 0x98A3);
        if px.abs_diff(mx) < 0x20 && py.abs_diff(my) < 0x20 {
            bus.write_byte(0, mon_base.wrapping_add(0x0E), px);
            bus.write_byte(0, mon_base.wrapping_add(0x0F), py);
            bus.write_byte(0, b3_hl, b3_now | 0x40);
        }
    }

    // Move ghosting monster 2px toward target along Y (if PRNG bit 1 != 0) or X
    let prng_lo = (seed & 0xFF) as u8;
    let b3_after = bus.read_byte(0, b3_hl);
    if (prng_lo & 0x02) != 0 {
        let target_y = if (b3_after & 0x40) != 0 && (f0 & 0x01) == 0 {
            bus.read_byte(0, mon_base.wrapping_add(0x0F))
        } else if (f0 & 0x01) != 0 {
            0x08
        } else {
            bus.read_byte(0, 0x98A3)
        };
        if target_y != my {
            let nb1 = bus.read_byte(0, b1_hl) | 0x02;
            bus.write_byte(0, b1_hl, nb1);
            let y_hl = mon_base.wrapping_add(5);
            let nmy = if target_y > my {
                my.wrapping_add(2)
            } else {
                my.wrapping_sub(2)
            };
            bus.write_byte(0, y_hl, nmy);
        }
    } else {
        let target_x = if (b3_after & 0x40) != 0 && (f0 & 0x01) == 0 {
            bus.read_byte(0, mon_base.wrapping_add(0x0E))
        } else if (f0 & 0x01) != 0 {
            0x21
        } else {
            bus.read_byte(0, 0x98A2)
        };
        if target_x != mx {
            let nmx = if target_x > mx {
                mx.wrapping_add(2)
            } else {
                mx.wrapping_sub(2)
            };
            bus.write_byte(0, x_hl, nmx);
        }
    }

    // 0x2490: Check if ghosting monster can materialize into an open tunnel
    let v4a = bus.read_byte(0, 0x864A) | 0x40;
    bus.write_byte(0, 0x864A, v4a);
    loop {
        let cur_b1 = bus.read_byte(0, b1_hl);
        cpu.cb_op(0x46, cur_b1);
        let mut can_reenter = false;
        if (cur_b1 & 0x01) == 0 {
            cpu.b = 0x00;
            if (cur_b1 & 0x02) == 0 {
                let cur_x = bus.read_byte(0, mon_base.wrapping_add(4));
                if (cur_x & 0x0F) == 0x01 {
                    cpu.b |= 0x01;
                }
            } else {
                let cur_y = bus.read_byte(0, mon_base.wrapping_add(5));
                cpu.c = cur_y;
                cpu.d = cur_y & 0x0F;
                if (cur_y >= 0x20 && (cur_y & 0x0F) == 0) || (cur_y < 0x20 && cur_y == 0x08) {
                    cpu.b |= 0x01;
                }
            }
            if (cpu.b & 0x01) != 0 {
                cpu.d = bus.read_byte(0, mon_base.wrapping_add(4));
                cpu.e = bus.read_byte(0, mon_base.wrapping_add(5));
                decomp_populate_4tile_quad(cpu, bus, 0, 0x85FA);
                let mut c = 4u8;
                for ptr_addr in [0x85FAu16, 0x85FC, 0x85FE, 0x8600] {
                    let tp = bus.read_word(0, ptr_addr);
                    cpu.set_hl(tp);
                    let t = bus.read_byte(0, tp) & 0x7F;
                    cpu.a = t;
                    cpu.and8(t);
                    if t < 0x10 || t == 0x7E {
                        c = cpu.dec8(c);
                    }
                }
                cpu.c = c;
                cpu.a = c;
                cpu.and8(c);
                if c == 0 {
                    cpu.b |= 0x01;
                    can_reenter = true;
                } else {
                    cpu.b &= !0x01;
                }
            }
        }
        if can_reenter {
            let ns0 = bus.read_byte(0, mon_base) & !0x02;
            bus.write_byte(0, mon_base, ns0);
            if (cur_b1 & 0x02) == 0 {
                let y_hl = mon_base.wrapping_add(5);
                let cur_y = bus.read_byte(0, y_hl);
                if cur_y < 0x16 {
                    bus.write_byte(0, y_hl, 0x08);
                    cpu.a = cur_y;
                    cpu.sub8(0x16, false, false);
                    return;
                }
                let c_add = if cur_y < 0x20 || (cur_y & 0x0F) >= 0x08 {
                    0x10u8
                } else {
                    0x00u8
                };
                cpu.c = c_add;
                cpu.a = cur_y & 0xF0;
                cpu.add8(c_add, false);
                bus.write_byte(0, y_hl, cpu.a);
                return;
            } else {
                let x_hl = mon_base.wrapping_add(4);
                let cur_x = bus.read_byte(0, x_hl);
                if (cur_x & 0x0F) >= 0x08 {
                    cpu.a = cur_x & 0xF0;
                    cpu.add8(0x11, false);
                } else {
                    cpu.a = cpu.inc8(cur_x & 0xF0);
                }
                bus.write_byte(0, x_hl, cpu.a);
                return;
            }
        }

        let cur_4a = bus.read_byte(0, 0x864A);
        cpu.cb_op(0x76, cur_4a);
        if (cur_4a & 0x40) == 0 {
            return;
        }
        bus.write_byte(0, 0x864A, cur_4a & !0x40);
        bus.write_byte(0, b1_hl, cur_b1 ^ 0x02);
    }
}

fn decomp_monster_deflate_or_pop_2531<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    mon_base: u16,
) {
    bus.write_word(0, 0x85F6, mon_base);
    let f2_hl = mon_base.wrapping_add(2);
    let f2 = bus.read_byte(0, f2_hl);
    cpu.cb_op(0x66, f2); // BIT 4, (HL)
    if (f2 & 0x10) != 0 {
        return;
    }
    let st0 = bus.read_byte(0, mon_base);
    cpu.cb_op(0x46, st0); // BIT 0, (HL) -> if Fygar, clear its fire sprites ($2637)
    if (st0 & 0x01) != 0 {
        let slot = bus.read_byte(0, 0x8629);
        let mut fire_hl = match slot {
            1 => 0x98AAu16,
            2 => 0x98B0u16,
            3 => 0x98A4u16,
            _ => 0x98B6u16,
        };
        for _ in 0..3 {
            bus.write_byte(0, fire_hl, 0x00);
            bus.write_byte(0, fire_hl.wrapping_add(1), 0x50);
            fire_hl = fire_hl.wrapping_add(2);
        }
    }

    cpu.a = st0;
    cpu.and8(0x3C);
    if (st0 & 0x3C) == 0 {
        return;
    }
    let timer_hl = cpu.add16(mon_base, 0x000C);
    let nt = cpu.dec8(bus.read_byte(0, timer_hl));
    bus.write_byte(0, timer_hl, nt);
    cpu.a = nt;
    cpu.sub8(0x0D, false, false);
    if nt == 0x0D {
        cpu.cb_op(0x6E, st0); // BIT 5, (HL) -> Stage 4 POP!
        if (st0 & 0x20) == 0 {
            return;
        }
        let x_hl = cpu.add16(mon_base, 0x0004);
        let mx = bus.read_byte(0, x_hl);
        let my = bus.read_byte(0, x_hl.wrapping_add(1));
        let s8400 = bus.read_byte(0, 0x8400);
        if (s8400 & 0x06) == 0x06 {
            bus.write_byte(0, 0x98FE, mx);
            bus.write_byte(0, 0x98F6, mx.wrapping_sub(0x10));
            let sy = my.wrapping_sub(0x09);
            bus.write_byte(0, 0x98FF, sy);
            bus.write_byte(0, 0x98F7, sy);
        } else {
            bus.write_byte(0, 0x98F6, mx);
            bus.write_byte(0, 0x98FE, mx.wrapping_add(0x10));
            let sy = my.wrapping_add(0x07);
            bus.write_byte(0, 0x98F7, sy);
            bus.write_byte(0, 0x98FF, sy);
        }
        bus.write_byte(0, mon_base, st0 | 0x20);
        let f2_v = bus.read_byte(0, f2_hl) | 0x01;
        bus.write_byte(0, f2_hl, f2_v);
        let b3_addr = mon_base.wrapping_add(3);
        let b3_v = bus.read_byte(0, b3_addr) | 0x80;
        bus.write_byte(0, b3_addr, b3_v);
        bus.write_byte(0, x_hl, 0xF1);
        bus.write_byte(0, x_hl.wrapping_add(1), 0x00);
        bus.write_byte(0, x_hl.wrapping_add(2), 0x32);
        let d_hl = cpu.add16(mon_base.wrapping_add(6), 0x0007);
        let d_v = bus.read_byte(0, d_hl) & !0x0C;
        bus.write_byte(0, d_hl, d_v);
        cpu.a = 0;
        cpu.xor8(0);
        bus.write_byte(0, 0x98A0, 0x00);
        bus.write_byte(0, 0x98A1, 0x00);
        let v8403 = bus.read_byte(0, 0x8403) & !0x04;
        bus.write_byte(0, 0x8403, v8403);
        return;
    }

    cpu.a = nt;
    cpu.and8(nt);
    if nt != 0 {
        return;
    }

    // Timer reached 0: step down inflation stage (4->3, 3->2, 2->0, or finish pop)
    let step = cpu.dec8(bus.read_byte(0, 0x8410));
    let tbl_hl = cpu.add16(0x32DF, step as u16);
    let def_time = roms.cpu0[tbl_hl as usize];
    bus.write_byte(0, timer_hl, def_time);

    if (st0 & 0x04) != 0 {
        bus.write_byte(0, mon_base, st0 & !(0x04 | 0x40));
        let d9d = bus.read_byte(0, 0x879D);
        let d9e = bus.read_byte(0, 0x879E);
        cpu.a = d9d;
        cpu.sub8(d9e, false, false);
        if d9d == d9e {
            for addr in 0x9882..0x988Au16 {
                bus.write_byte(0, addr, 0x00);
                bus.write_byte(0, 0x6830, cpu.a);
            }
            cpu.b = 0;
            let v8403 = bus.read_byte(0, 0x8403) & !0x04;
            bus.write_byte(0, 0x8403, v8403);
            let v8401 = bus.read_byte(0, 0x8401) & !0x08;
            bus.write_byte(0, 0x8401, v8401);
        }
    } else if (st0 & 0x08) != 0 {
        bus.write_byte(0, mon_base, (st0 & !0x08) | 0x04);
        let f2_v = bus.read_byte(0, f2_hl) | 0x20;
        bus.write_byte(0, f2_hl, f2_v);
    } else if (st0 & 0x10) != 0 {
        bus.write_byte(0, mon_base, (st0 & !0x10) | 0x08);
        let f2_v = bus.read_byte(0, f2_hl) | 0x20;
        bus.write_byte(0, f2_hl, f2_v);
    } else if (st0 & 0x20) != 0 {
        bus.write_byte(0, mon_base, st0 & !0x20);
        let v8404 = bus.read_byte(0, 0x8404) & !0x40;
        bus.write_byte(0, 0x8404, v8404);
        cpu.a = 0;
        cpu.xor8(0);
        bus.write_byte(0, 0x98F6, 0x00);
        bus.write_byte(0, 0x98F7, 0x00);
        bus.write_byte(0, 0x98FE, 0x00);
        bus.write_byte(0, 0x98FF, 0x00);
    }
}

fn decomp_monster_normal_tunnel_ai_291f<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    mon_base: u16,
) {
    // 0x291F..0x2D39: Normal tunnel walking, Fygar fire breathing, and intersection turning
    bus.write_word(0, 0x85F6, mon_base);
    let x_hl = cpu.add16(mon_base, 0x0004);
    let mx = bus.read_byte(0, x_hl);
    cpu.a = mx;
    cpu.sub8(0x01, false, false);
    if mx == 0x01 {
        let b3_hl = mon_base.wrapping_add(3);
        let b3_v = bus.read_byte(0, b3_hl) | 0x80;
        bus.write_byte(0, b3_hl, b3_v);
        return;
    }

    let dir_ptr = cpu.add16(x_hl, 0x000B);
    bus.write_word(0, 0x861A, dir_ptr);
    let f2_hl = mon_base.wrapping_add(2);
    let f2_v = bus.read_byte(0, f2_hl) & !0x30;
    bus.write_byte(0, f2_hl, f2_v);
    let b3_hl = mon_base.wrapping_add(3);
    let b3 = bus.read_byte(0, b3_hl) & !0x40;
    bus.write_byte(0, b3_hl, b3);
    cpu.cb_op(0x7E, b3);
    if (b3 & 0x80) != 0 {
        return;
    }

    // Select monster speed into B ($294A..$29D1)
    let st0 = bus.read_byte(0, mon_base);
    let is_fygar = (st0 & 0x01) != 0;
    let my = bus.read_byte(0, mon_base.wrapping_add(5));
    let mut b_spd = if my == 0x08 {
        let mut s = bus.read_byte(0, 0x87BD);
        if is_fygar {
            let (ns, ovf) = s.overflowing_add(0x0A);
            s = if ovf { 0xFF } else { ns };
        }
        s
    } else {
        let s8646 = bus.read_byte(0, 0x8646);
        let s8653 = bus.read_byte(0, 0x8653);
        let spd_addr = if (s8646 & 0x08) != 0 {
            if is_fygar { 0x87A5 } else { 0x87A2 }
        } else if (s8653 & 0x02) != 0 {
            if is_fygar { 0x87A4 } else { 0x87A1 }
        } else if is_fygar {
            0x87A3
        } else {
            0x87A0
        };
        bus.read_byte(0, spd_addr)
    };

    let cur_dir = bus.read_byte(0, mon_base.wrapping_add(0x0E)) & 0x07;
    if cur_dir == 0x04 {
        let s8657 = bus.read_byte(0, 0x8657);
        let r0f = bus.read_byte(0, 0x840F);
        if (s8657 & 0x02) != 0 || r0f >= 0x04 {
            let bonus = roms.cpu0[0x32EE];
            let sum = bonus.wrapping_add(b_spd);
            b_spd = if sum < 0x28 { 0xFF } else { sum };
        }
    }
    cpu.b = b_spd;

    // Subtract speed from fractional accumulator at mon_base + 9
    let acc_hl = cpu.add16(mon_base, 0x0009);
    let acc = bus.read_byte(0, acc_hl);
    cpu.a = acc;
    cpu.sub8(b_spd, false, true);
    bus.write_byte(0, acc_hl, cpu.a);
    if (cpu.f & FLAG_C) == 0 {
        return;
    }

    // Check X and Y grid alignment ($28A1 / $28B7)
    let x_aligned = (mx & 0x0F) == 0x01;
    let y_aligned = if my >= 0x20 {
        (my & 0x0F) == 0
    } else {
        my == 0x08
    };
    if !x_aligned && !y_aligned {
        decomp_enter_ghost_mode_2c71(cpu, bus, mon_base);
        return;
    }
    if !x_aligned || !y_aligned {
        decomp_step_monster_2px_in_dir_2ce2(cpu, bus, mon_base, dir_ptr);
        return;
    }

    // Grid intersection! Populate 4-tile quad at $8602..$8608
    cpu.d = mx;
    cpu.e = my;
    decomp_populate_4tile_quad(cpu, bus, 0, 0x8602);

    // Check Fygar fire-breathing trigger / active fire state ($2A07..$2C27)
    if is_fygar {
        let spr_hl = mon_base.wrapping_add(6);
        let spr = bus.read_byte(0, spr_hl);
        if spr != 0x2A && spr != 0x2B {
            if spr == 0x27 {
                bus.write_byte(0, spr_hl, 0x24);
            }
            let d_prev = bus.read_byte(0, dir_ptr.wrapping_sub(1));
            if (d_prev & 0x02) != 0 && mx >= 0x22 && mx < 0xD1 {
                decomp_fygar_check_fire_clearance_2ed4(cpu, bus);
                if cpu.b != 0x03 {
                    let b1_hl = mon_base.wrapping_add(1);
                    let b1 = bus.read_byte(0, b1_hl);
                    if (b1 & 0x40) != 0 {
                        decomp_update_fygar_fire_2aa4(cpu, bus, mon_base, dir_ptr);
                        return;
                    }
                    let r0f_idx = (bus.read_byte(0, 0x840F).wrapping_sub(1) & 0x0F) as u16;
                    let mut prob = roms.cpu0[(0x32D0 + r0f_idx) as usize];
                    let px = bus.read_byte(0, 0x98A2);
                    let py = bus.read_byte(0, 0x98A3);
                    if px.abs_diff(mx) < 0x3C && py.abs_diff(my) < 0x3C {
                        prob = prob.wrapping_shl(2);
                    }
                    let seed = bus.read_word(0, 0x85F4).wrapping_mul(5).wrapping_add(1);
                    bus.write_word(0, 0x85F4, seed);
                    if ((seed & 0xFF) as u8) < prob {
                        bus.write_byte(0, b1_hl, b1 | 0x40);
                        bus.write_byte(0, mon_base.wrapping_add(0x0B), 0x0E);
                        return;
                    }
                }
            }
        }
    }

    // 0x2C28: Choose tunnel direction at intersection
    let seed = bus.read_word(0, 0x85F4).wrapping_mul(5).wrapping_add(1);
    bus.write_word(0, 0x85F4, seed);
    let mut chosen = false;
    if (seed & 0x01) == 0 {
        decomp_monster_try_horizontal_dir_2d3a(cpu, bus);
        if (cpu.c & 0x01) != 0 {
            chosen = true;
        } else {
            decomp_monster_try_vertical_dir_2d67(cpu, bus);
            if (cpu.c & 0x01) != 0 {
                chosen = true;
            }
        }
    } else {
        decomp_monster_try_vertical_dir_2d67(cpu, bus);
        if (cpu.c & 0x01) != 0 {
            chosen = true;
        } else {
            decomp_monster_try_horizontal_dir_2d3a(cpu, bus);
            if (cpu.c & 0x01) != 0 {
                chosen = true;
            }
        }
    }

    if !chosen {
        let straight = bus.read_byte(0, dir_ptr.wrapping_sub(1));
        bus.write_byte(0, dir_ptr, straight);
        decomp_monster_check_dir_walkable_2da8(cpu, bus);
        if (cpu.c & 0x01) != 0 {
            chosen = true;
        }
    }

    if !chosen {
        let seed2 = bus.read_word(0, 0x85F4).wrapping_mul(5).wrapping_add(1);
        bus.write_word(0, 0x85F4, seed2);
        if ((seed2 & 0x0F) as u8) < 0x0A {
            let straight = bus.read_byte(0, dir_ptr.wrapping_sub(1));
            bus.write_byte(0, dir_ptr, straight.wrapping_sub(2));
            decomp_monster_check_dir_walkable_2da8(cpu, bus);
            if (cpu.c & 0x01) != 0 {
                decomp_get_monster_timer_864b_ptr(cpu, bus);
                bus.write_byte(0, cpu.hl(), 0x03);
                chosen = true;
            } else {
                bus.write_byte(0, dir_ptr, straight.wrapping_add(2));
                decomp_monster_check_dir_walkable_2da8(cpu, bus);
                if (cpu.c & 0x01) != 0 {
                    decomp_get_monster_timer_864b_ptr(cpu, bus);
                    let v = bus.read_byte(0, cpu.hl()).wrapping_add(1);
                    bus.write_byte(0, cpu.hl(), v);
                    chosen = true;
                }
            }
        }
        if !chosen {
            let rev = bus.read_byte(0, dir_ptr.wrapping_sub(1)).wrapping_add(4);
            bus.write_byte(0, dir_ptr, rev);
            decomp_monster_check_dir_walkable_2da8(cpu, bus);
            if (cpu.c & 0x01) != 0 {
                decomp_get_monster_timer_864b_ptr(cpu, bus);
                let v = bus.read_byte(0, cpu.hl()).wrapping_add(1);
                bus.write_byte(0, cpu.hl(), v);
                chosen = true;
            }
        }
    }

    if !chosen {
        decomp_enter_ghost_mode_2c71(cpu, bus, mon_base);
        return;
    }

    decomp_get_monster_timer_864b_ptr(cpu, bus);
    let cnt = bus.read_byte(0, cpu.hl());
    if cnt >= 0x06 && cnt < 0xC0 {
        decomp_enter_ghost_mode_2c71(cpu, bus, mon_base);
        return;
    }

    let new_d = bus.read_byte(0, dir_ptr) & 0x07;
    bus.write_byte(0, dir_ptr.wrapping_sub(1), new_d);
    if my == 0x08 && (bus.read_byte(0, 0x87F0) & 0x01) != 0 {
        bus.write_byte(0, dir_ptr.wrapping_sub(1), 0x06);
    }
    decomp_step_monster_2px_in_dir_2ce2(cpu, bus, mon_base, dir_ptr);
}

fn decomp_enter_ghost_mode_2c71<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, mon_base: u16) {
    let mb_v = bus.read_byte(0, mon_base) | 0x02;
    bus.write_byte(0, mon_base, mb_v);
    let b1_hl = mon_base.wrapping_add(1);
    let b1_v = bus.read_byte(0, b1_hl) | 0x01;
    bus.write_byte(0, b1_hl, b1_v);
    bus.write_byte(0, mon_base.wrapping_add(0x0B), 0x28);
    let d_hl = mon_base.wrapping_add(0x0E);
    let d_v = bus.read_byte(0, d_hl).wrapping_add(2);
    bus.write_byte(0, d_hl, d_v);
    decomp_get_monster_timer_864b_ptr(cpu, bus);
    bus.write_byte(0, cpu.hl(), 0x03);
}

fn decomp_step_monster_2px_in_dir_2ce2<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    mon_base: u16,
    dir_ptr: u16,
) {
    let my = bus.read_byte(0, mon_base.wrapping_add(5));
    if my == 0x08 && (bus.read_byte(0, 0x87F0) & 0x01) != 0 {
        bus.write_byte(0, dir_ptr.wrapping_sub(1), 0x06);
        let x_hl = mon_base.wrapping_add(4);
        let x_v = bus.read_byte(0, x_hl).wrapping_sub(2);
        bus.write_byte(0, x_hl, x_v);
        return;
    }
    let d = bus.read_byte(0, dir_ptr.wrapping_sub(1)) & 0x07;
    match d {
        0 => {
            let y_hl = mon_base.wrapping_add(5);
            let y_v = bus.read_byte(0, y_hl).wrapping_sub(2);
            bus.write_byte(0, y_hl, y_v);
        }
        2 => {
            let x_hl = mon_base.wrapping_add(4);
            let x_v = bus.read_byte(0, x_hl).wrapping_add(2);
            bus.write_byte(0, x_hl, x_v);
        }
        4 => {
            if (bus.read_byte(0, 0x87F0) & 0x01) != 0 {
                decomp_enter_ghost_mode_2c71(cpu, bus, mon_base);
                return;
            }
            let y_hl = mon_base.wrapping_add(5);
            let y_v = bus.read_byte(0, y_hl).wrapping_add(2);
            bus.write_byte(0, y_hl, y_v);
        }
        _ => {
            let x_hl = mon_base.wrapping_add(4);
            let x_v = bus.read_byte(0, x_hl).wrapping_sub(2);
            bus.write_byte(0, x_hl, x_v);
        }
    }
}

fn decomp_update_fygar_fire_2aa4<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    mon_base: u16,
    dir_ptr: u16,
) {
    let slot = bus.read_byte(0, 0x8629);
    let de_off = match slot {
        1 => 0x0006u16,
        2 => 0x000Cu16,
        3 => 0x0000u16,
        _ => 0x0012u16,
    };
    let spr_base = 0x9824u16.wrapping_add(de_off);
    let mut flip_bc = 0x9924u16.wrapping_add(de_off);
    let mut ix = 0x98A4u16.wrapping_add(de_off);
    let mut iy = 0x98A5u16.wrapping_add(de_off);

    let d = bus.read_byte(0, dir_ptr.wrapping_sub(1)) & 0x07;
    let mx = bus.read_byte(0, mon_base.wrapping_add(4));
    let my = bus.read_byte(0, mon_base.wrapping_add(5));
    let flip_val = if d == 0x02 {
        let mut x = mx;
        for i in 0..3 {
            x = x.wrapping_add(0x10);
            bus.write_byte(0, ix, x);
            if i < 2 {
                ix = ix.wrapping_add(2);
            }
        }
        0x00u8
    } else {
        let mut x = mx;
        for i in 0..3 {
            x = x.wrapping_sub(0x10);
            bus.write_byte(0, ix, x);
            if i < 2 {
                ix = ix.wrapping_add(2);
            }
        }
        0x02u8
    };
    for i in 0..3 {
        bus.write_byte(0, flip_bc, flip_val);
        if i < 2 {
            flip_bc = flip_bc.wrapping_add(2);
        }
    }
    for i in 0..3 {
        bus.write_byte(0, iy, my);
        if i < 2 {
            iy = iy.wrapping_add(2);
        }
    }
    cpu.ix = ix;
    cpu.iy = iy;
    bus.write_word(0, 0x8627, spr_base);

    let t_hl = mon_base.wrapping_add(0x0B);
    let c = cpu.dec8(bus.read_byte(0, t_hl));
    bus.write_byte(0, t_hl, c);
    let clearance = bus.read_byte(0, mon_base.wrapping_add(8));

    enum FireStep {
        FlapRed,
        FlapOrange,
        EndFire,
        Seg1,
        Seg2,
        Seg3,
        None,
    }
    let step = match clearance {
        0 => match c {
            0 => FireStep::EndFire,
            1 | 2 => FireStep::Seg3,
            3 => FireStep::Seg2,
            4 => FireStep::Seg1,
            5 | 8 | 12 => FireStep::FlapOrange,
            7 | 10 => FireStep::FlapRed,
            _ => FireStep::None,
        },
        1 => match c {
            1 => FireStep::EndFire,
            2 | 3 => FireStep::Seg2,
            4 => FireStep::Seg1,
            5 | 8 | 12 => FireStep::FlapOrange,
            7 | 10 => FireStep::FlapRed,
            _ => FireStep::None,
        },
        _ => match c {
            2 => FireStep::EndFire,
            3 | 4 => FireStep::Seg1,
            5 | 8 | 12 => FireStep::FlapOrange,
            7 | 10 => FireStep::FlapRed,
            _ => FireStep::None,
        },
    };

    match step {
        FireStep::FlapRed => {
            bus.write_byte(0, mon_base.wrapping_add(7), 0x02);
            decomp_clear_fire_pos_2f9d(cpu, bus);
        }
        FireStep::FlapOrange => {
            bus.write_byte(0, mon_base.wrapping_add(7), 0x0C);
            decomp_clear_fire_pos_2f9d(cpu, bus);
        }
        FireStep::EndFire => {
            let b1_hl = mon_base.wrapping_add(1);
            let b1_v = bus.read_byte(0, b1_hl) & !0x40;
            bus.write_byte(0, b1_hl, b1_v);
            decomp_clear_fire_pos_2f9d(cpu, bus);
            bus.write_byte(0, spr_base, 0x32);
            bus.write_byte(0, spr_base.wrapping_add(2), 0x32);
            bus.write_byte(0, spr_base.wrapping_add(4), 0x32);
            decomp_check_fire_player_collision_155f_body(cpu, bus);
        }
        FireStep::Seg1 => {
            bus.write_byte(0, spr_base, 0x2C);
            bus.write_byte(0, spr_base.wrapping_add(1), 0x05);
            bus.write_byte(0, spr_base.wrapping_add(2), 0x32);
            bus.write_byte(0, spr_base.wrapping_add(4), 0x32);
            cpu.ix = cpu.ix.wrapping_sub(4);
            cpu.iy = cpu.iy.wrapping_sub(4);
            decomp_check_fire_player_collision_155f_body(cpu, bus);
            bus.write_byte(0, 0x9A8E, 0x01);
        }
        FireStep::Seg2 => {
            bus.write_byte(0, spr_base, 0x2D);
            bus.write_byte(0, spr_base.wrapping_add(1), 0x05);
            bus.write_byte(0, spr_base.wrapping_add(2), 0x2E);
            bus.write_byte(0, spr_base.wrapping_add(3), 0x05);
            bus.write_byte(0, spr_base.wrapping_add(4), 0x32);
            cpu.ix = cpu.ix.wrapping_sub(2);
            cpu.iy = cpu.iy.wrapping_sub(2);
            decomp_check_fire_player_collision_155f_body(cpu, bus);
        }
        FireStep::Seg3 => {
            bus.write_byte(0, spr_base, 0x2F);
            bus.write_byte(0, spr_base.wrapping_add(1), 0x05);
            bus.write_byte(0, spr_base.wrapping_add(2), 0x30);
            bus.write_byte(0, spr_base.wrapping_add(3), 0x05);
            bus.write_byte(0, spr_base.wrapping_add(4), 0x31);
            bus.write_byte(0, spr_base.wrapping_add(5), 0x05);
            decomp_check_fire_player_collision_155f_body(cpu, bus);
        }
        FireStep::None => {}
    }
}

fn decomp_clear_fire_pos_2f9d<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let iy = cpu.iy;
    bus.write_byte(0, iy, 0x50);
    bus.write_byte(0, iy.wrapping_sub(2), 0x50);
    bus.write_byte(0, iy.wrapping_sub(4), 0x50);
    cpu.a = 0;
    cpu.xor8(0);
    let ix = cpu.ix;
    bus.write_byte(0, ix, 0x00);
    bus.write_byte(0, ix.wrapping_sub(2), 0x00);
    bus.write_byte(0, ix.wrapping_sub(4), 0x00);
}

fn decomp_check_fire_player_collision_155f_body<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let fx = bus.read_byte(0, cpu.ix);
    let px = bus.read_byte(0, 0x98A2);
    if px.abs_diff(fx) >= 0x0B {
        return;
    }
    let fy = bus.read_byte(0, cpu.iy);
    let py = bus.read_byte(0, 0x98A3);
    if py.abs_diff(fy) >= 0x0B {
        return;
    }
    let f6 = bus.read_word(0, 0x85F6).wrapping_add(4);
    bus.write_word(0, 0x85F6, f6);
    let v864a = bus.read_byte(0, 0x864A) | 0x01;
    bus.write_byte(0, 0x864A, v864a);
    decomp_trigger_player_hit_from_13a7(cpu, bus);
}

// =========================================================================
// CPU 1 PART 2: VBLANK ($0080), ATTRACT CUTSCENE ($049B), DIGGING ($130E,
// $140A, $14EF), EAROM ($1DDC), ROM CHECKSUM ($1F07), AND `step_human_cpu1`
// =========================================================================

#[inline]
fn call_cpu_sub<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    target_pc: u16,
    ret_pc: u16,
) {
    cpu.push(bus, ret_pc);
    cpu.pc = target_pc;
    let mut sub_stats = DecompStats::default();
    let ok = try_run_decompiled_subroutine(cpu, bus, roms, &mut sub_stats);
    SUB_STATS_ACCUM.with(|acc| acc.borrow_mut().merge(&sub_stats));
    debug_assert!(
        ok,
        "Missing decompiled subroutine (cpu={}, ${:04X})",
        cpu.cpu_id,
        target_pc
    );
}

/// CPU 1 `$1F07`: `cpu1_rom_checksum_init` (`$1F07..$1F25`)
/// Sums both 4 KB halves of CPU 1 ROM (`$0000..$0FFF` and `$1000..$1FFF`),
/// writes `$FF` to `$8A00`, and advances `PC` to the handshake loop at `$1F25`.
pub fn decomp_cpu1_rom_checksum_init<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.iff1 = false;
    cpu.iff2 = false;
    let mut sum2 = 0u8;
    for &b in &roms.cpu1[0x1000..0x2000] {
        sum2 = sum2.wrapping_add(b);
    }
    cpu.set_hl(0x2000);
    cpu.b = 0;
    cpu.a = sum2;
    cpu.sub8(roms.cpu1[0x1FFF], false, false);
    cpu.set_de(0x1FFF);
    cpu.e = cpu.inc8(cpu.e);
    cpu.a = 0xFF;
    bus.write_byte(1, 0x8A00, 0xFF);
    cpu.pc = 0x1F25;
}

/// CPU 1 `$02EA`: `carve_tunnel_reverse_tiles` (`$02EA..$0314`)
pub fn decomp_cpu1_clear_player_and_harpoon_sprites<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let hl_dc = bus.read_word(1, 0x85DC);
    cpu.set_hl(hl_dc);
    cpu.push(bus, hl_dc);
    cpu.iy = cpu.pop(bus);
    let hl_da = bus.read_word(1, 0x85DA);
    cpu.set_hl(hl_da);
    cpu.push(bus, hl_da);
    cpu.ix = cpu.pop(bus);
    cpu.set_hl(bus.read_word(1, 0x85D6));
    cpu.set_de(bus.read_word(1, 0x85D8));
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.and8(0x0F);

    if (cpu.f & FLAG_Z) != 0 {
        // JP Z, $154D
        cpu.a = bus.read_byte(1, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1551);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.hl(), 0x01);
        }
        cpu.a = bus.read_byte(1, cpu.de());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x155B);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.a = 0x00;
            bus.write_byte(1, cpu.de(), 0x00);
        }
        return;
    }

    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // JP Z, $1562
        cpu.a = bus.read_byte(1, cpu.de());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1566);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.a = 0x0A;
            bus.write_byte(1, cpu.de(), 0x0A);
        }
        cpu.a = bus.read_byte(1, cpu.iy);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1573);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.iy, 0x0B);
        }
        return;
    }

    cpu.sub8(0x04, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // JP Z, $1519
        cpu.a = bus.read_byte(1, cpu.ix);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x151F);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.ix, 0x05);
        }
        cpu.a = bus.read_byte(1, cpu.iy);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x152D);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.iy, 0x04);
        }
        return;
    }

    cpu.sub8(0x06, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // JP Z, $1535
        cpu.a = bus.read_byte(1, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1539);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.hl(), 0x06);
        }
        cpu.a = bus.read_byte(1, cpu.ix);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1545);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.ix, 0x07);
        }
    }
}

/// CPU 1 `$10C7`: `advance_digdug_anim_and_move` (`$10C7..$1135`)
pub fn decomp_cpu1_advance_digdug_anim_and_move<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x1000, 0x10CA);
    cpu.xor8(cpu.a);
    bus.write_byte(1, 0x98A0, 0x00);
    bus.write_byte(1, 0x98A1, 0x00);
    cpu.a = bus.read_byte(1, 0x98A3);
    cpu.sub8(0x08, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = bus.read_byte(1, 0x85AE);
        cpu.and8(0x0F);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x02;
            bus.write_byte(1, 0x85AE, 0x02);
            call_cpu_sub(cpu, bus, roms, 0x104E, 0x10C6);
            return;
        }
    }
    call_cpu_sub(cpu, bus, roms, 0x111F, 0x10DB);
    call_cpu_sub(cpu, bus, roms, 0x118F, 0x10DE);
    call_cpu_sub(cpu, bus, roms, 0x11EA, 0x10E1);
    cpu.set_hl(0x85AE);
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.and8(0x0F);
    cpu.sub8(0x08, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x104E, 0x1135);
        return;
    }
    let v_ae = bus.read_byte(1, 0x85AE);
    cpu.a = v_ae;
    cpu.cb_op(0x76, v_ae);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x104E, 0x1135);
        return;
    }
    cpu.a = bus.read_byte(1, 0x85B3);
    cpu.set_hl(0x85B1);
    cpu.sub8(bus.read_byte(1, 0x85B1), false, true);
    bus.write_byte(1, 0x85B3, cpu.a);
    if (cpu.f & FLAG_C) == 0 {
        return;
    }
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.cb_op(0x7F, cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x104E, 0x1105);
        return;
    }
    call_cpu_sub(cpu, bus, roms, 0x104E, 0x1109);
    cpu.set_hl(0x85B2);
    let b2 = cpu.inc8(bus.read_byte(1, 0x85B2));
    bus.write_byte(1, 0x85B2, b2);
    cpu.cb_op(0x46, b2);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.set_hl(0x9822);
        let s = cpu.inc8(bus.read_byte(1, 0x9822));
        bus.write_byte(1, 0x9822, s);
    }
    cpu.a = bus.read_byte(1, 0x8401);
    cpu.cb_op(0x57, cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        return;
    }
    call_cpu_sub(cpu, bus, roms, 0x1136, 0x111E);
}

/// CPU 1 `$130E`: `update_digging_and_tunnels` (`$130E..$1352`)
pub fn decomp_cpu1_update_digging_and_tunnels<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let hl_d6 = bus.read_word(1, 0x85D6);
    cpu.set_hl(hl_d6);
    bus.write_word(1, 0x85DE, hl_d6);
    cpu.a = bus.read_byte(1, 0x98A2);
    cpu.d = cpu.a;
    cpu.a = bus.read_byte(1, 0x98A3);
    cpu.e = cpu.a;
    call_cpu_sub(cpu, bus, roms, 0x1393, 0x131F);
    call_cpu_sub(cpu, bus, roms, 0x1353, 0x1322);
    let new_d6 = bus.read_word(1, 0x85D6);
    cpu.set_hl(new_d6);
    cpu.a = bus.read_byte(1, 0x85DE);
    cpu.sub8(cpu.l, false, false);
    let mut tile_changed = (cpu.f & FLAG_Z) == 0;
    if !tile_changed {
        cpu.a = bus.read_byte(1, 0x85DF);
        cpu.sub8(cpu.h, false, false);
        tile_changed = (cpu.f & FLAG_Z) == 0;
    }
    if tile_changed {
        cpu.a = bus.read_byte(1, 0x8646);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            return;
        }
        call_cpu_sub(cpu, bus, roms, 0x140A, 0x133A);
        call_cpu_sub(cpu, bus, roms, 0x140A, 0x133D);
    }
    cpu.a = bus.read_byte(1, 0x8646);
    cpu.cb_op(0x47, cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        return;
    }
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.and8(0x07);
    cpu.b = cpu.a;
    cpu.a = bus.read_byte(1, 0x85AF);
    cpu.and8(0x07);
    cpu.sub8(cpu.b, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x14EF, 0x1352);
    }
}

/// CPU 1 `$140A`: `carve_tunnel_front_tiles` (`$140A..$14EE`)
pub fn decomp_cpu1_carve_tunnel_front_tiles<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let hl_d6 = bus.read_word(1, 0x85D6);
    cpu.set_hl(hl_d6);
    cpu.push(bus, hl_d6);
    cpu.ix = cpu.pop(bus);
    cpu.set_bc(bus.read_word(1, 0x85D8));
    cpu.set_hl(bus.read_word(1, 0x85DA));
    cpu.set_de(bus.read_word(1, 0x85DC));
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.and8(0x0F);

    let is_vert = if (cpu.f & FLAG_Z) != 0 {
        true
    } else {
        cpu.sub8(0x04, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            let ix = cpu.ix;
            cpu.push(bus, ix);
            let popped_hl = cpu.pop(bus);
            cpu.set_hl(popped_hl);
            let bc = cpu.bc();
            cpu.push(bus, bc);
            let popped_de = cpu.pop(bus);
            cpu.set_de(popped_de);
            true
        } else {
            cpu.sub8(0x02, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                let ix = cpu.ix;
                cpu.push(bus, ix);
                let popped_bc = cpu.pop(bus);
                cpu.set_bc(popped_bc);
                let hl = cpu.hl();
                cpu.push(bus, hl);
                let popped_de = cpu.pop(bus);
                cpu.set_de(popped_de);
                false
            } else {
                cpu.sub8(0x06, false, false);
                if (cpu.f & FLAG_Z) != 0 {
                    false
                } else {
                    return;
                }
            }
        }
    };

    if is_vert {
        // $1432..$1484
        cpu.a = bus.read_byte(1, cpu.hl());
        cpu.sub8(0x08, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, cpu.hl(), 0x0C);
            return;
        }
        cpu.sub8(0x09, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, cpu.hl(), 0x0C);
            return;
        }
        cpu.sub8(0x0C, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.sub8(0x8D, false, false);
            let skip_hl = if (cpu.f & FLAG_C) != 0 {
                false
            } else {
                cpu.sub8(0x90, false, false);
                (cpu.f & FLAG_C) != 0
            };
            if !skip_hl {
                cpu.sub8(0x8C, false, false);
                if (cpu.f & FLAG_Z) == 0 {
                    cpu.sub8(0x7F, false, false);
                    if (cpu.f & FLAG_Z) != 0 {
                        call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1452);
                    }
                    cpu.sub8(0x7E, false, false);
                    if (cpu.f & FLAG_Z) != 0 {
                        bus.write_byte(1, cpu.hl(), 0x8C);
                    } else {
                        bus.write_byte(1, cpu.hl(), 0x03);
                    }
                }
            }
        }

        // $145C: second tile (DE)
        cpu.a = bus.read_byte(1, cpu.de());
        cpu.sub8(0x08, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.de(), 0x0C);
            return;
        }
        cpu.sub8(0x09, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.de(), 0x0C);
            return;
        }
        cpu.sub8(0x0C, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            return;
        }
        cpu.sub8(0x8D, false, false);
        if (cpu.f & FLAG_C) == 0 {
            cpu.sub8(0x90, false, false);
            if (cpu.f & FLAG_C) != 0 {
                return;
            }
        }
        cpu.sub8(0x8C, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            return;
        }
        cpu.sub8(0x7F, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1479);
        }
        cpu.sub8(0x7E, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x8C;
            bus.write_byte(1, cpu.de(), 0x8C);
        } else {
            cpu.a = 0x02;
            bus.write_byte(1, cpu.de(), 0x02);
        }
    } else {
        // $1485..$14D3 (Horizontal: BC and DE)
        cpu.a = bus.read_byte(1, cpu.bc());
        cpu.sub8(0x02, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.bc(), 0x0C);
            return;
        }
        cpu.sub8(0x03, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.bc(), 0x0C);
            return;
        }
        cpu.sub8(0x7F, false, false);
        let mut write_bc_08 = true;
        if (cpu.f & FLAG_Z) == 0 {
            cpu.sub8(0x0C, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                write_bc_08 = false;
            } else {
                cpu.sub8(0x8C, false, false);
                if (cpu.f & FLAG_Z) != 0 {
                    write_bc_08 = false;
                } else {
                    cpu.sub8(0x7E, false, false);
                    if (cpu.f & FLAG_Z) != 0 {
                        write_bc_08 = false;
                    } else {
                        cpu.sub8(0x8D, false, false);
                        if (cpu.f & FLAG_C) == 0 {
                            cpu.sub8(0x90, false, false);
                            if (cpu.f & FLAG_C) != 0 {
                                write_bc_08 = false;
                            }
                        }
                        if write_bc_08 {
                            cpu.sub8(0x7F, false, false);
                        }
                    }
                }
            }
        }
        if write_bc_08 {
            if (cpu.f & FLAG_Z) != 0 {
                call_cpu_sub(cpu, bus, roms, 0x15CB, 0x14AB);
            }
            cpu.a = 0x08;
            bus.write_byte(1, cpu.bc(), 0x08);
        }

        // $14AE: second tile (DE)
        cpu.a = bus.read_byte(1, cpu.de());
        cpu.sub8(0x02, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.de(), 0x0C);
            return;
        }
        cpu.sub8(0x03, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.a = 0x0C;
            bus.write_byte(1, cpu.de(), 0x0C);
            return;
        }
        cpu.sub8(0x7F, false, false);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.sub8(0x0C, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                return;
            }
            cpu.sub8(0x8C, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                return;
            }
            cpu.sub8(0x7E, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                return;
            }
            cpu.sub8(0x8D, false, false);
            if (cpu.f & FLAG_C) == 0 {
                cpu.sub8(0x90, false, false);
                if (cpu.f & FLAG_C) != 0 {
                    return;
                }
            }
            cpu.sub8(0x7F, false, false);
        }
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x15CB, 0x14D0);
        }
        cpu.a = 0x09;
        bus.write_byte(1, cpu.de(), 0x09);
    }
}

/// CPU 1 `$14EF`: `carve_tunnel_back_tiles` (`$14EF..$157A`)
pub fn decomp_cpu1_carve_tunnel_back_tiles<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x15D3, 0x14F2);
    let hl_dc = bus.read_word(1, 0x85DC);
    cpu.set_hl(hl_dc);
    cpu.push(bus, hl_dc);
    cpu.iy = cpu.pop(bus);
    let hl_da = bus.read_word(1, 0x85DA);
    cpu.set_hl(hl_da);
    cpu.push(bus, hl_da);
    cpu.ix = cpu.pop(bus);
    cpu.set_hl(bus.read_word(1, 0x85D6));
    cpu.set_de(bus.read_word(1, 0x85D8));
    cpu.a = bus.read_byte(1, 0x85AE);
    cpu.and8(0x0F);

    if (cpu.f & FLAG_Z) != 0 {
        // $1519: dir == 0 (Up)
        cpu.a = bus.read_byte(1, cpu.ix);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x151F);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.ix, 0x05);
        }
        cpu.a = bus.read_byte(1, cpu.iy);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x152D);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.iy, 0x04);
        }
        return;
    }

    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // $1535: dir == 2 (Right)
        cpu.a = bus.read_byte(1, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1539);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.hl(), 0x06);
        }
        cpu.a = bus.read_byte(1, cpu.ix);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1545);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.ix, 0x07);
        }
        return;
    }

    cpu.sub8(0x04, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // $154D: dir == 4 (Down)
        cpu.a = bus.read_byte(1, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1551);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.hl(), 0x01);
        }
        cpu.a = bus.read_byte(1, cpu.de());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x155B);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.a = 0x00;
            bus.write_byte(1, cpu.de(), 0x00);
        }
        return;
    }

    cpu.sub8(0x06, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        // $1562: dir == 6 (Left)
        cpu.a = bus.read_byte(1, cpu.de());
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1566);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.a = 0x0A;
            bus.write_byte(1, cpu.de(), 0x0A);
        }
        cpu.a = bus.read_byte(1, cpu.iy);
        call_cpu_sub(cpu, bus, roms, 0x157B, 0x1573);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            bus.write_byte(1, cpu.iy, 0x0B);
        }
    }
}

/// CPU 1 `$1DDC`: `service_earom_highscore_io` (`$1DDC..$1F56`)
pub fn decomp_cpu1_service_earom_highscore_io<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = bus.read_byte(1, 0x89CF);
    cpu.and8(cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x9B3D, 0x00);
        return;
    }
    cpu.a = bus.read_byte(1, 0x9B3D);
    cpu.sub8(0x05, false, false);
    if (cpu.f & FLAG_C) == 0 {
        return;
    }
    cpu.set_hl(0x1DF6);
    cpu.push(bus, 0x1DEC);
    let a0 = cpu.a;
    cpu.add8(a0, false);
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
    let l = cpu.l;
    cpu.add8(l, false);
    cpu.l = cpu.a;
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
    let _ = cpu.pop(bus);
    let lo = bus.read_byte(1, cpu.hl());
    cpu.set_hl(cpu.hl().wrapping_add(1));
    let hi = bus.read_byte(1, cpu.hl());
    cpu.h = hi;
    cpu.l = lo;
    cpu.a = lo;

    match cpu.hl() {
        0x1E00 => {}
        0x1E3A => {
            cpu.set_hl(0xB800);
            cpu.set_de(0x89A0);
            cpu.b = 0x25;
            while cpu.b != 0 {
                bus.write_byte(1, cpu.hl(), 0x00);
                bus.write_byte(1, 0xB840, 0x08);
                bus.write_byte(1, 0xB840, 0x09);
                bus.write_byte(1, 0xB840, 0x08);
                bus.write_byte(1, 0xB840, 0x0C);
                bus.write_byte(1, 0xB840, 0x0D);
                cpu.c = 0x01;
                cpu.ldi(bus);
                cpu.a = 0x04;
                bus.write_byte(1, 0xB840, 0x04);
                cpu.b = cpu.b.wrapping_sub(1);
            }
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9B3D, 0x00);
        }
        0x1F34 => {
            cpu.push(bus, 0x1F37);
            let _ = cpu.pop(bus);
            decomp_earom_block_write_1e77(cpu, bus);
            cpu.set_de(0x89A0);
            cpu.set_hl(0xB800);
            cpu.b = 0x25;
            cpu.push(bus, 0x1F42);
            let _ = cpu.pop(bus);
            decomp_earom_verify_block_1eb3(cpu, bus);
            cpu.a = bus.read_byte(1, 0x89CF);
            cpu.and8(cpu.a);
            if (cpu.f & FLAG_Z) == 0 {
                cpu.set_hl(0x1F57);
                cpu.set_de(0xB830);
                cpu.b = 0x04;
                cpu.push(bus, 0x1F53);
                let _ = cpu.pop(bus);
                decomp_earom_write_loop_1e7f(cpu, bus);
            }
            bus.write_byte(1, 0x9B3D, cpu.a);
        }
        0x1E6F => {
            cpu.push(bus, 0x1E72);
            let _ = cpu.pop(bus);
            decomp_earom_block_write_1e77(cpu, bus);
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9B3D, 0x00);
        }
        0x1E01 => {
            cpu.push(bus, 0x1E04);
            let _ = cpu.pop(bus);
            cpu.set_hl(0xB830);
            cpu.set_de(0x1F03);
            cpu.b = 0x04;
            decomp_earom_verify_block_1eb3(cpu, bus);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.xor8(cpu.a);
                bus.write_byte(1, 0x9B3D, 0x00);
                return;
            }
            cpu.set_hl(0x1EDE);
            cpu.push(bus, 0x1E11);
            let _ = cpu.pop(bus);
            cpu.set_de(0xB800);
            cpu.b = 0x25;
            decomp_earom_write_loop_1e7f(cpu, bus);
            cpu.set_de(0xB830);
            cpu.b = 0x04;
            cpu.push(bus, 0x1E19);
            let _ = cpu.pop(bus);
            decomp_earom_write_loop_1e7f(cpu, bus);
            cpu.set_de(0x1EDE);
            cpu.set_hl(0xB800);
            cpu.b = 0x25;
            cpu.push(bus, 0x1E24);
            let _ = cpu.pop(bus);
            decomp_earom_verify_block_1eb3(cpu, bus);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.push(bus, 0x1E27);
                let _ = cpu.pop(bus);
                cpu.set_hl(0xB830);
                cpu.set_de(0x1F03);
                cpu.b = 0x04;
                decomp_earom_verify_block_1eb3(cpu, bus);
            }
            bus.write_byte(1, 0x89CF, cpu.a);
            cpu.set_hl(0x1EDE);
            cpu.set_de(0x89A0);
            cpu.set_bc(0x0025);
            while cpu.bc() != 0 {
                cpu.ldi(bus);
            }
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9B3D, 0x00);
        }
        _ => {}
    }
}

fn decomp_earom_block_write_1e77<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x89A0);
    cpu.set_de(0xB800);
    cpu.b = 0x25;
    decomp_earom_write_loop_1e7f(cpu, bus);
}

fn decomp_earom_write_loop_1e7f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    while cpu.b != 0 {
        cpu.c = 0x01;
        cpu.ldi(bus);
        bus.write_byte(1, 0xB840, 0x0E);
        decomp_earom_delay_1ea0(cpu, bus);
        bus.write_byte(1, 0xB840, 0x04);
        bus.write_byte(1, 0xB840, 0x0A);
        decomp_earom_delay_1ea0(cpu, bus);
        cpu.a = 0x04;
        bus.write_byte(1, 0xB840, 0x04);
        cpu.b = cpu.b.wrapping_sub(1);
    }
}

fn decomp_earom_delay_1ea0<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let saved_bc = cpu.bc();
    cpu.push(bus, saved_bc);
    cpu.set_bc(0x24DB);
    loop {
        cpu.c = cpu.dec8(cpu.c);
        if (cpu.f & FLAG_Z) == 0 {
            continue;
        }
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    let restored_bc = cpu.pop(bus);
    cpu.set_bc(restored_bc);
}

fn decomp_earom_verify_block_1eb3<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        bus.write_byte(1, 0xB840, 0x08);
        bus.write_byte(1, 0xB840, 0x09);
        bus.write_byte(1, 0xB840, 0x08);
        bus.write_byte(1, 0xB840, 0x0C);
        bus.write_byte(1, 0xB840, 0x0D);
        cpu.a = bus.read_byte(1, cpu.de());
        cpu.xor8(bus.read_byte(1, cpu.hl()));
        if (cpu.f & FLAG_Z) == 0 {
            return;
        }
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_de(cpu.de().wrapping_add(1));
        cpu.a = 0x04;
        bus.write_byte(1, 0xB840, 0x04);
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.xor8(cpu.a);
}

/// CPU 1 `$049B`: `attract_cutscene_dispatcher` (`$049B..$0E2D` and `$1700..$1950`)
pub fn decomp_cpu1_attract_cutscene_dispatcher<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x8787);
    bus.write_byte(1, 0x8787, 0xFF);
    cpu.set_hl(0x8657);
    let v8657 = bus.read_byte(1, 0x8657);
    cpu.cb_op(0x46, v8657);
    if (v8657 & 0x01) == 0 {
        decomp_cpu1_attract_exit_0517(cpu, bus);
        return;
    }

    let v8657_set1 = cpu.cb_op(0xCE, v8657);
    bus.write_byte(1, 0x8657, v8657_set1);
    cpu.cb_op(0x76, v8657_set1);
    if (v8657_set1 & 0x40) == 0 {
        let v8657_set6 = cpu.cb_op(0xF6, v8657_set1);
        bus.write_byte(1, 0x8657, v8657_set6);
        cpu.set_hl(0x8658);
        bus.write_byte(1, 0x8658, 0x00);
        decomp_cpu1_attract_exit_0517(cpu, bus);
        return;
    }

    cpu.set_hl(0x8658);
    let v8658 = bus.read_byte(1, 0x8658);
    cpu.a = v8658;
    cpu.and8(0x0F);
    cpu.b = cpu.a;
    cpu.cb_op(0x66, v8658);

    if (v8658 & 0x10) == 0 {
        // State Initialization ($04C3..$04EC)
        cpu.a = cpu.b;
        cpu.and8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state0(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state1(cpu, bus);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state2(cpu, bus);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state3(cpu, bus);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        cpu.a = cpu.dec8(cpu.a);
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state6(cpu, bus);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state7(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state8(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_init_state9(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            let v = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
            bus.write_byte(1, cpu.hl(), v);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        bus.write_byte(1, cpu.hl(), 0x00);
        decomp_cpu1_attract_exit_0517(cpu, bus);
    } else {
        // State Update ($04EE..$0516)
        cpu.a = cpu.b;
        cpu.and8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_update_state0(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_update_state1(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_update_state2(cpu, bus);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_attract_update_state3_1700(cpu, bus, roms);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        cpu.a = cpu.dec8(cpu.a);
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            // $0BBF: State 6 update
            cpu.set_hl(0x8658);
            bus.write_byte(1, 0x8658, 0x07);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            // $0BC7: State 7 update
            cpu.a = bus.read_byte(1, 0x8676);
            cpu.sub8(0x80, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                // JP $015F (inside VBlank handler!)
                decomp_cpu1_vblank_from_015f(cpu, bus, roms);
                return;
            }
            cpu.sub8(0x90, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.set_hl(0x8658);
                bus.write_byte(1, 0x8658, 0x08);
                decomp_cpu1_attract_init_state6_from_075f(cpu, bus);
            }
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            // $0BDC: State 8 update
            cpu.set_hl(0x865E);
            let v = cpu.dec8(bus.read_byte(1, 0x865E));
            bus.write_byte(1, 0x865E, v);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.set_hl(0x8658);
                bus.write_byte(1, 0x8658, 0x09);
            }
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            // $0BEB: State 9 update
            cpu.set_hl(0x8658);
            bus.write_byte(1, 0x8658, 0x00);
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        cpu.a = cpu.dec8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            // $0BF3: State 10 update
            decomp_cpu1_attract_exit_0517(cpu, bus);
            return;
        }
        bus.write_byte(1, cpu.hl(), 0x00);
        decomp_cpu1_attract_exit_0517(cpu, bus);
    }
}

fn decomp_cpu1_attract_exit_0517<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x8657);
    let v = bus.read_byte(1, 0x8657);
    cpu.cb_op(0x46, v);
    if (v & 0x01) == 0 {
        bus.write_byte(1, 0x8657, 0x00);
    }
    cpu.a = 0x01;
    bus.write_byte(1, 0x6821, 0x01);
}

fn decomp_cpu1_attract_init_state0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);

    cpu.set_hl(0x9880);
    cpu.b = 0x80;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0xA000);
    bus.write_byte(1, 0xA000, 0x01);
    cpu.set_hl(0xA001);
    bus.write_byte(1, 0xA001, 0x01);
    cpu.set_hl(0xA003);
    bus.write_byte(1, 0xA003, 0x00);

    cpu.set_hl(0x8010);
    cpu.set_de(0x8030);
    cpu.b = 0x10;
    while cpu.b != 0 {
        cpu.a = 0x7F;
        bus.write_byte(1, cpu.hl(), 0x7F);
        bus.write_byte(1, cpu.de(), 0x7F);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_de(cpu.de().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_de(0x801B);
    cpu.set_hl(0x059C);
    cpu.b = 0x06;
    while cpu.b != 0 {
        let v = bus.read_byte(1, cpu.hl());
        cpu.a = v;
        bus.write_byte(1, cpu.de(), v);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_de(cpu.de().wrapping_sub(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x8013);
    bus.write_byte(1, 0x8013, 0x10);

    cpu.set_hl(0x8040);
    cpu.set_bc(0x0380);
    while cpu.bc() != 0 {
        bus.write_byte(1, cpu.hl(), 0x7F);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_bc(cpu.bc().wrapping_sub(1));
        bus.write_byte(1, 0x6830, cpu.a);
        cpu.a = cpu.c;
        cpu.or8(cpu.b);
    }

    cpu.set_hl(0x8668);
    cpu.b = 0x07;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0x8659);
    bus.write_byte(1, 0x8659, 0x01);

    cpu.set_hl(0x9840);
    cpu.b = 0x0A;
    cpu.c = 0x86;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), cpu.c);
        cpu.c = cpu.inc8(cpu.c);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        bus.write_byte(1, cpu.hl(), 0x0B);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0x9940);
    cpu.b = 0x14;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0x05A2);
    cpu.set_de(0x98C0);
    cpu.set_bc(0x0014);
    while cpu.bc() != 0 {
        cpu.ldi(bus);
    }

    cpu.a = 0xF1;
    bus.write_byte(1, 0x9880, 0xF1);
    bus.write_byte(1, 0x98A2, 0xF1);
    bus.write_byte(1, 0x98E4, 0xF1);
    bus.write_byte(1, 0x98EC, 0xF1);
    cpu.set_hl(0x98D4);
    call_cpu_sub(cpu, bus, roms, 0x069A, 0x05D5);

    cpu.a = 0x10;
    bus.write_byte(1, 0x9881, 0x10);
    bus.write_byte(1, 0x98A3, 0x10);
    bus.write_byte(1, 0x98E5, 0x10);
    bus.write_byte(1, 0x98ED, 0x10);
    cpu.set_hl(0x98D5);
    call_cpu_sub(cpu, bus, roms, 0x069A, 0x05E9);

    cpu.a = 0x02;
    bus.write_byte(1, 0x9822, 0x02);
    cpu.set_hl(0x9922);
    call_cpu_sub(cpu, bus, roms, 0x069A, 0x05F4);
    bus.write_byte(1, 0x9964, cpu.a);
    bus.write_byte(1, 0x996C, cpu.a);
    bus.write_byte(1, 0x986D, cpu.a);

    cpu.xor8(cpu.a);
    bus.write_byte(1, 0x9823, 0x00);
    cpu.set_hl(0x9954);
    call_cpu_sub(cpu, bus, roms, 0x069A, 0x0607);

    cpu.a = 0x20;
    bus.write_byte(1, 0x9864, 0x20);
    cpu.a = 0x01;
    bus.write_byte(1, 0x9865, 0x01);
    cpu.a = 0x24;
    bus.write_byte(1, 0x986C, 0x24);
    cpu.a = 0x05;
    bus.write_byte(1, 0x9825, 0x05);
    bus.write_byte(1, 0x9827, 0x05);
    bus.write_byte(1, 0x9829, 0x05);

    cpu.a = 0x09;
    cpu.set_hl(0x9855);
    call_cpu_sub(cpu, bus, roms, 0x069A, 0x0629);

    cpu.set_hl(0x06DD);
    cpu.set_de(0x988A);
    cpu.set_bc(0x0018);
    cpu.push(bus, 0x0018);
    while cpu.bc() != 0 {
        cpu.ldi(bus);
    }
    let popped_bc = cpu.pop(bus);
    cpu.set_bc(popped_bc);

    cpu.set_hl(0x06F5);
    cpu.set_de(0x980A);
    while cpu.bc() != 0 {
        cpu.ldi(bus);
    }

    cpu.set_hl(0x990A);
    cpu.b = 0x0C;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(2));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0x9854);
    cpu.b = 0x04;
    cpu.a = 0x48;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), cpu.a);
        cpu.a = cpu.inc8(cpu.a);
        cpu.set_hl(cpu.hl().wrapping_add(2));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.set_hl(0x0BF6);
    bus.write_word(1, 0x865A, 0x0BF6);
    cpu.set_hl(0x0C48);
    bus.write_word(1, 0x865C, 0x0C48);
    cpu.set_hl(0x0C2C);
    bus.write_word(1, 0x865E, 0x0C2C);
    cpu.set_hl(0x0C04);
    bus.write_word(1, 0x8660, 0x0C04);
    cpu.set_hl(0x0C0E);
    bus.write_word(1, 0x8662, 0x0C0E);
    cpu.set_hl(0x0C18);
    bus.write_word(1, 0x8664, 0x0C18);
    cpu.set_hl(0x0C22);
    bus.write_word(1, 0x8666, 0x0C22);

    cpu.set_hl(0x83C0);
    cpu.b = 0x3F;
    while cpu.b != 0 {
        cpu.a = bus.read_byte(1, cpu.hl());
        cpu.sub8(0x8C, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, cpu.hl(), 0x0C);
        }
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.a = 0x00;
    bus.write_byte(1, 0xA004, 0x00);
    bus.write_byte(1, 0xA005, 0x00);
}

fn decomp_cpu1_attract_init_state1<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0xA003);
    bus.write_byte(1, 0xA003, 0x01);
    cpu.set_hl(0x8040);
    cpu.set_bc(0x0380);
    while cpu.bc() != 0 {
        bus.write_byte(1, cpu.hl(), 0x7D);
        cpu.a = cpu.l;
        cpu.and8(0x1F);
        cpu.sub8(0x04, false, false);
        if (cpu.f & FLAG_C) != 0 {
            bus.write_byte(1, cpu.hl(), 0x8C);
        }
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_bc(cpu.bc().wrapping_sub(1));
        cpu.a = cpu.c;
        cpu.or8(cpu.b);
    }
    cpu.set_hl(0x8084);
    cpu.set_de(0x0020);
    cpu.c = 0x08;
    loop {
        cpu.b = 0x18;
        let saved_hl = cpu.hl();
        cpu.push(bus, saved_hl);
        while cpu.b != 0 {
            bus.write_byte(1, cpu.hl(), 0x7F);
            let next_hl = cpu.add16(cpu.hl(), 0x0020);
            cpu.set_hl(next_hl);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        let popped_hl = cpu.pop(bus).wrapping_add(1);
        cpu.set_hl(popped_hl);
        cpu.c = cpu.dec8(cpu.c);
        if (cpu.f & FLAG_Z) != 0 {
            break;
        }
    }
    cpu.set_hl(0x8676);
    bus.write_byte(1, 0x8676, 0x00);
}

fn decomp_cpu1_attract_init_state2<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0x8676);
    bus.write_byte(1, 0x8676, 0x00);
    cpu.set_hl(0x83C0);
    cpu.b = 0x3F;
    while cpu.b != 0 {
        cpu.a = bus.read_byte(1, cpu.hl());
        cpu.sub8(0x0C, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, cpu.hl(), 0x8C);
        }
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x805F);
    bus.write_word(1, 0x865A, 0x805F);
    cpu.set_hl(0x9888);
    cpu.b = 0x38;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x9840);
    cpu.b = 0x20;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x32);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x9808);
    cpu.b = 0x1A;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x32);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
}

fn decomp_cpu1_attract_init_state3<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0x98A4);
    cpu.b = 0x06;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x0000);
    bus.write_word(1, 0x865A, 0x0000);
}

fn decomp_cpu1_attract_init_state6<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    decomp_cpu1_attract_init_state6_from_075f(cpu, bus);
}

fn decomp_cpu1_attract_init_state6_from_075f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x9880);
    cpu.b = 0x80;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x8040);
    cpu.set_bc(0x0380);
    while cpu.bc() != 0 {
        bus.write_byte(1, cpu.hl(), 0x7F);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.set_bc(cpu.bc().wrapping_sub(1));
        bus.write_byte(1, 0x6830, cpu.a);
        cpu.a = cpu.c;
        cpu.or8(cpu.b);
    }
    cpu.set_hl(0x87F0);
    let v = cpu.cb_op(0xDE, bus.read_byte(1, 0x87F0));
    bus.write_byte(1, 0x87F0, v);
}

fn decomp_cpu1_attract_init_state7<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0x0487);
    bus.write_word(1, 0x865A, 0x0487);
    cpu.a = 0x0A;
    bus.write_byte(1, 0x865C, 0x0A);
    cpu.set_hl(0x0000);
    bus.write_word(1, 0x865E, 0x0000);
    cpu.set_hl(0x8785);
    let v85 = cpu.cb_op(0xBE, bus.read_byte(1, 0x8785));
    bus.write_byte(1, 0x8785, v85);
    call_cpu_sub(cpu, bus, roms, 0x0351, 0x07A1);
    cpu.set_hl(0x0C30);
    cpu.set_de(0x813F);
    cpu.b = 0x08;
    call_cpu_sub(cpu, bus, roms, 0x0E3B, 0x07AC);
    cpu.set_hl(0x0C38);
    cpu.set_de(0x815F);
    cpu.b = 0x06;
    call_cpu_sub(cpu, bus, roms, 0x0E3B, 0x07B7);
    cpu.set_hl(0x8676);
    bus.write_byte(1, 0x8676, 0x10);
}

fn decomp_cpu1_attract_init_state8<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0x9880);
    cpu.b = 0x80;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x0C40);
    cpu.set_de(0x811E);
    cpu.b = 0x08;
    call_cpu_sub(cpu, bus, roms, 0x0E3B, 0x07D6);
    cpu.set_hl(0x865E);
    bus.write_byte(1, 0x865E, 0xFF);
}

fn decomp_cpu1_attract_init_state9<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    _roms: &DigDugRoms,
) {
    let v8658 = cpu.cb_op(0xE6, bus.read_byte(1, cpu.hl()));
    bus.write_byte(1, cpu.hl(), v8658);
    cpu.set_hl(0x805E);
    cpu.set_de(0x0020);
    cpu.b = 0x18;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x7F);
        let next_hl = cpu.add16(cpu.hl(), 0x0020);
        cpu.set_hl(next_hl);
        cpu.b = cpu.b.wrapping_sub(1);
    }
    cpu.set_hl(0x87CC);
    let v = cpu.cb_op(0xC6, bus.read_byte(1, 0x87CC));
    bus.write_byte(1, 0x87CC, v);
}

fn decomp_cpu1_attract_update_state0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x0C4E);
    cpu.a = bus.read_byte(1, 0x8659);
    cpu.sub8(bus.read_byte(1, 0x0C4E), false, true);
    bus.write_byte(1, 0x8659, cpu.a);
    if (cpu.f & FLAG_C) == 0 {
        return;
    }

    // Channel 0 ($8668 / $865A / $866F)
    cpu.a = bus.read_byte(1, 0x8668);
    cpu.and8(cpu.a);
    if (cpu.f & FLAG_Z) != 0 {
        let mut ptr = bus.read_word(1, 0x865A);
        let cmd = bus.read_byte(1, ptr);
        bus.write_byte(1, 0x866F, cmd);
        ptr = ptr.wrapping_add(1);
        let cnt = bus.read_byte(1, ptr);
        ptr = ptr.wrapping_add(1);
        bus.write_word(1, 0x865A, ptr);
        cpu.set_hl(ptr);
        cpu.a = cnt;
        cpu.sub8(0x09, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, 0x8668, cpu.a);
            cpu.a = 0x8C;
            bus.write_byte(1, 0x8082, 0x8C);
            bus.write_byte(1, 0x80A2, 0x8C);
            bus.write_byte(1, 0x8083, 0x8C);
            bus.write_byte(1, 0x80A3, 0x8C);
        } else {
            bus.write_byte(1, 0x8668, cpu.a);
        }
    }
    cpu.a = cpu.dec8(bus.read_byte(1, 0x8668));
    bus.write_byte(1, 0x8668, cpu.a);
    decomp_attract_step_digdug_0c4f(cpu, bus);

    cpu.a = bus.read_byte(1, 0x98A2);
    bus.write_byte(1, 0x9880, cpu.a);
    cpu.a = bus.read_byte(1, 0x98A3);
    bus.write_byte(1, 0x9881, cpu.a);
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x9880);
        bus.write_byte(1, 0x9880, 0x00);
        cpu.set_hl(0x9881);
        bus.write_byte(1, 0x9881, 0x00);
    }
    cpu.a = 0x38;
    bus.write_byte(1, 0x9800, 0x38);
    cpu.a = 0x06;
    bus.write_byte(1, 0x9801, 0x06);
    cpu.a = bus.read_byte(1, 0x98A3);
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x130E, 0x08F2);
    }

    // Channels 1..5 ($8669..$866E)
    decomp_attract_step_channel(cpu, bus, 0x8669, 0x865C, 0x8670, 0x0CEE);
    decomp_attract_step_channel(cpu, bus, 0x866A, 0x865E, 0x8671, 0x0D4D);
    decomp_attract_step_channel(cpu, bus, 0x866B, 0x8660, 0x8672, 0x0CFB);
    decomp_attract_step_channel(cpu, bus, 0x866C, 0x8662, 0x8673, 0x0D08);
    decomp_attract_step_channel(cpu, bus, 0x866D, 0x8664, 0x8674, 0x0D15);
    decomp_attract_step_channel(cpu, bus, 0x866E, 0x8666, 0x8675, 0x0D22);
}

fn decomp_attract_step_channel<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    cnt_addr: u16,
    ptr_addr: u16,
    cmd_addr: u16,
    handler: u16,
) {
    cpu.a = bus.read_byte(1, cnt_addr);
    cpu.and8(cpu.a);
    if (cpu.f & FLAG_Z) != 0 {
        let mut ptr = bus.read_word(1, ptr_addr);
        let cmd = bus.read_byte(1, ptr);
        bus.write_byte(1, cmd_addr, cmd);
        ptr = ptr.wrapping_add(1);
        let cnt = bus.read_byte(1, ptr);
        ptr = ptr.wrapping_add(1);
        bus.write_word(1, ptr_addr, ptr);
        cpu.set_hl(ptr);
        cpu.a = cnt;
        bus.write_byte(1, cnt_addr, cnt);
    }
    cpu.a = cpu.dec8(bus.read_byte(1, cnt_addr));
    bus.write_byte(1, cnt_addr, cpu.a);
    match handler {
        0x0CEE => {
            cpu.a = bus.read_byte(1, 0x8670);
            cpu.set_hl(0x98E4);
            cpu.set_de(0x9864);
            decomp_attract_pooka_0d2f(cpu, bus);
        }
        0x0D4D => decomp_attract_fygar_0d4d(cpu, bus),
        0x0CFB => {
            cpu.a = bus.read_byte(1, 0x8672);
            cpu.set_hl(0x98D4);
            cpu.set_de(0x9854);
            decomp_attract_move_up_0d47(cpu, bus);
        }
        0x0D08 => {
            cpu.a = bus.read_byte(1, 0x8673);
            cpu.set_hl(0x98D6);
            cpu.set_de(0x9856);
            decomp_attract_move_up_0d47(cpu, bus);
        }
        0x0D15 => {
            cpu.a = bus.read_byte(1, 0x8674);
            cpu.set_hl(0x98D8);
            cpu.set_de(0x9858);
            decomp_attract_move_up_0d47(cpu, bus);
        }
        0x0D22 => {
            cpu.a = bus.read_byte(1, 0x8675);
            cpu.set_hl(0x98DA);
            cpu.set_de(0x985A);
            decomp_attract_move_up_0d47(cpu, bus);
        }
        _ => {}
    }
}

fn decomp_attract_pooka_0d2f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let saved_af = cpu.af();
    cpu.push(bus, saved_af);
    bus.write_byte(1, cpu.de(), 0x21);
    cpu.set_bc(0x8677);
    let v = cpu.inc8(bus.read_byte(1, 0x8677));
    bus.write_byte(1, 0x8677, v);
    cpu.a = v;
    cpu.and8(0x07);
    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_C) != 0 {
        let s = cpu.dec8(bus.read_byte(1, cpu.de()));
        bus.write_byte(1, cpu.de(), s);
    }
    let restored_af = cpu.pop(bus);
    cpu.set_af(restored_af);
    decomp_attract_move_up_0d47(cpu, bus);
}

fn decomp_attract_move_up_0d47<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.and8(0xF0);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    let v1 = cpu.dec8(bus.read_byte(1, cpu.hl()));
    let v2 = cpu.dec8(v1);
    bus.write_byte(1, cpu.hl(), v2);
}

fn decomp_attract_fygar_0d4d<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x986C);
    bus.write_byte(1, 0x986C, 0x24);
    cpu.set_de(0x8678);
    let v = cpu.inc8(bus.read_byte(1, 0x8678));
    bus.write_byte(1, 0x8678, v);
    cpu.a = v;
    cpu.cb_op(0x57, v);
    if (cpu.f & FLAG_Z) != 0 {
        let s = cpu.inc8(bus.read_byte(1, 0x986C));
        bus.write_byte(1, 0x986C, s);
    }
    cpu.a = bus.read_byte(1, 0x8671);
    cpu.and8(0xF0);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x98EC);
        let v1 = cpu.dec8(bus.read_byte(1, 0x98EC));
        let v2 = cpu.dec8(v1);
        bus.write_byte(1, 0x98EC, v2);
        return;
    }
    cpu.sub8(0x30, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = bus.read_byte(1, 0x8671);
        cpu.set_hl(0x986D);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, 0x986D, 0x02);
        } else {
            bus.write_byte(1, 0x986D, 0x0C);
        }
        return;
    }
    cpu.ix = 0x98A4;
    let mut x = bus.read_byte(1, 0x98EC);
    for offs in [0u16, 2, 4] {
        cpu.a = x;
        cpu.sub8(0x10, false, true);
        x = cpu.a;
        bus.write_byte(1, cpu.ix.wrapping_add(offs), x);
    }
    let y = bus.read_byte(1, 0x98ED);
    cpu.a = y;
    for offs in [1u16, 3, 5] {
        bus.write_byte(1, cpu.ix.wrapping_add(offs), y);
    }
    cpu.ix = 0x9824;
    cpu.a = bus.read_byte(1, 0x8671);
    cpu.and8(0x0F);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = 0x32;
        bus.write_byte(1, cpu.ix, 0x32);
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x32);
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x32);
        cpu.ix = 0x98A4;
        cpu.b = 0x06;
        while cpu.b != 0 {
            bus.write_byte(1, cpu.ix, 0x00);
            cpu.ix = cpu.ix.wrapping_add(1);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.sub8(0x01, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, cpu.ix, 0x2C);
        cpu.a = 0x32;
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x32);
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x32);
        return;
    }
    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, cpu.ix, 0x2D);
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x2E);
        cpu.a = 0x32;
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x32);
        return;
    }
    bus.write_byte(1, cpu.ix, 0x2F);
    bus.write_byte(1, cpu.ix.wrapping_add(2), 0x30);
    cpu.a = 0x31;
    bus.write_byte(1, cpu.ix.wrapping_add(4), 0x31);
}

fn decomp_attract_step_digdug_0c4f<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = bus.read_byte(1, 0x866F);
    cpu.and8(0xF0);
    cpu.sub8(0x20, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x8658);
        bus.write_byte(1, 0x8658, 0x01);
        return;
    }
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        return;
    }
    let prev = bus.read_byte(1, 0x85AE);
    bus.write_byte(1, 0x85AF, prev);
    cpu.set_hl(0x9822);
    cpu.set_de(0x9922);
    cpu.a = bus.read_byte(1, 0x866F);
    cpu.and8(0x03);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x9822, 0x00);
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x9922, 0x00);
        cpu.set_hl(0x98A3);
        let y0 = cpu.dec8(bus.read_byte(1, 0x98A3));
        let y = cpu.dec8(y0);
        bus.write_byte(1, 0x98A3, y);
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x85AE, 0x00);
        cpu.a = 0x04;
        bus.write_byte(1, 0x808B, 0x04);
    } else {
        cpu.sub8(0x01, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            bus.write_byte(1, 0x9822, 0x02);
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x9922, 0x00);
            cpu.set_hl(0x98A2);
            let x0 = cpu.inc8(bus.read_byte(1, 0x98A2));
            let x = cpu.inc8(x0);
            bus.write_byte(1, 0x98A2, x);
            bus.write_byte(1, 0x85AE, 0x02);
            cpu.a = 0x05;
            bus.write_byte(1, 0x836B, 0x05);
        } else {
            cpu.sub8(0x02, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                bus.write_byte(1, 0x9822, 0x00);
                bus.write_byte(1, 0x9922, 0x01);
                cpu.set_hl(0x98A3);
                let y0 = cpu.inc8(bus.read_byte(1, 0x98A3));
                let y = cpu.inc8(y0);
                bus.write_byte(1, 0x98A3, y);
                bus.write_byte(1, 0x85AE, 0x04);
                cpu.a = 0x8C;
                bus.write_byte(1, 0x8363, 0x8C);
                bus.write_byte(1, 0x8343, 0x8C);
                bus.write_byte(1, 0x8362, 0x8C);
                bus.write_byte(1, 0x8342, 0x8C);
            } else {
                bus.write_byte(1, 0x9822, 0x02);
                bus.write_byte(1, 0x9922, 0x02);
                cpu.set_hl(0x98A2);
                let x0 = cpu.dec8(bus.read_byte(1, 0x98A2));
                let x = cpu.dec8(x0);
                bus.write_byte(1, 0x98A2, x);
                cpu.a = 0x06;
                bus.write_byte(1, 0x85AE, 0x06);
            }
        }
    }
    cpu.a = bus.read_byte(1, 0x98A3);
    cpu.sub8(0x10, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x9822);
        let mut s = bus.read_byte(1, 0x9822);
        for _ in 0..4 {
            s = cpu.inc8(s);
        }
        bus.write_byte(1, 0x9822, s);
    }
    cpu.set_hl(0x8676);
    let t = cpu.inc8(bus.read_byte(1, 0x8676));
    bus.write_byte(1, 0x8676, t);
    cpu.cb_op(0x46, t);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x9822);
        let s = cpu.inc8(bus.read_byte(1, 0x9822));
        bus.write_byte(1, 0x9822, s);
    }
}

fn decomp_cpu1_attract_update_state1<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x8676);
    let t = cpu.inc8(bus.read_byte(1, 0x8676));
    bus.write_byte(1, 0x8676, t);
    cpu.a = t;
    cpu.sub8(0x05, false, false);
    if (cpu.f & FLAG_C) != 0 {
        let saved_af = cpu.af();
        cpu.push(bus, saved_af);
        cpu.set_hl(0x98C1);
        cpu.b = 0x0A;
        while cpu.b != 0 {
            let mut v = bus.read_byte(1, cpu.hl());
            for _ in 0..4 {
                v = cpu.inc8(v);
            }
            bus.write_byte(1, cpu.hl(), v);
            cpu.set_hl(cpu.hl().wrapping_add(2));
            cpu.b = cpu.b.wrapping_sub(1);
        }
        let restored = cpu.pop(bus);
        cpu.set_af(restored);
    }
    cpu.sub8(0x05, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.sub8(0x09, false, false);
        if (cpu.f & FLAG_C) != 0 {
            let saved_af = cpu.af();
            cpu.push(bus, saved_af);
            cpu.set_hl(0x98E5);
            cpu.set_de(0x98ED);
            let mut v1 = bus.read_byte(1, 0x98E5);
            for _ in 0..4 {
                v1 = cpu.inc8(v1);
            }
            bus.write_byte(1, 0x98E5, v1);
            std::mem::swap(&mut cpu.d, &mut cpu.h);
            std::mem::swap(&mut cpu.e, &mut cpu.l);
            let mut v2 = bus.read_byte(1, 0x98ED);
            for _ in 0..4 {
                v2 = cpu.inc8(v2);
            }
            bus.write_byte(1, 0x98ED, v2);
            let restored = cpu.pop(bus);
            cpu.set_af(restored);
        }
    }
    cpu.sub8(0x09, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x9864);
        bus.write_byte(1, 0x9864, 0x23);
        cpu.set_hl(0x986C);
        bus.write_byte(1, 0x986C, 0x27);
    }
    cpu.sub8(0x05, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.sub8(0x37, false, false);
        if (cpu.f & FLAG_C) != 0 {
            let saved_af = cpu.af();
            cpu.push(bus, saved_af);
            cpu.set_hl(0x98D5);
            cpu.b = 0x04;
            while cpu.b != 0 {
                let mut v = bus.read_byte(1, cpu.hl());
                for _ in 0..4 {
                    v = cpu.inc8(v);
                }
                bus.write_byte(1, cpu.hl(), v);
                cpu.set_hl(cpu.hl().wrapping_add(2));
                cpu.b = cpu.b.wrapping_sub(1);
            }
            let restored = cpu.pop(bus);
            cpu.set_af(restored);
        }
    }
    cpu.sub8(0x72, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x8658);
        bus.write_byte(1, 0x8658, 0x02);
    }
    // $0DFB..$0E2D
    cpu.sub8(0x06, false, false);
    if (cpu.f & FLAG_C) != 0 {
        return;
    }
    cpu.sub8(0x17, false, false);
    if (cpu.f & FLAG_C) != 0 {
        cpu.a = bus.read_byte(1, 0x8676);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.c = 0xFF;
            call_cpu_sub(cpu, bus, roms, 0x0E2E, 0x0E11);
        }
        return;
    }
    cpu.sub8(0x1A, false, false);
    if (cpu.f & FLAG_C) != 0 {
        return;
    }
    cpu.sub8(0x29, false, false);
    if (cpu.f & FLAG_C) != 0 {
        cpu.a = bus.read_byte(1, 0x8676);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            cpu.c = 0x01;
            call_cpu_sub(cpu, bus, roms, 0x0E2E, 0x0E2B);
        }
    }
}

fn decomp_cpu1_attract_update_state2<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
) {
    cpu.set_hl(0x8676);
    let t = cpu.inc8(bus.read_byte(1, 0x8676));
    bus.write_byte(1, 0x8676, t);
    cpu.cb_op(0x46, t);
    if (cpu.f & FLAG_Z) != 0 {
        return;
    }
    let ptr = bus.read_word(1, 0x865A);
    cpu.set_hl(ptr);
    cpu.set_de(0x803F);
    cpu.and8(cpu.a);
    cpu.sbc_hl(0x803F);
    if (cpu.f & FLAG_Z) == 0 {
        let mut col_hl = bus.read_word(1, 0x865A);
        cpu.set_de(0x87F0);
        let v = cpu.cb_op(0xDF, bus.read_byte(1, 0x87F0));
        cpu.a = v;
        bus.write_byte(1, 0x87F0, v);
        cpu.set_de(0x0020);
        cpu.b = 0x1C;
        while cpu.b != 0 {
            bus.write_byte(1, col_hl, 0x7F);
            col_hl = cpu.add16(col_hl, 0x0020);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        let next_col = bus.read_word(1, 0x865A).wrapping_sub(1);
        cpu.set_hl(next_col);
        bus.write_word(1, 0x865A, next_col);
    }
    cpu.set_hl(0x8676);
    cpu.a = bus.read_byte(1, 0x8676);
    cpu.sub8(0x07, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.xor8(cpu.a);
        cpu.set_hl(0x98D4);
        for _ in 0..8 {
            bus.write_byte(1, cpu.hl(), 0x00);
            cpu.set_hl(cpu.hl().wrapping_add(1));
        }
        cpu.set_hl(0x93D4);
        for _ in 0..8 {
            bus.write_byte(1, cpu.hl(), 0x00);
            cpu.set_hl(cpu.hl().wrapping_add(1));
        }
        cpu.b = 0;
        cpu.set_hl(0x0A72);
        cpu.set_de(0x819C);
        cpu.b = 0x07;
        // CALL $0E3B
        std::mem::swap(&mut cpu.d, &mut cpu.h);
        std::mem::swap(&mut cpu.e, &mut cpu.l);
        cpu.ix = cpu.de();
        cpu.set_de(0x0020);
        while cpu.b != 0 {
            let ch = bus.read_byte(1, cpu.ix);
            cpu.a = ch;
            cpu.ix = cpu.ix.wrapping_add(1);
            bus.write_byte(1, cpu.hl(), ch);
            let next_hl = cpu.add16(cpu.hl(), 0x0020);
            cpu.set_hl(next_hl);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.sub8(0x0B, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x0A8B);
        cpu.set_de(0x80FA);
        cpu.b = 0x11;
        std::mem::swap(&mut cpu.d, &mut cpu.h);
        std::mem::swap(&mut cpu.e, &mut cpu.l);
        cpu.ix = cpu.de();
        cpu.set_de(0x0020);
        while cpu.b != 0 {
            let ch = bus.read_byte(1, cpu.ix);
            cpu.a = ch;
            cpu.ix = cpu.ix.wrapping_add(1);
            bus.write_byte(1, cpu.hl(), ch);
            let next_hl = cpu.add16(cpu.hl(), 0x0020);
            cpu.set_hl(next_hl);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.sub8(0x15, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x0AAE);
        cpu.set_de(0x80D5);
        cpu.b = 0x12;
        std::mem::swap(&mut cpu.d, &mut cpu.h);
        std::mem::swap(&mut cpu.e, &mut cpu.l);
        cpu.ix = cpu.de();
        cpu.set_de(0x0020);
        while cpu.b != 0 {
            let ch = bus.read_byte(1, cpu.ix);
            cpu.a = ch;
            cpu.ix = cpu.ix.wrapping_add(1);
            bus.write_byte(1, cpu.hl(), ch);
            let next_hl = cpu.add16(cpu.hl(), 0x0020);
            cpu.set_hl(next_hl);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.sub8(0x23, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x0AD2);
        cpu.set_de(0x816E);
        cpu.b = 0x09;
        std::mem::swap(&mut cpu.d, &mut cpu.h);
        std::mem::swap(&mut cpu.e, &mut cpu.l);
        cpu.ix = cpu.de();
        cpu.set_de(0x0020);
        while cpu.b != 0 {
            let ch = bus.read_byte(1, cpu.ix);
            cpu.a = ch;
            cpu.ix = cpu.ix.wrapping_add(1);
            bus.write_byte(1, cpu.hl(), ch);
            let next_hl = cpu.add16(cpu.hl(), 0x0020);
            cpu.set_hl(next_hl);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.sub8(0x61, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x87F0);
        let v = cpu.cb_op(0x9E, bus.read_byte(1, 0x87F0));
        bus.write_byte(1, 0x87F0, v);
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x9964, 0x00);
        cpu.a = cpu.inc8(cpu.a);
        bus.write_byte(1, 0x9865, cpu.a);
        cpu.a = cpu.inc8(cpu.a);
        bus.write_byte(1, 0x986D, cpu.a);
        bus.write_byte(1, 0x996C, cpu.a);
        cpu.a = 0x90;
        bus.write_byte(1, 0x98E5, 0x90);
        bus.write_byte(1, 0x98ED, 0x90);
        cpu.a = 0xB2;
        bus.write_byte(1, 0x98EC, 0xB2);
        cpu.a = 0x52;
        bus.write_byte(1, 0x98E4, 0x52);
        cpu.a = 0x20;
        bus.write_byte(1, 0x9864, 0x20);
        cpu.a = 0x24;
        bus.write_byte(1, 0x986C, 0x24);
        return;
    }
    cpu.set_hl(0x986D);
    cpu.sub8(0x77, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x0C);
        return;
    }
    cpu.sub8(0x7F, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x02);
        return;
    }
    cpu.sub8(0x87, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x0C);
        return;
    }
    cpu.sub8(0x8F, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x02);
        return;
    }
    cpu.sub8(0xB3, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x02);
        cpu.ix = 0x98A4;
        cpu.b = 0x06;
        while cpu.b != 0 {
            bus.write_byte(1, cpu.ix, 0x00);
            cpu.ix = cpu.ix.wrapping_add(1);
            cpu.b = cpu.b.wrapping_sub(1);
        }
        return;
    }
    cpu.ix = 0x9824;
    cpu.iy = 0x98A4;
    cpu.set_de(0x98EC);
    cpu.sub8(0x97, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, 0x986D, 0x0C);
        let mut x = bus.read_byte(1, 0x98EC);
        for offs in [0u16, 2, 4] {
            cpu.a = x;
            cpu.sub8(0x10, false, true);
            x = cpu.a;
            bus.write_byte(1, cpu.iy.wrapping_add(offs), x);
        }
        cpu.set_de(0x98ED);
        let y = bus.read_byte(1, 0x98ED);
        cpu.a = y;
        for offs in [1u16, 3, 5] {
            bus.write_byte(1, cpu.iy.wrapping_add(offs), y);
        }
        bus.write_byte(1, cpu.ix, 0x2C);
        cpu.a = 0x32;
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x32);
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x32);
        return;
    }
    cpu.sub8(0x9D, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, cpu.ix, 0x2D);
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x2E);
        cpu.a = 0x32;
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x32);
        return;
    }
    cpu.sub8(0xA3, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        bus.write_byte(1, cpu.ix, 0x2F);
        bus.write_byte(1, cpu.ix.wrapping_add(2), 0x30);
        cpu.a = 0x31;
        bus.write_byte(1, cpu.ix.wrapping_add(4), 0x31);
        return;
    }
    cpu.sub8(0xFF, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x8658);
        bus.write_byte(1, 0x8658, 0x03);
    }
}

fn decomp_cpu1_attract_update_state3_1700<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let hl = bus.read_word(1, 0x865A).wrapping_add(1);
    cpu.set_hl(hl);
    bus.write_word(1, 0x865A, hl);
    cpu.a = cpu.h;
    cpu.sub8(0x03, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = cpu.l;
        cpu.sub8(0x1A, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.set_hl(0x8658);
            bus.write_byte(1, 0x8658, 0x06);
        }
        return;
    }
    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = cpu.l;
        cpu.sub8(0xDE, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_attract_clear_98f4_1881(cpu, bus);
            return;
        }
        cpu.sub8(0x32, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.set_hl(0x986C);
            bus.write_byte(1, 0x986C, 0x32);
            cpu.set_hl(0x98EC);
            bus.write_byte(1, 0x98EC, 0x00);
            cpu.set_hl(0x98ED);
            bus.write_byte(1, 0x98ED, 0x00);
            cpu.set_hl(0x98F4);
            cpu.b = 0x04;
            cpu.c = 0x90;
            cpu.a = 0x97;
            while cpu.b != 0 {
                bus.write_byte(1, cpu.hl(), cpu.a);
                cpu.add8(0x10, false);
                cpu.set_hl(cpu.hl().wrapping_add(1));
                bus.write_byte(1, cpu.hl(), cpu.c);
                cpu.set_hl(cpu.hl().wrapping_add(1));
                cpu.b = cpu.b.wrapping_sub(1);
            }
            bus.write_byte(1, cpu.hl(), 0xB1);
            cpu.set_hl(cpu.hl().wrapping_add(1));
            bus.write_byte(1, cpu.hl(), cpu.c);
            cpu.set_hl(0x1951);
            cpu.set_de(0x9874);
            cpu.set_bc(0x000A);
            while cpu.bc() != 0 {
                cpu.ldi(bus);
            }
            decomp_attract_reset_digdug_188e(cpu, bus);
            return;
        }
        cpu.sub8(0x22, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x195B, 0x191A);
            cpu.set_hl(0x986C);
            let v = cpu.inc8(bus.read_byte(1, 0x986C));
            bus.write_byte(1, 0x986C, v);
            decomp_attract_clear_9882_183a(cpu, bus);
            return;
        }
        cpu.sub8(0x12, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x1961, 0x1830);
            return;
        }
        cpu.sub8(0x02, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x195B, 0x18FE);
            cpu.set_hl(0x986C);
            bus.write_byte(1, 0x986C, 0x84);
            cpu.set_hl(0x98ED);
            cpu.a = bus.read_byte(1, 0x98ED);
            cpu.sub8(0x03, false, true);
            bus.write_byte(1, 0x98ED, cpu.a);
        }
        return;
    }
    cpu.sub8(0x01, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = cpu.l;
        cpu.sub8(0xF2, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x1961, 0x1830);
            return;
        }
        cpu.sub8(0xE2, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x195B, 0x190C);
            cpu.set_hl(0x986C);
            bus.write_byte(1, 0x986C, 0x83);
            cpu.set_hl(0x98EC);
            cpu.a = bus.read_byte(1, 0x98EC);
            cpu.sub8(0x08, false, true);
            bus.write_byte(1, 0x98EC, cpu.a);
            cpu.set_hl(0x98ED);
            cpu.a = bus.read_byte(1, 0x98ED);
            cpu.sub8(0x04, false, true);
            bus.write_byte(1, 0x98ED, cpu.a);
            return;
        }
        cpu.sub8(0xBE, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x1961, 0x18F3);
            cpu.set_hl(0x986C);
            bus.write_byte(1, 0x986C, 0x27);
            return;
        }
        if (cpu.f & FLAG_C) == 0 {
            return;
        }
        cpu.sub8(0xBA, false, false);
        if (cpu.f & FLAG_C) == 0 {
            decomp_attract_inc_harpoon_x(cpu, bus, 0x9882);
            return;
        }
        cpu.sub8(0xB2, false, false);
        if (cpu.f & FLAG_C) == 0 {
            decomp_attract_inc_harpoon_x(cpu, bus, 0x9884);
            return;
        }
        cpu.sub8(0xAA, false, false);
        if (cpu.f & FLAG_C) == 0 {
            decomp_attract_inc_harpoon_x(cpu, bus, 0x9886);
            return;
        }
        cpu.sub8(0xA9, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.b = 0x00;
            decomp_attract_init_harpoon_18a5(cpu, bus);
            return;
        }
        cpu.sub8(0x6D, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_attract_clear_98f4_1881(cpu, bus);
        }
        return;
    }

    // $176E: H == 0
    cpu.a = cpu.l;
    cpu.sub8(0xC2, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x98F4);
        cpu.b = 0x04;
        cpu.c = 0x90;
        cpu.a = 0x37;
        while cpu.b != 0 {
            bus.write_byte(1, cpu.hl(), cpu.a);
            cpu.add8(0x10, false);
            cpu.set_hl(cpu.hl().wrapping_add(1));
            bus.write_byte(1, cpu.hl(), cpu.c);
            cpu.set_hl(cpu.hl().wrapping_add(1));
            cpu.b = cpu.b.wrapping_sub(1);
        }
        bus.write_byte(1, cpu.hl(), 0x51);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        bus.write_byte(1, cpu.hl(), cpu.c);
        cpu.set_hl(0x9864);
        bus.write_byte(1, 0x9864, 0x32);
        cpu.set_hl(0x98E4);
        bus.write_byte(1, 0x98E4, 0x00);
        cpu.set_hl(0x98E5);
        bus.write_byte(1, 0x98E5, 0x00);
        cpu.set_hl(0x1877);
        cpu.set_de(0x9874);
        cpu.set_bc(0x000A);
        while cpu.bc() != 0 {
            cpu.ldi(bus);
        }
        decomp_attract_reset_digdug_188e(cpu, bus);
        return;
    }
    cpu.sub8(0xB2, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x195B, 0x1836);
        cpu.set_hl(0x9864);
        let v = cpu.inc8(bus.read_byte(1, 0x9864));
        bus.write_byte(1, 0x9864, v);
        decomp_attract_clear_9882_183a(cpu, bus);
        return;
    }
    cpu.sub8(0xA2, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x1961, 0x1830);
        return;
    }
    cpu.sub8(0x92, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x195B, 0x1807);
        cpu.set_hl(0x9864);
        bus.write_byte(1, 0x9864, 0x81);
        cpu.set_hl(0x98E5);
        cpu.a = bus.read_byte(1, 0x98E5);
        cpu.sub8(0x03, false, true);
        bus.write_byte(1, 0x98E5, cpu.a);
        return;
    }
    cpu.sub8(0x82, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x1961, 0x1830);
        return;
    }
    cpu.sub8(0x72, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x195B, 0x1819);
        cpu.set_hl(0x9864);
        bus.write_byte(1, 0x9864, 0x80);
        cpu.set_hl(0x98E4);
        cpu.a = bus.read_byte(1, 0x98E4);
        cpu.sub8(0x08, false, true);
        bus.write_byte(1, 0x98E4, cpu.a);
        cpu.set_hl(0x98E5);
        cpu.a = bus.read_byte(1, 0x98E5);
        cpu.sub8(0x04, false, true);
        bus.write_byte(1, 0x98E5, cpu.a);
        return;
    }
    cpu.sub8(0x52, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x1961, 0x17FC);
        cpu.set_hl(0x9864);
        bus.write_byte(1, 0x9864, 0x23);
        return;
    }
    if (cpu.f & FLAG_C) == 0 {
        return;
    }
    cpu.sub8(0x4F, false, false);
    if (cpu.f & FLAG_C) == 0 {
        decomp_attract_dec_harpoon_x(cpu, bus, 0x9882);
        return;
    }
    cpu.sub8(0x47, false, false);
    if (cpu.f & FLAG_C) == 0 {
        decomp_attract_dec_harpoon_x(cpu, bus, 0x9884);
        return;
    }
    cpu.sub8(0x3F, false, false);
    if (cpu.f & FLAG_C) == 0 {
        decomp_attract_dec_harpoon_x(cpu, bus, 0x9886);
        return;
    }
    cpu.sub8(0x3E, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.b = 0x02;
        decomp_attract_init_harpoon_18a5(cpu, bus);
        return;
    }
    cpu.sub8(0x02, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x98A2);
        bus.write_byte(1, 0x98A2, 0x82);
        cpu.set_hl(0x98A3);
        bus.write_byte(1, 0x98A3, 0x90);
        cpu.set_hl(0x9822);
        bus.write_byte(1, 0x9822, 0x06);
        cpu.set_hl(0x9823);
        bus.write_byte(1, 0x9823, 0x00);
        cpu.set_hl(0x9922);
        bus.write_byte(1, 0x9922, 0x02);
        cpu.set_hl(0x9888);
        bus.write_byte(1, 0x9888, 0x82);
        cpu.set_hl(0x9889);
        bus.write_byte(1, 0x9889, 0x90);
        cpu.set_hl(0x9808);
        bus.write_byte(1, 0x9808, 0x13);
        cpu.set_hl(0x9809);
        bus.write_byte(1, 0x9809, 0x06);
    }
}

fn decomp_attract_dec_harpoon_x<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, start_hl: u16) {
    let mut addr = start_hl;
    while addr <= 0x9886 {
        cpu.set_hl(addr);
        let v0 = cpu.dec8(bus.read_byte(1, addr));
        let v = cpu.dec8(v0);
        bus.write_byte(1, addr, v);
        addr = addr.wrapping_add(2);
    }
}

fn decomp_attract_inc_harpoon_x<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, start_hl: u16) {
    let mut addr = start_hl;
    while addr <= 0x9886 {
        cpu.set_hl(addr);
        let v0 = cpu.inc8(bus.read_byte(1, addr));
        let v = cpu.inc8(v0);
        bus.write_byte(1, addr, v);
        addr = addr.wrapping_add(2);
    }
}

fn decomp_attract_clear_9882_183a<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x9882);
    cpu.b = 0x06;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
}

fn decomp_attract_clear_98f4_1881<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x98F4);
    cpu.b = 0x0A;
    while cpu.b != 0 {
        bus.write_byte(1, cpu.hl(), 0x00);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
    }
}

fn decomp_attract_reset_digdug_188e<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x9822);
    bus.write_byte(1, 0x9822, 0x06);
    cpu.set_hl(0x9922);
    bus.write_byte(1, 0x9922, 0x00);
    cpu.set_hl(0x9920);
    bus.write_byte(1, 0x9920, 0x00);
}

fn decomp_attract_init_harpoon_18a5<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.set_hl(0x9822);
    bus.write_byte(1, 0x9822, 0x11);
    cpu.ix = 0x9882;
    cpu.iy = 0x9802;
    cpu.a = 0x82;
    cpu.c = 0x90;
    bus.write_byte(1, cpu.ix, 0x82);
    bus.write_byte(1, cpu.ix.wrapping_add(2), 0x82);
    bus.write_byte(1, cpu.ix.wrapping_add(4), 0x82);
    bus.write_byte(1, cpu.ix.wrapping_add(1), 0x90);
    bus.write_byte(1, cpu.ix.wrapping_add(3), 0x90);
    bus.write_byte(1, cpu.ix.wrapping_add(5), 0x90);
    bus.write_byte(1, cpu.iy, 0x37);
    bus.write_byte(1, cpu.iy.wrapping_add(2), 0x37);
    bus.write_byte(1, cpu.iy.wrapping_add(4), 0x36);
    bus.write_byte(1, cpu.iy.wrapping_add(1), 0x08);
    bus.write_byte(1, cpu.iy.wrapping_add(3), 0x08);
    bus.write_byte(1, cpu.iy.wrapping_add(5), 0x08);
    cpu.ix = 0x9902;
    bus.write_byte(1, cpu.ix, cpu.b);
    bus.write_byte(1, cpu.ix.wrapping_add(2), cpu.b);
    bus.write_byte(1, cpu.ix.wrapping_add(4), cpu.b);
}

/// CPU 1 `$0080`: `cpu1_vblank_handler` (`$0080..$02E8`)
pub fn decomp_cpu1_vblank_handler<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.xor8(cpu.a);
    bus.write_byte(1, 0x6821, 0x00);
    bus.write_byte(1, 0xA002, 0x00);

    cpu.a = bus.read_byte(1, 0x879A);
    cpu.and8(cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        decomp_cpu1_vblank_exit_02df(cpu, bus);
        return;
    }

    call_cpu_sub(cpu, bus, roms, 0x0E4C, 0x0091);
    call_cpu_sub(cpu, bus, roms, 0x1B9F, 0x0094);

    cpu.a = bus.read_byte(1, 0x87CF);
    cpu.cb_op(0x6F, cpu.a);
    if (cpu.f & FLAG_Z) != 0 {
        decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
        return;
    }

    cpu.a = bus.read_byte(1, 0x898C);
    cpu.and8(cpu.a);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = 0x05;
    }
    bus.write_byte(1, 0x8409, cpu.a);

    cpu.set_hl(0x89E5);
    cpu.set_de(0x89EF);
    cpu.b = 0x04;
    while cpu.b != 0 {
        let saved_bc = cpu.bc();
        cpu.push(bus, saved_bc);
        call_cpu_sub(cpu, bus, roms, 0x046F, 0x00B3);
        let restored_bc = cpu.pop(bus);
        cpu.set_bc(restored_bc);
        cpu.b = cpu.b.wrapping_sub(1);
    }

    cpu.a = bus.read_byte(1, 0x89E2);
    cpu.and8(0x0F);
    bus.write_byte(1, 0x89E2, cpu.a);

    cpu.a = bus.read_byte(1, 0x89F3);
    cpu.sub8(0x3C, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x89F3, 0x00);
        cpu.set_hl(0x89E8);
        cpu.b = 0x03;
        cpu.f = (cpu.f & (FLAG_S | FLAG_Z | FLAG_PV)) | FLAG_C;
        while cpu.b != 0 {
            cpu.a = bus.read_byte(1, cpu.hl());
            cpu.add8(0x00, true);
            cpu.daa();
            bus.write_byte(1, cpu.hl(), cpu.a);
            cpu.set_hl(cpu.hl().wrapping_sub(1));
            cpu.b = cpu.b.wrapping_sub(1);
        }
    }

    cpu.set_hl(0x87CC);
    let v87cc = bus.read_byte(1, 0x87CC);
    cpu.cb_op(0x4E, v87cc);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.cb_op(0x56, v87cc);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
            return;
        }
    }

    cpu.a = bus.read_byte(1, 0x8657);
    cpu.and8(0x03);
    if (cpu.f & FLAG_Z) == 0 {
        decomp_cpu1_attract_cutscene_dispatcher(cpu, bus, roms);
        return;
    }

    cpu.set_hl(0x8785);
    cpu.set_de(0x8400);
    cpu.a = bus.read_byte(1, 0x8400);
    cpu.cb_op(0x4F, cpu.a);

    enum ModeBranch {
        InitDemo0117,
        Set7_0138,
        Res7_013C,
    }

    let branch = if (cpu.f & FLAG_Z) != 0 {
        // $00F7: BIT 2, (HL)
        let v8785 = bus.read_byte(1, 0x8785);
        cpu.cb_op(0x56, v8785);
        if (cpu.f & FLAG_Z) == 0 {
            // $012E: BIT 0, (HL)
            cpu.cb_op(0x46, v8785);
            if (cpu.f & FLAG_Z) == 0 {
                ModeBranch::Res7_013C
            } else {
                ModeBranch::Set7_0138
            }
        } else {
            // $010D
            cpu.a = bus.read_byte(1, 0x9A81);
            cpu.sub8(0x00, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
                return;
            }
            let v_set = cpu.cb_op(0xD6, bus.read_byte(1, 0x8785));
            bus.write_byte(1, 0x8785, v_set);
            ModeBranch::InitDemo0117
        }
    } else {
        // $00FD: BIT 3, (HL)
        let v8785 = bus.read_byte(1, 0x8785);
        cpu.cb_op(0x5E, v8785);
        if (cpu.f & FLAG_Z) == 0 {
            // $0134: BIT 1, (HL)
            cpu.cb_op(0x4E, v8785);
            if (cpu.f & FLAG_Z) == 0 {
                ModeBranch::Res7_013C
            } else {
                ModeBranch::Set7_0138
            }
        } else {
            // $0101
            cpu.a = bus.read_byte(1, 0x9A81);
            cpu.sub8(0x00, false, false);
            if (cpu.f & FLAG_Z) != 0 {
                decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
                return;
            }
            let v_set = cpu.cb_op(0xDE, bus.read_byte(1, 0x8785));
            bus.write_byte(1, 0x8785, v_set);
            ModeBranch::InitDemo0117
        }
    };

    match branch {
        ModeBranch::InitDemo0117 => {
            cpu.set_hl(0x0487);
            bus.write_word(1, 0x865A, 0x0487);
            cpu.a = 0x0A;
            bus.write_byte(1, 0x865C, 0x0A);
            cpu.set_hl(0x0000);
            bus.write_word(1, 0x865E, 0x0000);
            call_cpu_sub(cpu, bus, roms, 0x0351, 0x012B);
            decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
            return;
        }
        ModeBranch::Set7_0138 => {
            let v = cpu.cb_op(0xFE, bus.read_byte(1, 0x8785));
            bus.write_byte(1, 0x8785, v);
        }
        ModeBranch::Res7_013C => {
            let v = cpu.cb_op(0xBE, bus.read_byte(1, 0x8785));
            bus.write_byte(1, 0x8785, v);
            cpu.a = bus.read_byte(1, 0x85D5);
            cpu.and8(cpu.a);
            if (cpu.f & FLAG_Z) == 0 {
                decomp_cpu1_vblank_exit_02e5(cpu, bus, roms);
                return;
            }
            cpu.set_hl(0x8400);
            let v8400 = bus.read_byte(1, 0x8400);
            cpu.cb_op(0x6E, v8400);
            if (cpu.f & FLAG_Z) != 0 {
                decomp_cpu1_vblank_exit_02e5(cpu, bus, roms);
                return;
            }
            cpu.set_hl(0x8401);
            let v8401 = bus.read_byte(1, 0x8401);
            cpu.cb_op(0x7E, v8401);
            if (cpu.f & FLAG_Z) == 0 {
                decomp_cpu1_vblank_exit_02e5(cpu, bus, roms);
                return;
            }
            cpu.cb_op(0x6E, v8401);
            if (cpu.f & FLAG_Z) != 0 {
                call_cpu_sub(cpu, bus, roms, 0x0351, 0x015C);
                decomp_cpu1_vblank_exit_02e5(cpu, bus, roms);
                return;
            }
        }
    }

    decomp_cpu1_vblank_from_015f(cpu, bus, roms);
}

fn decomp_cpu1_vblank_from_015f<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    // $015F
    call_cpu_sub(cpu, bus, roms, 0x1251, 0x0162);
    cpu.a = bus.read_byte(1, 0x8401);
    cpu.cb_op(0x7F, cpu.a);
    if (cpu.f & FLAG_Z) == 0 {
        decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
        return;
    }

    cpu.a = bus.read_byte(1, 0x8785);
    cpu.cb_op(0x7F, cpu.a);
    let mut run_demo_input = (cpu.f & FLAG_Z) == 0;
    if !run_demo_input {
        cpu.a = bus.read_byte(1, 0x896F);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
            return;
        }
        cpu.a = bus.read_byte(1, 0x8657);
        cpu.cb_op(0x4F, cpu.a);
        run_demo_input = (cpu.f & FLAG_Z) == 0;
    }

    if run_demo_input {
        // $0180
        cpu.a = bus.read_byte(1, 0x865C);
        cpu.and8(cpu.a);
        if (cpu.f & FLAG_Z) != 0 {
            decomp_cpu1_vblank_exit_02df(cpu, bus);
            return;
        }
        let hl_5e = bus.read_word(1, 0x865E);
        cpu.set_hl(hl_5e);
        cpu.a = cpu.l;
        cpu.or8(cpu.h);
        if (cpu.f & FLAG_Z) != 0 {
            let mut ptr = bus.read_word(1, 0x865A);
            let cmd = bus.read_byte(1, ptr);
            cpu.a = cmd;
            bus.write_byte(1, 0x8677, cmd);
            ptr = ptr.wrapping_add(1);
            let d = bus.read_byte(1, ptr);
            cpu.d = d;
            ptr = ptr.wrapping_add(1);
            let e = bus.read_byte(1, ptr);
            cpu.e = e;
            let de = cpu.de();
            bus.write_word(1, 0x865E, de);
            ptr = ptr.wrapping_add(1);
            cpu.set_hl(ptr);
            bus.write_word(1, 0x865A, ptr);
            cpu.a = cpu.dec8(bus.read_byte(1, 0x865C));
            bus.write_byte(1, 0x865C, cpu.a);
            cpu.a = cpu.e;
            cpu.or8(cpu.d);
            if (cpu.f & FLAG_Z) != 0 {
                cpu.xor8(cpu.a);
                bus.write_byte(1, 0x865C, 0x00);
                decomp_cpu1_vblank_exit_02df(cpu, bus);
                return;
            }
        }
        let dec_5e = bus.read_word(1, 0x865E).wrapping_sub(1);
        cpu.set_hl(dec_5e);
        bus.write_word(1, 0x865E, dec_5e);
        cpu.a = bus.read_byte(1, 0x8677);
        bus.write_byte(1, 0x8970, cpu.a);
    }

    // $01C0
    cpu.a = bus.read_byte(1, 0x8401);
    cpu.cb_op(0x5F, cpu.a);
    let saved_af = cpu.af();
    cpu.push(bus, saved_af);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x104E, 0x01C9);
    }
    let restored_af = cpu.pop(bus);
    cpu.set_af(restored_af);

    if (cpu.f & FLAG_Z) != 0 {
        // $01CD
        cpu.a = bus.read_byte(1, 0x8646);
        cpu.cb_op(0x47, cpu.a);
        if (cpu.f & FLAG_Z) == 0 {
            // $0235
            cpu.xor8(cpu.a);
            bus.write_byte(1, 0x98A0, 0x00);
            bus.write_byte(1, 0x98A1, 0x00);
            cpu.a = bus.read_byte(1, 0x85AE);
            cpu.cb_op(0x4F, cpu.a);
            cpu.a = if (cpu.f & FLAG_Z) != 0 { 0x1C } else { 0x1D };
            bus.write_byte(1, 0x9822, cpu.a);
        } else {
            // $01D5
            cpu.set_hl(0x8403);
            let v8403 = bus.read_byte(1, 0x8403);
            cpu.cb_op(0x56, v8403);
            if (cpu.f & FLAG_Z) != 0 {
                // $022B
                call_cpu_sub(cpu, bus, roms, 0x10C7, 0x022E);
                cpu.set_hl(0x8404);
                let v = cpu.cb_op(0x96, bus.read_byte(1, 0x8404));
                bus.write_byte(1, 0x8404, v);
            } else {
                // $01DC: Harpoon / Pump active
                cpu.a = bus.read_byte(1, 0x8785);
                cpu.cb_op(0x7F, cpu.a);
                let use_8970 = if (cpu.f & FLAG_Z) == 0 {
                    true
                } else {
                    cpu.a = bus.read_byte(1, 0x8657);
                    cpu.cb_op(0x4F, cpu.a);
                    (cpu.f & FLAG_Z) == 0
                };
                cpu.a = if use_8970 {
                    bus.read_byte(1, 0x8970)
                } else {
                    bus.read_byte(1, 0x85B0)
                };
                cpu.c = cpu.a;
                cpu.and8(0x0F);
                cpu.sub8(0x08, false, false);
                let mut set_879f_1 = (cpu.f & FLAG_Z) != 0;
                if !set_879f_1 {
                    cpu.a = cpu.c;
                    cpu.cb_op(0x6F, cpu.a);
                    set_879f_1 = (cpu.f & FLAG_Z) != 0;
                }

                let mut continue_to_0253 = true;
                if set_879f_1 {
                    // $024E
                    cpu.a = 0x01;
                    bus.write_byte(1, 0x879F, 0x01);
                } else {
                    // $01FF
                    cpu.set_hl(0x879F);
                    let v9f = bus.read_byte(1, 0x879F);
                    cpu.a = v9f;
                    cpu.and8(0x0F);
                    cpu.sub8(0x00, false, false);
                    if (cpu.f & FLAG_Z) == 0 {
                        let dec_v = cpu.dec8(v9f);
                        bus.write_byte(1, 0x879F, dec_v);
                    } else {
                        // $020C
                        cpu.set_hl(0x8403);
                        let r3 = cpu.cb_op(0x96, bus.read_byte(1, 0x8403));
                        bus.write_byte(1, 0x8403, r3);
                        cpu.set_hl(0x8404);
                        let r4 = cpu.cb_op(0x96, bus.read_byte(1, 0x8404));
                        bus.write_byte(1, 0x8404, r4);
                        cpu.xor8(cpu.a);
                        bus.write_byte(1, 0x8641, 0x00);
                        bus.write_byte(1, 0x98A0, 0x00);
                        bus.write_byte(1, 0x98A1, 0x00);
                        cpu.set_hl(0x9882);
                        cpu.b = 0x08;
                        while cpu.b != 0 {
                            bus.write_byte(1, cpu.hl(), 0x00);
                            cpu.set_hl(cpu.hl().wrapping_add(1));
                            bus.write_byte(1, 0x6830, cpu.a);
                            cpu.b = cpu.b.wrapping_sub(1);
                        }
                        call_cpu_sub(cpu, bus, roms, 0x10C7, 0x022E);
                        cpu.set_hl(0x8404);
                        let r4b = cpu.cb_op(0x96, bus.read_byte(1, 0x8404));
                        bus.write_byte(1, 0x8404, r4b);
                        continue_to_0253 = false;
                    }
                }

                if continue_to_0253 {
                    // $0253
                    cpu.a = bus.read_byte(1, 0x98A2);
                    bus.write_byte(1, 0x98A0, cpu.a);
                    cpu.a = bus.read_byte(1, 0x98A3);
                    bus.write_byte(1, 0x98A1, cpu.a);
                    cpu.a = bus.read_byte(1, 0x9922);
                    bus.write_byte(1, 0x9920, cpu.a);
                    cpu.a = 0x07;
                    bus.write_byte(1, 0x9821, 0x07);
                    cpu.a = bus.read_byte(1, 0x8403);
                    cpu.cb_op(0x5F, cpu.a);
                    if (cpu.f & FLAG_Z) != 0 {
                        // $0271
                        cpu.a = bus.read_byte(1, 0x87E4);
                        cpu.cb_op(0x4F, cpu.a);
                        if (cpu.f & FLAG_Z) == 0 {
                            cpu.set_de(0x0C0E);
                            call_cpu_sub(cpu, bus, roms, 0x040E, 0x027E);
                        } else {
                            cpu.set_de(0x080A);
                            call_cpu_sub(cpu, bus, roms, 0x040E, 0x0286);
                        }
                    } else {
                        // $0288
                        cpu.a = bus.read_byte(1, 0x85AE);
                        cpu.cb_op(0x4F, cpu.a);
                        if (cpu.f & FLAG_Z) == 0 {
                            cpu.set_de(0x0D0F);
                            call_cpu_sub(cpu, bus, roms, 0x040E, 0x0295);
                        } else {
                            cpu.set_de(0x090B);
                            call_cpu_sub(cpu, bus, roms, 0x040E, 0x029D);
                        }
                    }
                }
            }
        }
    }

    // $029D
    call_cpu_sub(cpu, bus, roms, 0x130E, 0x02A0);
    call_cpu_sub(cpu, bus, roms, 0x1967, 0x02A3);

    cpu.a = bus.read_byte(1, 0x8646);
    cpu.cb_op(0x47, cpu.a);
    let mut clear_spr = (cpu.f & FLAG_Z) == 0;
    if !clear_spr {
        cpu.cb_op(0x4F, cpu.a);
        clear_spr = (cpu.f & FLAG_Z) == 0;
    }
    if !clear_spr {
        cpu.set_hl(0x8401);
        let v8401 = bus.read_byte(1, 0x8401);
        cpu.cb_op(0x66, v8401);
        clear_spr = (cpu.f & FLAG_Z) == 0;
    }

    if !clear_spr {
        call_cpu_sub(cpu, bus, roms, 0x0315, 0x02B8);
        cpu.a = bus.read_byte(1, 0x98A2);
        bus.write_byte(1, 0x9880, cpu.a);
        cpu.a = bus.read_byte(1, 0x98A3);
        bus.write_byte(1, 0x9881, cpu.a);
    } else {
        // $02C6
        cpu.xor8(cpu.a);
        bus.write_byte(1, 0x9880, 0x00);
        bus.write_byte(1, 0x9881, 0x00);
        call_cpu_sub(cpu, bus, roms, 0x0315, 0x02D0);
    }

    decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
}

fn decomp_cpu1_vblank_exit_02e5<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x1967, 0x02E8);
    decomp_cpu1_vblank_exit_02d0(cpu, bus, roms);
}

fn decomp_cpu1_vblank_exit_02d0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    bus.write_byte(1, 0x6830, cpu.a);
    cpu.set_hl(0x8656);
    let v = bus.read_byte(1, 0x8656);
    cpu.cb_op(0x5E, v);
    if (cpu.f & FLAG_Z) == 0 {
        let v_res = cpu.cb_op(0x9E, v);
        bus.write_byte(1, 0x8656, v_res);
        call_cpu_sub(cpu, bus, roms, 0x02EA, 0x02DF);
    }
    decomp_cpu1_vblank_exit_02df(cpu, bus);
}

fn decomp_cpu1_vblank_exit_02df<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = 0x01;
    bus.write_byte(1, 0x6821, 0x01);
}

/// Executes a single step of CPU 1 (Sub / Game & Sprite AI Z80) 100% in Human Rust!
pub fn step_human_cpu1<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    stats: &mut DecompStats,
) -> u32 {
    if cpu.halted {
        cpu.total_cycles += 4;
        return 4;
    }
    if try_run_decompiled_subroutine(cpu, bus, roms, stats) {
        flush_nested_stats(stats);
        cpu.instructions_executed += 1;
        cpu.r = (cpu.r & 0x80) | (cpu.r.wrapping_add(1) & 0x7F);
        return 32;
    }

    let cycles = match cpu.pc {
        // Reset & Startup ($0000..$0003)
        0x0000 => {
            cpu.iff1 = false;
            cpu.iff2 = false;
            cpu.pc = 0x0001;
            4
        }
        0x0001 => {
            cpu.im = 1;
            cpu.pc = 0x0003;
            8
        }
        0x0003 => {
            cpu.pc = 0x0068;
            10
        }

        // IM 1 VBlank Interrupt Vector ($0038..$0044)
        0x0038 => {
            let hl = cpu.hl();
            cpu.push(bus, hl);
            cpu.pc = 0x0039;
            11
        }
        0x0039 => {
            let de = cpu.de();
            cpu.push(bus, de);
            cpu.pc = 0x003A;
            11
        }
        0x003A => {
            let bc = cpu.bc();
            cpu.push(bus, bc);
            cpu.pc = 0x003B;
            11
        }
        0x003B => {
            let af = cpu.af();
            cpu.push(bus, af);
            cpu.pc = 0x003C;
            11
        }
        0x003C => {
            cpu.push(bus, 0x003F);
            cpu.pc = 0x0080;
            17
        }
        0x003F => {
            let af = cpu.pop(bus);
            cpu.set_af(af);
            cpu.pc = 0x0040;
            10
        }
        0x0040 => {
            let bc = cpu.pop(bus);
            cpu.set_bc(bc);
            cpu.pc = 0x0041;
            10
        }
        0x0041 => {
            let de = cpu.pop(bus);
            cpu.set_de(de);
            cpu.pc = 0x0042;
            10
        }
        0x0042 => {
            let hl = cpu.pop(bus);
            cpu.set_hl(hl);
            cpu.pc = 0x0043;
            10
        }
        0x0043 => {
            cpu.iff1 = true;
            cpu.iff2 = true;
            cpu.pc = 0x0044;
            4
        }
        0x0044 => {
            cpu.pc = cpu.pop(bus);
            10
        }

        // NMI ($0066: RETN)
        0x0066 => {
            cpu.iff1 = cpu.iff2;
            cpu.pc = cpu.pop(bus);
            14
        }

        // Boot & Main Idle Loop ($0068..$007E)
        0x0068 => {
            cpu.xor8(cpu.a);
            cpu.pc = 0x0069;
            4
        }
        0x0069 => {
            bus.write_byte(1, 0x6821, cpu.a);
            cpu.pc = 0x006C;
            13
        }
        0x006C => {
            cpu.sp = 0x9A80;
            cpu.pc = 0x006F;
            10
        }
        0x006F => {
            cpu.push(bus, 0x0072);
            cpu.pc = 0x1F07;
            17
        }
        0x0072 => {
            cpu.a = 0x01;
            cpu.pc = 0x0074;
            7
        }
        0x0074 => {
            bus.write_byte(1, 0x6821, cpu.a);
            cpu.pc = 0x0077;
            13
        }
        0x0077 => {
            cpu.iff1 = true;
            cpu.iff2 = true;
            cpu.pc = 0x0078;
            4
        }
        0x0078 => {
            cpu.sp = 0x9A80;
            cpu.pc = 0x007B;
            10
        }
        0x007B => {
            cpu.push(bus, 0x007E);
            cpu.pc = 0x1DDC;
            17
        }
        0x007E => {
            cpu.pc = 0x0078;
            12
        }

        // Handshake wait loop ($1F25..$1F2B)
        0x1F25 => {
            cpu.a = bus.read_byte(1, 0x8A00);
            cpu.pc = 0x1F28;
            13
        }
        0x1F28 => {
            cpu.and8(cpu.a);
            cpu.pc = 0x1F29;
            4
        }
        0x1F29 => {
            if (cpu.f & FLAG_Z) == 0 {
                cpu.pc = 0x1F25;
                12
            } else {
                cpu.pc = 0x1F2B;
                7
            }
        }
        0x1F2B => {
            cpu.pc = cpu.pop(bus);
            10
        }

        pc => {
            let ret = pc.wrapping_add(1);
            cpu.push(bus, ret);
            cpu.pc = 0x0038;
            11
        }
    };

    cpu.instructions_executed += 1;
    cpu.r = (cpu.r & 0x80) | (cpu.r.wrapping_add(1) & 0x7F);
    cpu.total_cycles += cycles as u64;
    cycles
}

// =========================================================================
// CPU 0 PART 5A: REMAINING SUBROUTINES ($0020, $0C05..$199B, $1DEC, $3575..$3DE3)
// =========================================================================

#[inline]
fn jp_cpu_sub<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    target_pc: u16,
) {
    cpu.pc = target_pc;
    let mut sub_stats = DecompStats::default();
    let ok = try_run_decompiled_subroutine(cpu, bus, roms, &mut sub_stats);
    SUB_STATS_ACCUM.with(|acc| acc.borrow_mut().merge(&sub_stats));
    debug_assert!(
        ok,
        "Missing decompiled subroutine (cpu={}, ${:04X})",
        cpu.cpu_id,
        target_pc
    );
}

#[inline]
fn inline_rst_08(cpu: &mut Z80) {
    let a = cpu.a;
    cpu.add8(a, false);
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
    inline_rst_10(cpu);
}

#[inline]
fn inline_rst_10(cpu: &mut Z80) {
    let l = cpu.l;
    cpu.add8(l, false);
    cpu.l = cpu.a;
    if (cpu.f & FLAG_C) != 0 {
        cpu.h = cpu.inc8(cpu.h);
    }
}

#[inline]
fn z80_rra(cpu: &mut Z80) {
    let old_c = cpu.f & FLAG_C;
    let new_c = cpu.a & 0x01;
    cpu.a = (cpu.a >> 1) | (old_c << 7);
    cpu.f = (cpu.f & (FLAG_S | FLAG_Z | FLAG_PV)) | (cpu.a & 0x28) | new_c;
}

#[inline]
fn z80_cpl(cpu: &mut Z80) {
    cpu.a = !cpu.a;
    cpu.f = (cpu.f & (FLAG_S | FLAG_Z | FLAG_PV | FLAG_C)) | FLAG_H | FLAG_N | (cpu.a & 0x28);
}

#[inline]
fn z80_exx(cpu: &mut Z80) {
    std::mem::swap(&mut cpu.b, &mut cpu.b_alt);
    std::mem::swap(&mut cpu.c, &mut cpu.c_alt);
    std::mem::swap(&mut cpu.d, &mut cpu.d_alt);
    std::mem::swap(&mut cpu.e, &mut cpu.e_alt);
    std::mem::swap(&mut cpu.h, &mut cpu.h_alt);
    std::mem::swap(&mut cpu.l, &mut cpu.l_alt);
}

/// $0020: Modulo-100 subtraction loop (`while A >= 100 { A -= 100 }`).
pub fn decomp_mod_100_loop_0020<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    loop {
        cpu.sub8(0x64, false, false);
        if (cpu.f & FLAG_C) != 0 {
            cpu.ret_subroutine(bus);
            cpu.total_cycles += 18;
            return;
        }
        cpu.sub8(0x64, false, true);
        cpu.total_cycles += 26;
    }
}

/// $0C05: Spawns stage monsters/rocks and draws high score HUD.
pub fn decomp_init_stage_actors_and_hud_0c05<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x1A39, 0x0C08);
    call_cpu_sub(cpu, bus, roms, 0x1A94, 0x0C0B);
    call_cpu_sub(cpu, bus, roms, 0x1A73, 0x0C0E);
    call_cpu_sub(cpu, bus, roms, 0x1AC6, 0x0C11);
    call_cpu_sub(cpu, bus, roms, 0x1432, 0x0C14);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 95;
}

/// $0C15: Clears monster/rock work RAM, advances round HUD, and carves initial tunnels.
pub fn decomp_setup_round_tunnels_and_terrain_0c15<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x188D, 0x0C18);
    call_cpu_sub(cpu, bus, roms, 0x18A7, 0x0C1B);
    call_cpu_sub(cpu, bus, roms, 0x19A3, 0x0C1E);
    call_cpu_sub(cpu, bus, roms, 0x18AC, 0x0C21);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 80;
}

/// $0C22: Initializes playfield VRAM, player lives, round counters, and clears work variables.
pub fn decomp_init_round_state_and_clear_counters_0c22<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    call_cpu_sub(cpu, bus, roms, 0x17AE, 0x0C25);
    call_cpu_sub(cpu, bus, roms, 0x183E, 0x0C28);
    call_cpu_sub(cpu, bus, roms, 0x1845, 0x0C2B);
    call_cpu_sub(cpu, bus, roms, 0x17F8, 0x0C2E);
    call_cpu_sub(cpu, bus, roms, 0x1551, 0x0C31);
    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x87A7, 0x00);
    bus.write_byte(0, 0x87BF, 0x00);
    bus.write_byte(0, 0x879F, 0x00);
    bus.write_byte(0, 0x87EB, 0x00);
    bus.write_byte(0, 0x87DA, 0x00);
    bus.write_byte(0, 0x87D8, 0x00);
    cpu.set_hl(0x0000);
    bus.write_word(0, 0x87ED, 0x0000);
    bus.write_word(0, 0x87D4, 0x0000);
    bus.write_word(0, 0x87D2, 0x0000);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 230;
}

/// $1597: Fills $8000..$83BF with $7F and syncs round indicator HUD.
pub fn decomp_draw_title_screen_logo_1597<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    cpu.set_hl(0x8000);
    cpu.set_bc(0x03C0);
    call_cpu_sub(cpu, bus, roms, 0x15BF, 0x15A1);
    call_cpu_sub(cpu, bus, roms, 0x11EC, 0x15A4);
    cpu.set_af(saved_af);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 65;
}

/// $15A6: Fills $8040..$83FE with $7F while pulsing watchdog.
pub fn decomp_clear_sound_and_sub_cpu_ram_15a6<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    cpu.set_hl(0x8040);
    cpu.set_bc(0x0380);
    call_cpu_sub(cpu, bus, roms, 0x15BF, 0x15B0);
    for addr in 0x83C0..0x83FFu16 {
        bus.write_byte(0, addr, 0x7F);
        bus.write_byte(0, 0x6830, cpu.a);
    }
    cpu.set_hl(0x83FF);
    cpu.b = 0;
    cpu.set_af(saved_af);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 900;
}

/// $1606: Draws "PUSH START BUTTON" ("ONLY 1 PLAYER" or "1 OR 2 PLAYERS") on the credit screen.
pub fn decomp_draw_push_start_prompt_1606<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x812D);
    cpu.set_de(0x0020);
    cpu.a = bus.read_byte(0, 0x85A5);
    cpu.sub8(0x01, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.ix = 0x306F;
        cpu.a = 0x0D;
    } else {
        cpu.ix = 0x307C;
        cpu.a = 0x0E;
    }
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x161C);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 80;
}

/// $1625: Draws additional credit screen prompt columns.
pub fn decomp_draw_character_cast_table_1625<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    cpu.set_hl(0x80FA);
    cpu.set_de(0x0020);
    cpu.ix = 0x308A;
    cpu.a = 0x11;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1635);
    cpu.set_hl(0x819C);
    cpu.ix = 0x309B;
    cpu.a = 0x08;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x168E);
    cpu.set_af(saved_af);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 120;
}

/// $1640: Draws bonus life text lines on the credit screen according to $87C0 mode.
pub fn decomp_setup_cast_sprites_1640<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    cpu.set_hl(0x80EA);
    cpu.set_de(0x0020);
    cpu.ix = 0x30A3;
    cpu.a = 0x11;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1650);
    let c0 = bus.read_byte(0, 0x87C0);
    cpu.a = c0;
    cpu.and8(c0);
    if c0 != 0 {
        cpu.set_hl(0x80B1);
        cpu.ix = 0x30B4;
        cpu.a = 0x14;
        call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1664);
        let c0 = bus.read_byte(0, 0x87C0);
        cpu.a = c0;
        cpu.sub8(0x01, false, false);
        if c0 != 1 {
            cpu.set_hl(0x80B4);
            cpu.ix = 0x30C8;
            cpu.a = 0x14;
            call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1679);
            let c0 = bus.read_byte(0, 0x87C0);
            cpu.a = c0;
            cpu.sub8(0x02, false, false);
            if c0 != 2 {
                cpu.set_hl(0x80B7);
                cpu.ix = 0x30DC;
                cpu.a = 0x14;
                call_cpu_sub(cpu, bus, roms, 0x15CB, 0x168E);
            }
        }
    }
    cpu.set_af(saved_af);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 180;
}

/// $1690: Draws the bonus life score threshold digits on the credit screen.
pub fn decomp_animate_title_namco_logo_1690<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    let c0 = bus.read_byte(0, 0x87C0);
    cpu.a = c0;
    cpu.and8(c0);
    if c0 != 0 {
        cpu.set_de(0x0020);
        cpu.set_hl(0x81B1);
        cpu.set_bc(0x85AA);
        let a0 = bus.read_byte(0, 0x85AA);
        cpu.a = a0;
        cpu.add8(0x50, false);
        bus.write_byte(0, 0x81B1, cpu.a);
        cpu.set_bc(0x85AB);
        let hl1 = cpu.add16(0x81B1, 0x0020);
        cpu.set_hl(hl1);
        let a1 = bus.read_byte(0, 0x85AB);
        cpu.a = a1;
        cpu.and8(a1);
        if a1 != 0 {
            cpu.add8(0x50, false);
            bus.write_byte(0, hl1, cpu.a);
        } else {
            bus.write_byte(0, hl1, 0x7F);
        }
        let c0 = bus.read_byte(0, 0x87C0);
        cpu.a = c0;
        cpu.sub8(0x01, false, false);
        if c0 != 1 {
            cpu.set_hl(0x81B4);
            call_cpu_sub(cpu, bus, roms, 0x16D3, 0x16C2);
            let c0 = bus.read_byte(0, 0x87C0);
            cpu.a = c0;
            cpu.sub8(0x02, false, false);
            if c0 != 2 {
                cpu.set_hl(0x81B7);
                call_cpu_sub(cpu, bus, roms, 0x16D3, 0x16D1);
            }
        }
    }
    cpu.set_af(saved_af);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 160;
}

/// $16D3: Draws the 2nd/recurring bonus life digits and falls through into $16E5.
pub fn decomp_draw_bonus_pair_and_sync_flips_16d3<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_bc(0x85AC);
    cpu.set_de(0x0020);
    let a0 = bus.read_byte(0, 0x85AC);
    cpu.a = a0;
    cpu.add8(0x50, false);
    bus.write_byte(0, cpu.hl(), cpu.a);
    cpu.set_bc(0x85AD);
    let hl1 = cpu.add16(cpu.hl(), 0x0020);
    cpu.set_hl(hl1);
    let ix1 = cpu.add16(cpu.ix, 0x0020);
    cpu.ix = ix1;
    let a1 = bus.read_byte(0, 0x85AD);
    cpu.a = a1;
    cpu.add8(0x50, false);
    bus.write_byte(0, hl1, cpu.a);
    jp_cpu_sub(cpu, bus, roms, 0x16E5);
}

/// $199B: Copies attract demo round ($9A05 -> $9A06) and draws the round indicator HUD ($19C5).
pub fn decomp_abort_attract_demo_199b<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let a = bus.read_byte(0, 0x9A05);
    bus.write_byte(0, 0x9A06, a);
    cpu.a = a;
    cpu.set_hl(0x9A06);
    decomp_draw_round_indicator_from_19c5(cpu, bus, roms);
}

/// $0A71..$0A7F: VBlank handler epilogue (`SET 0, A; LD ($6820), A; POP AF..IX; EI; RET`).
pub fn decomp_vblank_epilogue_0a71<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    cpu.a = cpu.cb_op(0xC7, cpu.a);
    bus.write_byte(0, 0x6820, cpu.a);
    let af = cpu.pop(bus);
    let bc = cpu.pop(bus);
    let de = cpu.pop(bus);
    let hl = cpu.pop(bus);
    let iy = cpu.pop(bus);
    let ix = cpu.pop(bus);
    cpu.set_af(af);
    cpu.set_bc(bc);
    cpu.set_de(de);
    cpu.set_hl(hl);
    cpu.iy = iy;
    cpu.ix = ix;
    cpu.iff1 = true;
    cpu.iff2 = true;
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 99;
}

/// $1DEC..$1ECA: Attract mode title screen initialization and frame update, jumping to $0A71.
pub fn decomp_update_attract_title_screen_1dec<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let hl = cpu.hl();
    let m = bus.read_byte(0, hl);
    cpu.cb_op(0x56, m); // BIT 2, (HL)
    if (m & 0x04) == 0 {
        let m2 = cpu.cb_op(0xD6, m); // SET 2, (HL)
        bus.write_byte(0, hl, m2);
        cpu.set_hl(0x173A);
        bus.write_word(0, 0x8789, 0x173A);
        cpu.a = 0x01;
        bus.write_byte(0, 0xA003, 0x01);

        call_cpu_sub(cpu, bus, roms, 0x188D, 0x1E02);
        call_cpu_sub(cpu, bus, roms, 0x18A7, 0x1E05);
        call_cpu_sub(cpu, bus, roms, 0x0BF2, 0x1E08);
        call_cpu_sub(cpu, bus, roms, 0x1597, 0x1E0B);
        call_cpu_sub(cpu, bus, roms, 0x146B, 0x1E0E);
        call_cpu_sub(cpu, bus, roms, 0x0BCA, 0x1E11);
        call_cpu_sub(cpu, bus, roms, 0x0BDF, 0x1E14);
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }

    call_cpu_sub(cpu, bus, roms, 0x15DB, 0x1E1A);

    let timer_hl = bus.read_word(0, 0x8789);
    cpu.set_hl(timer_hl);
    cpu.l = cpu.inc8(cpu.l);
    cpu.a = cpu.l;
    cpu.sub8(0x3C, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.l = 0x00;
        cpu.and8(cpu.a);
        cpu.h = cpu.dec8(cpu.h);
        cpu.a = cpu.h;
        cpu.sub8(0x0F, false, false);
        if (cpu.f & FLAG_Z) != 0 {
            cpu.h = 0x09;
        }
        cpu.a = cpu.h;
        cpu.sub8(0x40, false, false);
        if (cpu.f & FLAG_C) == 0 {
            cpu.set_hl(0x8787);
            let v = bus.read_byte(0, 0x8787);
            let v2 = cpu.cb_op(0xCE, v); // SET 1, (HL)
            bus.write_byte(0, 0x8787, v2);
            decomp_vblank_epilogue_0a71(cpu, bus);
            return;
        }
        cpu.and8(0x0F);
        cpu.add8(0x50, false);
        bus.write_byte(0, 0x81D7, cpu.a);
        cpu.a = cpu.h;
        for _ in 0..4 {
            cpu.a = cpu.cb_op(0x3F, cpu.a); // SRL A
        }
        cpu.add8(0x50, false);
        bus.write_byte(0, 0x81F7, cpu.a);
    }

    bus.write_word(0, 0x8789, cpu.hl());

    cpu.a = 0x04;
    cpu.set_hl(0x8257);
    cpu.ix = 0x1D68;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1E5F);

    cpu.a = 0x14;
    cpu.set_hl(0x80D4);
    cpu.ix = 0x1D6C;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1E6B);

    cpu.a = 0x06;
    cpu.set_hl(0x8292);
    cpu.ix = 0x1D96;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1E77);

    cpu.a = 0x16;
    cpu.set_hl(0x80CE);
    cpu.ix = 0x1D80;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1E83);

    cpu.a = 0x19;
    cpu.set_hl(0x806C);
    cpu.ix = 0x1D4F;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1E8F);

    let timer_hl = bus.read_word(0, 0x8789);
    cpu.set_hl(timer_hl);
    cpu.a = cpu.l;
    cpu.sub8(0x1E, false, false);
    cpu.ix = if (cpu.f & FLAG_C) != 0 { 0x1DAF } else { 0x1D9C };
    cpu.set_hl(0x80E6);
    cpu.a = 0x13;
    call_cpu_sub(cpu, bus, roms, 0x15CB, 0x1EA9);

    cpu.a = bus.read_byte(0, 0x85A5);
    cpu.sub8(0x00, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x1ECC, 0x1EB3);
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }
    cpu.sub8(0x01, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x1ECC, 0x1EBD);
        call_cpu_sub(cpu, bus, roms, 0x1ED9, 0x1EC0);
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }
    call_cpu_sub(cpu, bus, roms, 0x1EE6, 0x1EC6);
    call_cpu_sub(cpu, bus, roms, 0x1EF3, 0x1EC9);
    decomp_vblank_epilogue_0a71(cpu, bus);
}

/// $3575: Draws the 6-digit top high score in big 4x2 tiles at $836D.
pub fn decomp_draw_big_highscore_digits_3575<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x8980);
    cpu.set_de(0x836D);
    cpu.b = 0x03;
    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x8983, 0x00);

    loop {
        cpu.a = bus.read_byte(0, cpu.hl());
        for _ in 0..4 {
            z80_rra(cpu);
        }
        call_cpu_sub(cpu, bus, roms, 0x3593, 0x3589);

        cpu.a = bus.read_byte(0, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x3593, 0x358D);

        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.a = 0x01;
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 120;
}

/// $3593: Draws a single big 4x2 digit (or suppresses leading zeroes).
pub fn decomp_draw_single_big_digit_3593<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.and8(0x0F);
    if (cpu.f & FLAG_Z) != 0 {
        let seen = bus.read_byte(0, 0x8983);
        cpu.a = seen;
        cpu.and8(seen);
        cpu.a = 0x00;
        if (cpu.f & FLAG_Z) != 0 {
            call_cpu_sub(cpu, bus, roms, 0x36E9, 0x35A2);
            jp_cpu_sub(cpu, bus, roms, 0x36E9);
            return;
        }
    }

    let saved_hl = cpu.hl();
    let saved_bc = cpu.bc();
    cpu.set_hl(0x37CA);
    inline_rst_08(cpu);
    let lo = bus.read_byte(0, cpu.hl());
    cpu.set_hl(cpu.hl().wrapping_add(1));
    let hi = bus.read_byte(0, cpu.hl());
    cpu.set_hl(((hi as u16) << 8) | (lo as u16));
    cpu.set_bc(0x04FF);

    loop {
        let saved_de = cpu.de();
        cpu.ldi(bus);
        cpu.ldi(bus);
        cpu.ldi(bus);
        cpu.ldi(bus);
        cpu.set_de(saved_de);
        call_cpu_sub(cpu, bus, roms, 0x36E9, 0x35BF);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }

    cpu.set_bc(saved_bc);
    cpu.set_hl(saved_hl);
    cpu.a = 0x01;
    bus.write_byte(0, 0x8983, 0x01);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 180;
}

/// $35E3: Clears active highlight row ($89C5 = 0) and renders the 5-row high-score table.
pub fn decomp_render_highscore_table_35e3<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x89C5, 0x00);
    decomp_render_highscore_entries_35e7(cpu, bus, roms);
}

/// $35E7: Renders the header and all 5 rows of the high-score table.
pub fn decomp_render_highscore_entries_35e7<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x3763);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x35ED);
    cpu.set_de(0x835D);
    cpu.b = 0x05;
    loop {
        call_cpu_sub(cpu, bus, roms, 0x35F8, 0x35F5);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 60;
}

/// $35F8: Renders a single high-score table row (rank, score, round, initials).
pub fn decomp_render_single_highscore_row_35f8<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.c = 0x00;
    cpu.a = bus.read_byte(0, 0x89C5);
    cpu.sub8(cpu.b, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.c = 0x40;
    }
    cpu.a = cpu.b;
    cpu.set_hl(0x368D);
    inline_rst_08(cpu);
    let lo = bus.read_byte(0, cpu.hl());
    cpu.set_hl(cpu.hl().wrapping_add(1));
    let hi = bus.read_byte(0, cpu.hl());
    cpu.set_hl(((hi as u16) << 8) | (lo as u16));

    let saved_de = cpu.de();
    let saved_bc = cpu.bc();
    cpu.b = 0x03;
    loop {
        let ch = bus.read_byte(0, cpu.hl());
        cpu.a = ch;
        cpu.or8(cpu.c);
        bus.write_byte(0, cpu.de(), cpu.a);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        call_cpu_sub(cpu, bus, roms, 0x36E9, 0x3616);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }

    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x361B);
    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x361E);
    call_cpu_sub(cpu, bus, roms, 0x3645, 0x3621);

    cpu.b = 0x04;
    loop {
        call_cpu_sub(cpu, bus, roms, 0x36E9, 0x3626);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }

    call_cpu_sub(cpu, bus, roms, 0x3664, 0x362B);
    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x362E);
    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x3631);

    let init_lo = bus.read_byte(0, cpu.hl());
    cpu.set_hl(cpu.hl().wrapping_add(1));
    let init_hi = bus.read_byte(0, cpu.hl());
    cpu.set_hl(((init_hi as u16) << 8) | (init_lo as u16));
    cpu.b = 0x03;
    loop {
        let ch = bus.read_byte(0, cpu.hl());
        cpu.a = ch;
        cpu.or8(cpu.c);
        bus.write_byte(0, cpu.de(), cpu.a);
        call_cpu_sub(cpu, bus, roms, 0x36E9, 0x363D);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }

    cpu.set_bc(saved_bc);
    cpu.set_de(saved_de.wrapping_sub(2));
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 160;
}

/// $3645: Renders a 6-digit BCD score in a high-score table row.
pub fn decomp_render_highscore_row_score_3645<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let lo = bus.read_byte(0, cpu.hl());
    let saved_hl = cpu.hl().wrapping_add(1);
    let hi = bus.read_byte(0, saved_hl);
    cpu.set_hl(((hi as u16) << 8) | (lo as u16));
    let saved_bc = cpu.bc();
    let saved_de = cpu.de();
    cpu.set_bc(0x0300);
    call_cpu_sub(cpu, bus, roms, 0x36C6, 0x3652);
    cpu.set_de(saved_de);
    cpu.set_bc(saved_bc);
    cpu.b = 0x06;
    loop {
        let v = bus.read_byte(0, cpu.de());
        cpu.a = v;
        cpu.or8(cpu.c);
        bus.write_byte(0, cpu.de(), cpu.a);
        call_cpu_sub(cpu, bus, roms, 0x36E9, 0x365C);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.set_hl(saved_hl.wrapping_add(1));
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 90;
}

/// $3661: Renders the binary number in A as 2 decimal digits at (DE).
pub fn decomp_render_two_digit_number_3661<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_hl = cpu.hl();
    decomp_render_two_digit_common_366a(cpu, bus, roms, saved_hl);
}

/// $3664: Dereferences pointer at (HL) to read round number and renders 2 decimal digits.
pub fn decomp_render_highscore_row_round_3664<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let lo = bus.read_byte(0, cpu.hl());
    let saved_hl = cpu.hl().wrapping_add(1);
    let hi = bus.read_byte(0, saved_hl);
    let ptr = ((hi as u16) << 8) | (lo as u16);
    cpu.set_hl(ptr);
    cpu.a = bus.read_byte(0, ptr);
    decomp_render_two_digit_common_366a(cpu, bus, roms, saved_hl);
}

fn decomp_render_two_digit_common_366a<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    saved_hl: u16,
) {
    cpu.b = 0x00;
    loop {
        cpu.sub8(0x0A, false, true);
        if (cpu.f & FLAG_C) != 0 {
            break;
        }
        cpu.b = cpu.inc8(cpu.b);
    }
    let saved_af = cpu.af();
    cpu.a = cpu.b;
    call_cpu_sub(cpu, bus, roms, 0x3681, 0x3678);
    cpu.set_af(saved_af);
    cpu.add8(0x0A, false);
    call_cpu_sub(cpu, bus, roms, 0x3687, 0x367E);
    cpu.set_hl(saved_hl.wrapping_add(1));
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 80;
}

/// $36C6: Renders B BCD bytes starting at (HL) into VRAM at (DE).
pub fn decomp_render_bcd_bytes_36c6<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    loop {
        cpu.a = bus.read_byte(0, cpu.hl());
        call_cpu_sub(cpu, bus, roms, 0x36CE, 0x36CA);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 50;
}

/// $36CE: Renders the high and low BCD nibbles of A into VRAM at (DE).
pub fn decomp_render_bcd_byte_pair_36ce<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let saved_af = cpu.af();
    for _ in 0..4 {
        z80_rra(cpu);
    }
    call_cpu_sub(cpu, bus, roms, 0x36DC, 0x36D6);
    cpu.set_af(saved_af);
    cpu.b = cpu.b.wrapping_sub(1);
    if cpu.b == 0 {
        cpu.c = 0xFF;
    }
    cpu.b = cpu.inc8(cpu.b);
    decomp_render_bcd_nibble_36dc(cpu, bus, roms);
}

/// $36DC: Renders a single BCD nibble in A into VRAM at (DE) and steps DE left.
pub fn decomp_render_bcd_nibble_36dc<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.and8(0x0F);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.c = 0xFF;
    }
    cpu.or8(0x10);
    cpu.and8(cpu.c);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x36F0, 0x36E8);
    }
    bus.write_byte(0, cpu.de(), cpu.a);
    jp_cpu_sub(cpu, bus, roms, 0x36E9);
}

/// $3AB8: Service mode input edge detector and switch index encoder.
pub fn decomp_service_poll_inputs_3ab8<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x3F8A);
    let cf = bus.read_byte(0, 0x89CF);
    cpu.a = cf;
    cpu.and8(cf);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3AC2);
    }
    cpu.set_hl(0x89D9);
    cpu.set_de(0x89D8);
    cpu.set_bc(0x0007);
    while cpu.bc() != 0 {
        cpu.ldi(bus);
    }
    cpu.a = bus.read_byte(0, 0x85A8);
    cpu.b = 0x02;
    loop {
        cpu.e = cpu.d;
        cpu.set_hl(cpu.hl().wrapping_sub(1));
        bus.write_byte(0, cpu.hl(), cpu.a);
        cpu.set_hl(cpu.hl().wrapping_sub(1));
        let v1 = bus.read_byte(0, cpu.hl());
        cpu.or8(v1);
        cpu.set_hl(cpu.hl().wrapping_sub(1));
        z80_cpl(cpu);
        let v2 = bus.read_byte(0, cpu.hl());
        cpu.and8(v2);
        cpu.set_hl(cpu.hl().wrapping_sub(1));
        let v3 = bus.read_byte(0, cpu.hl());
        cpu.and8(v3);
        bus.write_byte(0, cpu.hl(), cpu.a);
        cpu.d = cpu.a;
        cpu.a = bus.read_byte(0, 0x85A7);
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    let tmp = cpu.de();
    cpu.set_de(cpu.hl());
    cpu.set_hl(tmp);
    let hl2 = cpu.add16(cpu.hl(), cpu.hl());
    cpu.set_hl(hl2);
    cpu.b = 0x0F;
    loop {
        let hl_next = cpu.add16(cpu.hl(), cpu.hl());
        cpu.set_hl(hl_next);
        if (cpu.f & FLAG_C) != 0 {
            break;
        }
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.a = cpu.b;
    bus.write_byte(0, 0x8988, cpu.a);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 210;
}

/// $3AF1: Service mode coin/credit ratio display.
pub fn decomp_service_draw_coin_cred_settings_3af1<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.a = bus.read_byte(0, 0x898F);
    cpu.set_hl(0x3F9C);
    inline_rst_08(cpu);
    let saved_hl = cpu.hl();
    cpu.set_hl(0x3EBF);
    for ret_addr in [0x3AFF, 0x3B02, 0x3B05, 0x3B08] {
        call_cpu_sub(cpu, bus, roms, 0x36F3, ret_addr);
    }
    cpu.set_de(saved_hl);
    cpu.set_hl(0x8348);
    cpu.set_bc(0x8A1A);
    call_cpu_sub(cpu, bus, roms, 0x3B20, 0x3B12);

    cpu.a = bus.read_byte(0, 0x898A);
    cpu.set_hl(0x3FA4);
    inline_rst_10(cpu);
    let tmp = cpu.de();
    cpu.set_de(cpu.hl());
    cpu.set_hl(tmp);
    cpu.set_hl(0x834A);
    cpu.set_bc(0x8A1C);
    decomp_service_draw_ratio_pair_3b20(cpu, bus);
}

/// $3B20: Draws a coin/credit ratio pair into VRAM and updates plural 'S'.
pub fn decomp_service_draw_ratio_pair_3b20<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let a0 = bus.read_byte(0, cpu.de());
    cpu.a = a0;
    bus.write_byte(0, cpu.hl(), a0);
    bus.write_byte(0, cpu.bc(), a0);
    let saved_bc = cpu.bc().wrapping_add(1);
    cpu.sub8(0x11, false, true);
    cpu.a = if (cpu.f & FLAG_Z) == 0 { 0x2C } else { 0x37 };
    cpu.set_bc(0xFF40);
    let hl1 = cpu.add16(cpu.hl(), 0xFF40);
    cpu.set_hl(hl1);
    bus.write_byte(0, hl1, cpu.a);
    cpu.a = 0x37;
    cpu.set_bc(0xFFE0);
    let hl2 = cpu.add16(cpu.hl(), 0xFFE0);
    cpu.set_hl(hl2);
    bus.write_byte(0, hl2, 0x37);
    cpu.set_de(cpu.de().wrapping_add(1));
    let a1 = bus.read_byte(0, cpu.de());
    cpu.a = a1;
    let hl3 = cpu.add16(cpu.hl(), 0xFFE0);
    cpu.set_hl(hl3);
    bus.write_byte(0, hl3, a1);
    cpu.set_bc(saved_bc);
    bus.write_byte(0, saved_bc, a1);
    cpu.sub8(0x11, false, true);
    cpu.a = if (cpu.f & FLAG_Z) == 0 { 0x2C } else { 0x37 };
    cpu.h = cpu.dec8(cpu.h);
    bus.write_byte(0, cpu.hl(), cpu.a);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 120;
}

/// $3B4A: Service mode bonus life threshold display.
pub fn decomp_service_draw_bonus_life_settings_3b4a<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let b898b = bus.read_byte(0, 0x898B);
    cpu.a = b898b;
    cpu.and8(b898b);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0xFFFF);
        bus.write_word(0, 0x89D0, 0xFFFF);
        cpu.set_hl(0x3F6F);
        call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3B5C);
        cpu.set_hl(0x8354);
        call_cpu_sub(cpu, bus, roms, 0x3BB5, 0x3BB2);
        cpu.set_hl(0x8356);
        jp_cpu_sub(cpu, bus, roms, 0x3BB5);
        return;
    }

    cpu.set_hl(0x3FB2);
    cpu.c = cpu.a;
    let b898c = bus.read_byte(0, 0x898C);
    cpu.a = b898c;
    cpu.and8(b898c);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x3FC0);
    }
    cpu.b = 0x00;
    let table_hl = cpu.add16(cpu.hl(), cpu.bc());
    cpu.set_hl(0x3EE7);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3B76);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3B79);

    cpu.set_hl(table_hl);
    let v0 = bus.read_byte(0, table_hl);
    cpu.a = v0;
    bus.write_byte(0, 0x8192, v0);
    bus.write_byte(0, 0x89D0, v0);
    cpu.set_hl(table_hl.wrapping_add(1));
    let v1 = bus.read_byte(0, cpu.hl());
    cpu.a = v1;
    bus.write_byte(0, 0x89D1, v1);
    cpu.sub8(0xFF, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x8354);
        call_cpu_sub(cpu, bus, roms, 0x3BB5, 0x3BB2);
        cpu.set_hl(0x8356);
        jp_cpu_sub(cpu, bus, roms, 0x3BB5);
        return;
    }

    let saved_af = cpu.af();
    cpu.and8(0x1F);
    bus.write_byte(0, 0x8194, cpu.a);
    cpu.set_hl(0x3F04);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3B96);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3B99);
    cpu.set_af(saved_af);
    cpu.cb_op(0x7F, cpu.a); // BIT 7, A
    if (cpu.a & 0x80) == 0 {
        cpu.set_hl(0x8356);
        jp_cpu_sub(cpu, bus, roms, 0x3BB5);
        return;
    }
    cpu.and8(0x1F);
    bus.write_byte(0, 0x8196, cpu.a);
    cpu.set_hl(0x3F21);
    call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3BA9);
    jp_cpu_sub(cpu, bus, roms, 0x36F3);
}

/// $3BFC: Service mode sound test display and trigger.
pub fn decomp_service_update_sound_test_3bfc<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.a = bus.read_byte(0, 0x8989);
    cpu.c = 0x00;
    loop {
        cpu.sub8(0x0A, false, false);
        if (cpu.f & FLAG_C) != 0 {
            break;
        }
        cpu.sub8(0x0A, false, true);
        cpu.c = cpu.inc8(cpu.c);
    }
    cpu.add8(0x10, false);
    bus.write_byte(0, 0x8270, cpu.a);
    cpu.a = cpu.c;
    cpu.add8(0x10, false);
    bus.write_byte(0, 0x8290, cpu.a);
    cpu.set_hl(0x8401);
    let v = bus.read_byte(0, 0x8401);
    let v2 = cpu.cb_op(0xEE, v); // SET 5, (HL)
    bus.write_byte(0, 0x8401, v2);
    let sw = bus.read_byte(0, 0x8988);
    cpu.a = sw;
    cpu.and8(sw);
    if (cpu.f & FLAG_Z) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x3C27, 0x3C21);
    }
    cpu.set_hl(0x3F66);
    jp_cpu_sub(cpu, bus, roms, 0x36F3);
}

/// $3C47: Service mode bookkeeping counter display.
pub fn decomp_service_update_bookkeeping_display_3c47<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let timer = bus.read_word(0, 0x8991);
    cpu.set_hl(timer);
    cpu.a = cpu.h;
    cpu.or8(cpu.l);
    if (cpu.f & FLAG_Z) == 0 {
        let next = timer.wrapping_sub(1);
        cpu.set_hl(next);
        bus.write_word(0, 0x8991, next);
        cpu.ret_subroutine(bus);
        cpu.total_cycles += 42;
        return;
    }

    cpu.a = bus.read_byte(0, 0x8988);
    cpu.sub8(0x0F, false, false);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.set_hl(0x835A);
        jp_cpu_sub(cpu, bus, roms, 0x3BB5);
        return;
    }

    cpu.a = bus.read_byte(0, 0x85A7);
    cpu.and8(0x01);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.a = 0x03;
        bus.write_byte(0, 0x9B3D, 0x03);
        cpu.ret_subroutine(bus);
        cpu.total_cycles += 55;
        return;
    }

    cpu.set_hl(0x04B0);
    bus.write_word(0, 0x8991, 0x04B0);
    cpu.set_hl(0x89E0);
    cpu.set_de(0x835A);
    cpu.set_bc(0x0102);
    call_cpu_sub(cpu, bus, roms, 0x3C8B, 0x3C7F);
    cpu.b = 0x03;
    call_cpu_sub(cpu, bus, roms, 0x3C8B, 0x3C84);
    cpu.b = 0x02;
    call_cpu_sub(cpu, bus, roms, 0x3C8B, 0x3C89);
    cpu.b = 0x01;
    decomp_service_draw_bcd_counter_3c8b(cpu, bus, roms);
}

/// $3C8B: Renders a multi-byte inverted-BCD bookkeeping counter into VRAM.
pub fn decomp_service_draw_bcd_counter_3c8b<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.a = bus.read_byte(0, cpu.hl());
    call_cpu_sub(cpu, bus, roms, 0x3CA0, 0x3C8F);
    cpu.set_hl(cpu.hl().wrapping_add(1));
    loop {
        call_cpu_sub(cpu, bus, roms, 0x3C97, 0x3C93);
        cpu.set_hl(cpu.hl().wrapping_add(1));
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 80;
}

/// $3C97: Renders both nibbles of an inverted-BCD bookkeeping byte.
pub fn decomp_service_draw_bcd_byte_3c97<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.a = bus.read_byte(0, cpu.hl());
    for _ in 0..4 {
        z80_rra(cpu);
    }
    call_cpu_sub(cpu, bus, roms, 0x3CA0, 0x3C9F);
    cpu.a = bus.read_byte(0, cpu.hl());
    decomp_service_draw_bcd_digit_3ca0(cpu, bus, roms);
}

/// $3CA0: Renders one inverted-BCD nibble (`(~A & 0x0F) + 0x10`) and optional spacing.
pub fn decomp_service_draw_bcd_digit_3ca0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.and8(0x0F);
    z80_cpl(cpu);
    cpu.add8(0x1A, false);
    bus.write_byte(0, cpu.de(), cpu.a);
    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x3CA9);
    cpu.c = cpu.dec8(cpu.c);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.ret_subroutine(bus);
        return;
    }
    cpu.c = 0x04;
    cpu.a = 0x35;
    bus.write_byte(0, cpu.de(), 0x35);
    jp_cpu_sub(cpu, bus, roms, 0x36E9);
}

/// $3CB3: Service mode flip-screen toggle and secret Easter-egg code detector.
pub fn decomp_service_check_secret_code_3cb3<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.a = bus.read_byte(0, 0x85A7);
    cpu.and8(0x03);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.a = 0x01;
    }
    cpu.a = cpu.inc8(cpu.a);
    bus.write_byte(0, 0xA007, cpu.a);
    cpu.a = bus.read_byte(0, 0x85A7);
    cpu.and8(0x01);
    if (cpu.f & FLAG_Z) == 0 {
        cpu.xor8(cpu.a);
        bus.write_byte(0, 0x8990, 0x00);
        cpu.ret_subroutine(bus);
        cpu.total_cycles += 55;
        return;
    }
    cpu.a = bus.read_byte(0, 0x8988);
    cpu.a = cpu.dec8(cpu.a);
    cpu.sub8(0x04, false, false);
    if (cpu.f & FLAG_C) == 0 {
        cpu.ret_subroutine(bus);
        cpu.total_cycles += 55;
        return;
    }
    cpu.a = cpu.inc8(cpu.a);
    cpu.c = cpu.a;
    let idx = bus.read_byte(0, 0x8990);
    cpu.e = idx;
    cpu.a = cpu.inc8(idx);
    bus.write_byte(0, 0x8990, cpu.a);
    cpu.d = 0x00;
    let hl = cpu.add16(0x3FD0, cpu.de());
    cpu.set_hl(hl);
    let expected = bus.read_byte(0, hl);
    cpu.a = cpu.inc8(expected);
    if (cpu.f & FLAG_Z) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x3E3B, 0x3CF1);
        cpu.set_de(0x3D24);
        cpu.set_hl(0x8042);
        cpu.b = 0x1C;
        loop {
            call_cpu_sub(cpu, bus, roms, 0x3D07, 0x3CFC);
            cpu.b = cpu.b.wrapping_sub(1);
            if cpu.b == 0 {
                break;
            }
        }
        cpu.pc = 0x3CFE;
        return;
    }
    cpu.sub8(cpu.c, false, false);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.ret_subroutine(bus);
        return;
    }
    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x8990, 0x00);
    cpu.ret_subroutine(bus);
}

/// $3DE3: Runs the 16-pass LFSR RAM self-test over 1 KB at (HL) and clears it to $00.
pub fn decomp_ram_bank_lfsr_test_3de3<B: Z80Bus>(cpu: &mut Z80, bus: &mut B) {
    let base = cpu.hl();
    z80_exx(cpu);
    cpu.b = 0x10;
    loop {
        cpu.d = cpu.h;
        cpu.e = cpu.l;
        for _ in 0..0x0400 {
            let a = !(cpu.h ^ cpu.l);
            let carry = (a & 0x40) != 0;
            let next_hl = (cpu.hl() << 1) | (carry as u16);
            cpu.set_hl(next_hl);
        }
        cpu.d = cpu.h;
        cpu.e = cpu.l;
        cpu.b = cpu.b.wrapping_sub(1);
        if cpu.b == 0 {
            break;
        }
    }
    z80_exx(cpu);
    for i in 0..0x0400u16 {
        bus.write_byte(0, base.wrapping_add(i), 0x00);
    }
    bus.write_byte(0, 0x6830, 0x00);
    cpu.a = 0x00;
    cpu.set_hl(base.wrapping_add(0x0400));
    cpu.set_de(base.wrapping_add(0x0400));
    cpu.b = 0x00;
    cpu.c = cpu.dec8(1);
    cpu.ret_subroutine(bus);
    cpu.total_cycles += 50_000;
}

/// $0280..$0BA5: Complete CPU 0 VBlank Interrupt Handler.
pub fn decomp_cpu0_vblank_handler<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.push(bus, cpu.ix);
    cpu.push(bus, cpu.iy);
    cpu.push(bus, cpu.hl());
    cpu.push(bus, cpu.de());
    cpu.push(bus, cpu.bc());
    cpu.push(bus, cpu.af());

    cpu.xor8(cpu.a);
    bus.write_byte(0, 0x6820, 0x00);
    bus.write_byte(0, 0x6830, 0x00);

    let in_self_test = bus.read_byte(0, 0x879A);
    cpu.a = in_self_test;
    cpu.and8(in_self_test);
    if (cpu.f & FLAG_Z) != 0 {
        cpu.set_hl(0x9800);
        cpu.set_de(0x8B80);
        cpu.set_bc(0x0080);
        while cpu.bc() != 0 {
            cpu.ldi(bus);
        }
        cpu.set_hl(0x9900);
        cpu.set_de(0x9B80);
        cpu.set_bc(0x0080);
        while cpu.bc() != 0 {
            cpu.ldi(bus);
        }

        let dip_cf = bus.read_byte(0, 0x87CF);
        cpu.a = dip_cf;
        cpu.cb_op(0x6F, dip_cf); // BIT 5, A (Freeze DIP)
        if (dip_cf & 0x20) == 0 {
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }

        let cab = (!bus.read_byte(0, 0x898E)) & 0x04;
        let f8400 = bus.read_byte(0, 0x8400) | cab;
        cpu.a = f8400;
        bus.write_byte(0, 0x8400, f8400);
        call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x02C0);
    }

    // $02C0: Increment 16-bit frame counter ($8423)
    let fcnt = bus.read_word(0, 0x8423).wrapping_add(1);
    cpu.set_hl(fcnt);
    bus.write_word(0, 0x8423, fcnt);

    if bus.read_byte(0, 0x879A) == 0 {
        let prev_cred = bus.read_byte(0, 0x85A5);
        bus.write_byte(0, 0x85A6, prev_cred);
    }

    // $02D3: Queue Namco 06XX command $71 (read 3 bytes from $7000 into $85A7..$85A9)
    cpu.set_hl(0x7000);
    cpu.set_de(0x85A7);
    cpu.set_bc(0x0003);
    cpu.a = 0x71;
    call_cpu_sub(cpu, bus, roms, 0x00A1, 0x02E1);

    if bus.read_byte(0, 0x879A) != 0 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    // $02E8: Process pending monster-pop / bonus score addition ($87A7 bit 0)
    let f87a7 = bus.read_byte(0, 0x87A7);
    if (f87a7 & 0x01) != 0 {
        bus.write_byte(0, 0x87A7, f87a7 & !0x01);
        bus.write_byte(0, 0x8411, 0x00);
        let pts = bus.read_byte(0, 0x87E7);
        bus.write_byte(0, 0x8412, pts);
        bus.write_byte(0, 0x8413, 0x00);
        call_cpu_sub(cpu, bus, roms, 0x2FC8, 0x0301);
        if (bus.read_byte(0, 0x8657) & 0x02) == 0 {
            call_cpu_sub(cpu, bus, roms, 0x146B, 0x030B);
            if (bus.read_byte(0, 0x8400) & 0x08) == 0 {
                call_cpu_sub(cpu, bus, roms, 0x0BDF, 0x0315);
            }
        }
    }

    // $0315: Select active player's controller byte ($85A8 or $85A9) -> $85B0 and $8624
    let f8400 = bus.read_byte(0, 0x8400);
    let raw_joy = if (f8400 & 0x02) != 0 && (f8400 & 0x04) != 0 {
        bus.read_byte(0, 0x85A9)
    } else {
        bus.read_byte(0, 0x85A8)
    };
    let mut b = raw_joy;
    if (raw_joy & 0x0F) >= 0x09 {
        b &= !0x08;
    }
    let joy_clean = b & !0x01;
    bus.write_byte(0, 0x85B0, joy_clean);
    bus.write_byte(0, 0x8624, joy_clean & 0x30);

    // $0346: Latch BCD credits ($85A7 -> $85A5) and check service switch / coin / start
    cpu.set_hl(0x85A5);
    cpu.set_de(0x85A6);
    let cred_new = bus.read_byte(0, 0x85A7);
    bus.write_byte(0, 0x85A5, cred_new);
    if cred_new >= 0xB0 || (cred_new & 0x0F) >= 0x0A {
        cpu.pc = 0x387E;
        return;
    }

    let cred_old = bus.read_byte(0, 0x85A6);
    if cred_new == 0 && cred_old == 0 {
        if (bus.read_byte(0, 0x8400) & 0x40) == 0 {
            decomp_vblank_attract_zero_credits_0a80(cpu, bus, roms);
            return;
        }
    } else {
        let cc = bus.read_byte(0, 0x87CC);
        if (cc & 0x03) == 0 && cred_new < 0x90 {
            cpu.a = cred_new;
            cpu.sub8(cred_old, false, true);
            cpu.daa();
            if (cpu.f & (FLAG_C | FLAG_Z)) == 0 {
                bus.write_byte(0, 0x9B33, cpu.a);
            }
        }
        cpu.set_de(0x85A5);
        cpu.set_hl(0x85A6);
        // $038B: Check if credits decreased (Start 1P / 2P pressed!)
        if cred_new < cred_old {
            let mut start_1p = cred_new == 0x99 || cred_new == 0x9F;
            if !start_1p {
                cpu.a = cred_new;
                cpu.add8(0x01, false);
                cpu.daa();
                if cpu.a == cred_old {
                    start_1p = true;
                }
            }
            let f8400 = bus.read_byte(0, 0x8400);
            if start_1p {
                bus.write_byte(0, 0x8400, (f8400 & !0x03) | 0x40);
                cpu.a = 0x01;
            } else {
                bus.write_byte(0, 0x8400, (f8400 & !0x02) | 0x41);
                cpu.a = 0x02;
            }
            cpu.set_hl(0x89E1);
            call_cpu_sub(cpu, bus, roms, 0x1226, 0x03B0);
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }
        if (bus.read_byte(0, 0x8400) & 0x40) == 0 {
            decomp_vblank_attract_with_credits_0b03(cpu, bus, roms);
            return;
        }
    }

    // $03C8: Active game in progress ($8400 bit 6 == 1)
    let f8657 = bus.read_byte(0, 0x8657) & !0x01;
    bus.write_byte(0, 0x8657, f8657);
    if (f8657 & 0x02) != 0 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    let b89f3 = bus.read_byte(0, 0x89F3).wrapping_add(1);
    bus.write_byte(0, 0x89F3, b89f3);
    if bus.read_byte(0, 0x9A88) != 0 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    let f8400 = bus.read_byte(0, 0x8400);
    if (f8400 & 0x08) == 0 {
        // $03E6: Stage 1 of New Game Initialization
        bus.write_byte(0, 0x8400, f8400 | 0x08);
        call_cpu_sub(cpu, bus, roms, 0x15A6, 0x03EB);
        call_cpu_sub(cpu, bus, roms, 0x177C, 0x03EE);
        call_cpu_sub(cpu, bus, roms, 0x17ED, 0x03F1);
        call_cpu_sub(cpu, bus, roms, 0x187F, 0x03F4);
        call_cpu_sub(cpu, bus, roms, 0x1892, 0x03F7);

        // Push $03FA and jump to $005A so `run_frame` sees `sub_cpu_reset` and drains NMIs!
        cpu.push(bus, 0x03FA);
        cpu.pc = 0x005A;
        return;
    }

    if (f8400 & 0x80) == 0 {
        // $0449: Stage 2 of Round Initialization
        bus.write_byte(0, 0x8400, f8400 | 0x80);
        call_cpu_sub(cpu, bus, roms, 0x17AE, 0x044E);
        call_cpu_sub(cpu, bus, roms, 0x0C15, 0x0451);
        call_cpu_sub(cpu, bus, roms, 0x1551, 0x0454);

        if (bus.read_byte(0, 0x8400) & 0x02) == 0 {
            bus.write_byte(0, 0x87DA, 0x00);
            bus.write_word(0, 0x87D4, 0x0000);
        } else {
            bus.write_byte(0, 0x87D8, 0x00);
            bus.write_word(0, 0x87D2, 0x0000);
        }
        bus.write_byte(0, 0x8799, 0x00);
        bus.write_byte(0, 0x87EB, 0x00);
        bus.write_word(0, 0x87ED, 0x0000);
        bus.write_byte(0, 0x896D, 0x00);
        bus.write_byte(0, 0x896C, 0x00);
        bus.write_byte(0, 0x87F0, 0x00);
        bus.write_byte(0, 0x989E, 0x00);
        bus.write_byte(0, 0x989F, 0xE0);

        call_cpu_sub(cpu, bus, roms, 0x0BA6, 0x048A);
        call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x048D);
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    if (f8400 & 0x10) == 0 {
        // $0497: Stage 3 of Round Initialization
        bus.write_byte(0, 0x8400, f8400 | 0x10);
        call_cpu_sub(cpu, bus, roms, 0x19F7, 0x049C);
        let mut r = bus.read_byte(0, 0x8627).wrapping_sub(1);
        while r >= 0x0C {
            r = r.wrapping_sub(0x0C);
        }
        let bank = r >> 2;
        bus.write_byte(0, 0xA004, bank & 0x01);
        bus.write_byte(0, 0xA005, (bank >> 1) & 0x01);

        call_cpu_sub(cpu, bus, roms, 0x0C05, 0x04B8);
        call_cpu_sub(cpu, bus, roms, 0x140B, 0x04BB);
        call_cpu_sub(cpu, bus, roms, 0x1845, 0x04BE);
        call_cpu_sub(cpu, bus, roms, 0x1551, 0x04C1);
        call_cpu_sub(cpu, bus, roms, 0x0BA6, 0x04C4);
        call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x04C7);
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    // $04CA: Stage 4 / Active Round Loop
    cpu.push(bus, 0x8400);
    if (bus.read_byte(0, 0x8657) & 0x02) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x0C81, 0x04D5);
        call_cpu_sub(cpu, bus, roms, 0x146B, 0x04D8);
    }
    let _ = cpu.pop(bus);

    let f8400_now = bus.read_byte(0, 0x8400);
    if (f8400_now & 0x20) == 0 {
        // $04DE: Finalize player entry and spawn monsters/rocks
        bus.write_byte(0, 0x8400, f8400_now | 0x20);
        decomp_vblank_spawn_round_entities_04e0(cpu, bus, roms);
        return;
    }

    // $05C7: Ready prompt countdown & active gameplay
    if (bus.read_byte(0, 0x896C) & 0x08) == 0 {
        let f8401 = bus.read_byte(0, 0x8401);
        if (f8401 & 0x20) == 0 {
            cpu.push(bus, 0x8401);
            call_cpu_sub(cpu, bus, roms, 0x1C94, 0x05D9);
            bus.write_byte(0, 0x85D5, 0x78);
            let _ = cpu.pop(bus);
            bus_or(bus, 0x8401, 0x20);
            let f8785 = bus.read_byte(0, 0x8785);
            let played_bit = if (bus.read_byte(0, 0x8400) & 0x02) == 0 { 0x01 } else { 0x02 };
            if (f8785 & played_bit) == 0 {
                bus.write_byte(0, 0x9A81, 0x01);
            }
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }

        if bus.read_byte(0, 0x9A81) != 0 {
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }

        let d5 = bus.read_byte(0, 0x85D5);
        if d5 != 0 {
            if d5 == 0x01 {
                call_cpu_sub(cpu, bus, roms, 0x1CD3, 0x0618);
            }
            bus.write_byte(0, 0x85D5, d5.wrapping_sub(1));
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }

        let f8785 = bus.read_byte(0, 0x8785);
        let bit = if (bus.read_byte(0, 0x8400) & 0x02) == 0 { 0x01 } else { 0x02 };
        bus.write_byte(0, 0x8785, f8785 | bit);
    }

    // $062F: Active round tick
    bus_or(bus, 0x896C, 0x08);
    if (bus.read_byte(0, 0x8646) & 0x02) != 0 {
        bus.write_byte(0, 0x9A86, 0x00);
        bus.write_byte(0, 0x9A89, 0x00);
        let t47 = bus.read_byte(0, 0x8647);
        if t47 == 0 {
            bus_or(bus, 0x8401, 0x80);
        } else {
            bus.write_byte(0, 0x8647, t47.wrapping_sub(1));
        }
    }

    decomp_vblank_active_gameplay_0654(cpu, bus, roms);
}

#[inline]
fn bus_or<B: Z80Bus>(bus: &mut B, addr: u16, mask: u8) {
    let v = bus.read_byte(0, addr) | mask;
    bus.write_byte(0, addr, v);
}

#[inline]
fn bus_and<B: Z80Bus>(bus: &mut B, addr: u16, mask: u8) {
    let v = bus.read_byte(0, addr) & mask;
    bus.write_byte(0, addr, v);
}

/// $03FA..$0442: Continuation of VBlank Stage 1 after `CALL $005A` returns to `0x03FA`.
pub fn decomp_vblank_resume_after_005a_03fa<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    if (bus.read_byte(0, 0x85B0) & 0x20) != 0 {
        bus.write_byte(0, 0x8787, 0xFF);
    }
    call_cpu_sub(cpu, bus, roms, 0x0C22, 0x0409);

    let dip_cf = bus.read_byte(0, 0x87CF);
    let f8787 = bus.read_byte(0, 0x8787);
    let r = if (dip_cf & 0x08) == 0 && (f8787 & 0x02) == 0 {
        bus.read_byte(0, 0x840E)
    } else {
        0x01
    };
    bus.write_byte(0, 0x840F, r);
    bus.write_word(0, 0x8627, ((r as u16) << 8) | (r as u16));
    bus.write_byte(0, 0x8785, 0x00);

    call_cpu_sub(cpu, bus, roms, 0x18AC, 0x043C);
    call_cpu_sub(cpu, bus, roms, 0x12B1, 0x043F);
    call_cpu_sub(cpu, bus, roms, 0x0C51, 0x0442);
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

/// $04E0..$05C6: Spawns round monsters/rocks and initializes timers (also jumped to from $0AF8 in attract mode).
fn decomp_vblank_spawn_round_entities_04e0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    bus.write_word(0, 0x8625, 0x1068);
    bus.write_byte(0, 0x864A, 0x00);
    bus.write_byte(0, 0x8653, 0x00);
    call_cpu_sub(cpu, bus, roms, 0x19F7, 0x04F0);
    call_cpu_sub(cpu, bus, roms, 0x1551, 0x04F3);

    let mut step_c = 0x02u8;
    if (bus.read_byte(0, 0x8657) & 0x01) == 0 {
        let r = bus.read_byte(0, 0x8627);
        if r >= 0x20 {
            step_c = 0x00;
        } else if r >= 0x14 {
            step_c = 0x01;
        }
    }
    let mut val = 0x02u8;
    for i in 0..8u16 {
        bus.write_byte(0, 0x864B + i, val);
        val = val.wrapping_sub(step_c);
    }

    call_cpu_sub(cpu, bus, roms, 0x11EC, 0x051C);
    call_cpu_sub(cpu, bus, roms, 0x11E0, 0x051F);

    bus.write_byte(0, 0x83C1, 0x7F);
    bus.write_byte(0, 0x83C2, 0x7F);
    bus.write_byte(0, 0x83E2, 0x7F);
    bus.write_byte(0, 0x83E1, 0x7F);

    bus.write_byte(0, 0x989E, 0x00);
    bus.write_byte(0, 0x989F, 0xE0);
    bus.write_byte(0, 0x87EF, 0x00);
    bus.write_byte(0, 0x87F0, 0x00);
    bus.write_word(0, 0x87D6, 0x0000);
    bus.write_byte(0, 0x896F, 0x00);
    bus.write_byte(0, 0x896C, 0x00);
    bus.write_byte(0, 0x87A7, 0x00);
    bus.write_byte(0, 0x85B3, 0x00);
    bus.write_byte(0, 0x9922, 0x00);

    let mut inactive_count = 0u8;
    for i in 0..8u16 {
        if (bus.read_byte(0, 0x8528 + i * 0x10) & 0x80) != 0 {
            inactive_count += 1;
        }
    }
    if inactive_count == 8 {
        call_cpu_sub(cpu, bus, roms, 0x1B7D, 0x056C);
        bus_or(bus, 0x896C, 0x08);
        bus_or(bus, 0x8401, 0x20);
    } else {
        call_cpu_sub(cpu, bus, roms, 0x1B2E, 0x057B);
        call_cpu_sub(cpu, bus, roms, 0x1B7D, 0x057E);
        call_cpu_sub(cpu, bus, roms, 0x1845, 0x0581);
        bus_and(bus, 0x8401, !0x20);
    }

    bus.write_byte(0, 0x9A93, 0x00);
    bus.write_byte(0, 0x8787, 0xFF);
    call_cpu_sub(cpu, bus, roms, 0x0BA6, 0x0593);
    call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x0596);
    bus.write_byte(0, 0x85AE, 0x02);
    bus.write_byte(0, 0x85AF, 0x02);
    bus.write_word(0, 0x85F2, 0x01E0);
    bus_and(bus, 0x8401, !0x40);
    bus.write_byte(0, 0x6830, 0x02);
    bus.write_word(0, 0x85F4, 0x0000);
    if (bus.read_byte(0, 0x8657) & 0x02) != 0 {
        bus.write_byte(0, 0x8676, 0x80);
    }
    call_cpu_sub(cpu, bus, roms, 0x0BA6, 0x05C1);
    call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x05C4);
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

/// $0654..$0A26: Active gameplay tick (death sequence, player switching, game over, and frame updates).
fn decomp_vblank_active_gameplay_0654<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let f896f = bus.read_byte(0, 0x896F);
    if (f896f & 0x01) == 0 {
        bus.write_byte(0, 0x896F, f896f | 0x01);
        bus.write_word(0, 0x8423, 0x0000);
        cpu.xor8(cpu.a);
        cpu.set_hl(0x85B0);
        call_cpu_sub(cpu, bus, roms, 0x1964, 0x066A);
        bus.write_byte(0, 0x85F4, cpu.a);
        bus.write_byte(0, 0x85F5, cpu.a);
    }

    if (bus.read_byte(0, 0x87F0) & 0x02) == 0 && (bus.read_byte(0, 0x8401) & 0x80) != 0 {
        // $0680: Player death animation sequence
        bus.write_byte(0, 0x9A92, 0x00);
        if (bus.read_byte(0, 0x8402) & 0x01) == 0 {
            let t1c = bus.read_byte(0, 0x861C).wrapping_sub(1);
            bus.write_byte(0, 0x861C, t1c);
            if t1c == 0x64 {
                if (bus.read_byte(0, 0x8646) & 0x02) != 0 {
                    bus.write_byte(0, 0x8646, 0x80);
                }
                for i in 0..8u16 {
                    bus.write_byte(0, 0x8532 + i * 0x10, 0x00);
                    let m26 = bus.read_byte(0, 0x8526 + i * 0x10) & !0x40;
                    bus.write_byte(0, 0x8526 + i * 0x10, m26);
                    let m28 = bus.read_byte(0, 0x8528 + i * 0x10) & !0x40;
                    bus.write_byte(0, 0x8528 + i * 0x10, m28);
                    bus.write_byte(0, 0x852B + i * 0x10, 0x32);
                }
                bus.write_byte(0, 0x989E, 0x00);
                bus.write_byte(0, 0x989F, 0xE0);
                bus.write_byte(0, 0x864A, 0x00);
                call_cpu_sub(cpu, bus, roms, 0x1551, 0x06E1);
                call_cpu_sub(cpu, bus, roms, 0x0F89, 0x06E4);
                bus.write_byte(0, 0x98A0, 0x00);
                bus.write_byte(0, 0x98A1, 0x00);
                for addr in 0x98F4..0x9900u16 {
                    bus.write_byte(0, addr, 0x00);
                }
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }

            let check_77c;
            if t1c == 0x50 {
                bus.write_byte(0, 0x9A85, 0x01);
                bus.write_byte(0, 0x9822, 0x14);
                let f46 = bus.read_byte(0, 0x8646);
                if (f46 & 0x80) != 0 {
                    bus.write_byte(0, 0x8646, f46 & !0x80);
                    bus.write_byte(0, 0x861C, 0x22);
                } else {
                    call_cpu_sub(cpu, bus, roms, 0x11DA, 0x0719);
                }
                check_77c = true;
            } else if t1c == 0x34 {
                bus.write_byte(0, 0x9822, 0x15);
                let addr = if (bus.read_byte(0, 0x8400) & 0x02) == 0 { 0x87DA } else { 0x87D8 };
                let v = bus.read_byte(0, addr);
                if v == 0x02 {
                    bus.write_byte(0, addr, 0x03);
                }
                check_77c = true;
            } else if t1c == 0x20 {
                bus.write_byte(0, 0x9822, 0x16);
                check_77c = true;
            } else if t1c == 0x14 {
                bus.write_byte(0, 0x9822, 0x17);
                check_77c = true;
            } else if t1c != 0 {
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            } else if (bus.read_byte(0, 0x8657) & 0x02) != 0 {
                decomp_vblank_game_over_cleanup_08f8(cpu, bus, roms);
                return;
            } else {
                bus.write_byte(0, 0x9880, 0x00);
                bus.write_byte(0, 0x9881, 0x00);
                bus.write_byte(0, 0x9822, 0x32);
                bus.write_byte(0, 0x98A2, 0x00);
                bus.write_byte(0, 0x98A3, 0x50);
                bus_or(bus, 0x8402, 0x01);
                bus.write_byte(0, 0x861C, 0x3C);
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }

            if check_77c {
                if (bus.read_byte(0, 0x85AE) & 0x02) != 0 {
                    let s22 = bus.read_byte(0, 0x9822).wrapping_add(0x04);
                    bus.write_byte(0, 0x9822, s22);
                    for addr in 0x98F4..0x9900u16 {
                        bus.write_byte(0, addr, 0x00);
                    }
                }
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
        }

        // $0798: Post-death delay and life/player switch check
        let t1c = bus.read_byte(0, 0x861C);
        if t1c != 0 {
            bus.write_byte(0, 0x861C, t1c.wrapping_sub(1));
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }

        let f8400 = bus.read_byte(0, 0x8400);
        if (f8400 & 0x02) == 0 {
            // Player 1 just died
            if bus.read_byte(0, 0x840A) != 0 {
                if (f8400 & 0x01) != 0 && (bus.read_byte(0, 0x8402) & 0x04) == 0 {
                    decomp_vblank_switch_to_p2_07ee(cpu, bus, roms);
                } else {
                    decomp_vblank_continue_next_life_07c7(cpu, bus, roms);
                }
                return;
            }
            bus_or(bus, 0x8402, 0x02);
            // $081F: Player 1 out of lives
            if (f8400 & 0x01) == 0 || (bus.read_byte(0, 0x8402) & 0x04) != 0 {
                decomp_vblank_final_game_over_08b0(cpu, bus, roms);
                return;
            }
            let f8402 = bus.read_byte(0, 0x8402);
            if (f8402 & 0x08) == 0 {
                bus.write_byte(0, 0x861D, 0x78);
                call_cpu_sub(cpu, bus, roms, 0x12C9, 0x083D);
                bus_or(bus, 0x8402, 0x08);
                bus_or(bus, 0x87CC, 0x06);
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            let t1d = bus.read_byte(0, 0x861D);
            if t1d != 0 {
                bus.write_byte(0, 0x861D, t1d.wrapping_sub(1));
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            let cc = bus.read_byte(0, 0x87CC) & !0x04;
            bus.write_byte(0, 0x87CC, cc);
            if (cc & 0x02) != 0 {
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            call_cpu_sub(cpu, bus, roms, 0x134B, 0x0864);
            decomp_vblank_switch_to_p2_07ee(cpu, bus, roms);
            decomp_vblank_continue_next_life_07c7(cpu, bus, roms);
            return;
        } else {
            // Player 2 just died
            if bus.read_byte(0, 0x840B) != 0 {
                if (bus.read_byte(0, 0x8402) & 0x02) == 0 {
                    bus_and(bus, 0x8400, !0x02);
                    bus.write_byte(0, 0xA007, 0x00);
                    call_cpu_sub(cpu, bus, roms, 0x1297, 0x0805);
                    call_cpu_sub(cpu, bus, roms, 0x12A4, 0x0808);
                    call_cpu_sub(cpu, bus, roms, 0x12B1, 0x080B);
                }
                decomp_vblank_continue_next_life_07c7(cpu, bus, roms);
                return;
            }
            bus_or(bus, 0x8402, 0x04);
            // $0866: Player 2 out of lives
            let f8402 = bus.read_byte(0, 0x8402);
            if (f8402 & 0x02) != 0 {
                decomp_vblank_final_game_over_08b0(cpu, bus, roms);
                return;
            }
            if (f8402 & 0x08) == 0 {
                bus.write_byte(0, 0x861D, 0x78);
                call_cpu_sub(cpu, bus, roms, 0x12D3, 0x087C);
                bus_or(bus, 0x8402, 0x08);
                bus_or(bus, 0x87CC, 0x06);
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            let t1d = bus.read_byte(0, 0x861D);
            if t1d != 0 {
                bus.write_byte(0, 0x861D, t1d.wrapping_sub(1));
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            let cc = bus.read_byte(0, 0x87CC) & !0x04;
            bus.write_byte(0, 0x87CC, cc);
            if (cc & 0x02) != 0 {
                decomp_vblank_tail_0a27(cpu, bus, roms);
                return;
            }
            bus_and(bus, 0x8400, !0x02);
            call_cpu_sub(cpu, bus, roms, 0x134B, 0x08A8);
            bus.write_byte(0, 0xA007, 0x00);
            call_cpu_sub(cpu, bus, roms, 0x1297, 0x0805);
            call_cpu_sub(cpu, bus, roms, 0x12A4, 0x0808);
            call_cpu_sub(cpu, bus, roms, 0x12B1, 0x080B);
            decomp_vblank_continue_next_life_07c7(cpu, bus, roms);
            return;
        }
    }

    // $0949: Check if all monsters & rocks have settled for round-clear
    let s22 = bus.read_byte(0, 0x9822);
    let mut still_busy = bus.read_byte(0, 0x98F5) != 0
        || bus.read_byte(0, 0x98F7) != 0
        || s22 == 0x08
        || s22 == 0x09
        || s22 == 0x0C
        || s22 == 0x0D
        || bus.read_byte(0, 0x9A87) != 0;

    if !still_busy && (bus.read_byte(0, 0x8657) & 0x02) == 0 {
        for i in 0..8u16 {
            let b = bus.read_byte(0, 0x852B + i * 0x10);
            if b == 0x82 || b == 0x85 {
                still_busy = true;
                break;
            }
        }
    }

    if !still_busy {
        for i in 0..8u16 {
            let addr = 0x8528 + i * 0x10;
            let v = bus.read_byte(0, addr);
            if (v & 0x80) == 0 {
                still_busy = true;
                break;
            }
            bus.write_byte(0, addr, v & !0x40);
        }
    }

    if !still_busy && (bus.read_byte(0, 0x8646) & 0x01) == 0 {
        // $09AB: Round Cleared!
        call_cpu_sub(cpu, bus, roms, 0x128C, 0x09AE);
        bus.write_byte(0, 0x98A0, 0x00);
        bus.write_byte(0, 0x98A1, 0x00);
        call_cpu_sub(cpu, bus, roms, 0x0F89, 0x09B8);
        bus.write_byte(0, 0x8646, 0x00);
        bus_or(bus, 0x9A88, 0x01);
        let f8400 = bus.read_byte(0, 0x8400) & 0x4F;
        bus.write_byte(0, 0x8400, f8400);
        bus_and(bus, 0x8401, !0x28);
        if (f8400 & 0x02) == 0 {
            let l1 = bus.read_byte(0, 0x840A).wrapping_add(1);
            bus.write_byte(0, 0x840A, l1);
            bus.write_byte(0, 0x864A, 0x00);
        } else {
            let l2 = bus.read_byte(0, 0x840B).wrapping_add(1);
            bus.write_byte(0, 0x840B, l2);
        }
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    // $09E6: Normal active frame update (monsters, rocks, bonus veg, harpoon, collisions)
    call_cpu_sub(cpu, bus, roms, 0x0BA6, 0x09E9);
    call_cpu_sub(cpu, bus, roms, 0x0BB1, 0x09EC);
    let d6 = bus.read_word(0, 0x87D6).wrapping_add(1);
    bus.write_word(0, 0x87D6, d6);
    call_cpu_sub(cpu, bus, roms, 0x1432, 0x09F6);
    if (bus.read_byte(0, 0x8657) & 0x02) == 0 {
        let p1_lo = bus.read_byte(0, 0x840D);
        bus.write_byte(0, 0x9A05, p1_lo);
    }
    call_cpu_sub(cpu, bus, roms, 0x1F00, 0x0A06);
    call_cpu_sub(cpu, bus, roms, 0x1425, 0x0A09);

    if (bus.read_byte(0, 0x8646) & 0x01) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x0F89, 0x0A13);
    } else {
        call_cpu_sub(cpu, bus, roms, 0x0DA2, 0x0A18);
        bus.write_byte(0, 0x6823, 0xFF);
    }

    if (bus.read_byte(0, 0x8646) & 0x01) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x134E, 0x0A27);
    }
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

fn decomp_vblank_switch_to_p2_07ee<B: Z80Bus>(cpu: &mut Z80, bus: &mut B, roms: &DigDugRoms) {
    let f8400 = bus.read_byte(0, 0x8400);
    bus.write_byte(0, 0x6830, 0x00);
    if (f8400 & 0x04) != 0 {
        bus.write_byte(0, 0xA007, 0x01);
    }
    bus_or(bus, 0x8400, 0x02);
    call_cpu_sub(cpu, bus, roms, 0x1297, 0x0805);
    call_cpu_sub(cpu, bus, roms, 0x12A4, 0x0808);
    call_cpu_sub(cpu, bus, roms, 0x12B1, 0x080B);
}

fn decomp_vblank_continue_next_life_07c7<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    bus_and(bus, 0x8400, !0x30);
    bus_and(bus, 0x8401, 0x57);
    bus_and(bus, 0x8402, 0xE6);
    bus_and(bus, 0x8403, !0x0C);
    bus_and(bus, 0x8404, !0x40);
    bus.write_byte(0, 0x8646, 0x00);
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

fn decomp_vblank_final_game_over_08b0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let f8402 = bus.read_byte(0, 0x8402);
    if (f8402 & 0x10) == 0 {
        bus.write_byte(0, 0x8402, f8402 | 0x10);
        bus_or(bus, 0x9A83, 0x01);
        bus.write_byte(0, 0x861E, 0x41);
        bus_or(bus, 0x87CC, 0x06);
        bus.write_byte(0, 0x8658, 0x00);
        bus.write_byte(0, 0x8657, 0x00);
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }
    let t1e = bus.read_byte(0, 0x861E);
    if t1e != 0 {
        if t1e == 0x38 && bus.read_byte(0, 0x9A83) != 0 {
            decomp_vblank_tail_0a27(cpu, bus, roms);
            return;
        }
        cpu.push(bus, 0x861E);
        call_cpu_sub(cpu, bus, roms, 0x12CF, 0x08ED);
        let _ = cpu.pop(bus);
        bus.write_byte(0, 0x861E, t1e.wrapping_sub(1));
        call_cpu_sub(cpu, bus, roms, 0x188D, 0x08F2);
        call_cpu_sub(cpu, bus, roms, 0x18A7, 0x08F5);
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }
    decomp_vblank_game_over_cleanup_08f8(cpu, bus, roms);
}

fn decomp_vblank_game_over_cleanup_08f8<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let cc = bus.read_byte(0, 0x87CC) & !0x04;
    bus.write_byte(0, 0x87CC, cc);
    if (cc & 0x02) != 0 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }
    call_cpu_sub(cpu, bus, roms, 0x134B, 0x0905);
    bus_and(bus, 0x8400, 0x01);
    for addr in 0x8401..0x8407u16 {
        bus.write_byte(0, addr, 0x00);
    }
    bus.write_byte(0, 0x8676, 0x90);
    bus.write_byte(0, 0x8646, 0x00);
    let p1_lo = bus.read_byte(0, 0x840D);
    bus.write_byte(0, 0x87A9, p1_lo);
    for addr in 0x8904..0x8908u16 {
        bus.write_byte(0, addr, 0x02);
    }
    cpu.set_hl(0x8904);
    cpu.set_de(0x7000);
    cpu.set_bc(0x0004);
    cpu.a = 0xC1;
    call_cpu_sub(cpu, bus, roms, 0x00A1, 0x093C);
    bus.write_byte(0, 0x8787, 0x00);
    bus.write_byte(0, 0xA007, 0x00);
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

/// $0A27..$0A7F: Tail of VBlank handler (queues DIP read $D2, updates player/harpoon/HUD, pops regs & RETs).
pub fn decomp_vblank_tail_0a27<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x7000);
    cpu.set_de(0x87CE);
    cpu.set_bc(0x0002);
    cpu.a = 0xD2;
    call_cpu_sub(cpu, bus, roms, 0x00A1, 0x0A35);
    bus.write_byte(0, 0x6830, cpu.a);

    let in_self_test = bus.read_byte(0, 0x879A);
    cpu.a = in_self_test;
    cpu.and8(in_self_test);
    if (cpu.f & FLAG_Z) == 0 {
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }

    let f8657 = bus.read_byte(0, 0x8657);
    cpu.a = f8657;
    if (f8657 & 0x01) != 0 {
        let s58 = bus.read_byte(0, 0x8658) & 0x0F;
        cpu.a = s58;
        if s58 != 0x07 {
            decomp_vblank_epilogue_0a71(cpu, bus);
            return;
        }
    }

    call_cpu_sub(cpu, bus, roms, 0x1D0D, 0x0A52);
    for i in 0..12u16 {
        bus.write_byte(0, 0x9825 + i * 2, 0x05);
    }
    bus.write_byte(0, 0x6830, cpu.a);

    let cc = bus.read_byte(0, 0x87CC);
    cpu.a = cc;
    if (cc & 0x02) == 0 {
        call_cpu_sub(cpu, bus, roms, 0x1B92, 0x0A6B);
        call_cpu_sub(cpu, bus, roms, 0x1BAF, 0x0A6E);
        call_cpu_sub(cpu, bus, roms, 0x16E5, 0x0A71);
    }
    decomp_vblank_epilogue_0a71(cpu, bus);
}

/// $0A80..$0B02: Attract mode handler when credits == 0.
fn decomp_vblank_attract_zero_credits_0a80<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x8787);
    if (bus.read_byte(0, 0x8657) & 0x01) == 0
        && (bus.read_byte(0, 0x87CF) & 0x08) == 0
        && (bus.read_byte(0, 0x8787) & 0x02) == 0
    {
        jp_cpu_sub(cpu, bus, roms, 0x1DEC);
        return;
    }

    bus.write_byte(0, 0x8787, 0xFF);
    let f8657 = bus.read_byte(0, 0x8657) | 0x01;
    bus.write_byte(0, 0x8657, f8657);
    if (f8657 & 0x02) == 0 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    call_cpu_sub(cpu, bus, roms, 0x0BCA, 0x0AA5);
    if bus.read_byte(0, 0x8658) != 0x17 {
        decomp_vblank_tail_0a27(cpu, bus, roms);
        return;
    }

    let st76 = bus.read_byte(0, 0x8676);
    if st76 == 0x10 {
        bus.write_byte(0, 0x8799, 0x00);
        bus.write_byte(0, 0x896C, 0x00);
        bus.write_byte(0, 0x87F0, 0x00);
        call_cpu_sub(cpu, bus, roms, 0x0C22, 0x0AC2);
        call_cpu_sub(cpu, bus, roms, 0x0C15, 0x0AC5);
        bus.write_word(0, 0x87D6, 0x0000);
        bus.write_byte(0, 0x989E, 0x00);
        bus.write_byte(0, 0x989F, 0xE0);
        call_cpu_sub(cpu, bus, roms, 0x0C05, 0x0AD6);
        bus.write_byte(0, 0x8423, 0xFE);
        call_cpu_sub(cpu, bus, roms, 0x146B, 0x0ADE);
        bus.write_byte(0, 0x8423, 0x01);
        call_cpu_sub(cpu, bus, roms, 0x146B, 0x0AE6);
        for addr in 0x83C0..0x83FFu16 {
            if bus.read_byte(0, addr) == 0x0C {
                bus.write_byte(0, addr, 0x8C);
            }
        }
        decomp_vblank_spawn_round_entities_04e0(cpu, bus, roms);
        return;
    }
    if st76 == 0x80 {
        decomp_vblank_active_gameplay_0654(cpu, bus, roms);
        return;
    }
    decomp_vblank_tail_0a27(cpu, bus, roms);
}

/// $0B03..$0BA5: Attract / Push-Start screen handler when credits > 0.
fn decomp_vblank_attract_with_credits_0b03<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    cpu.set_hl(0x8787);
    if (bus.read_byte(0, 0x87CF) & 0x08) == 0 && (bus.read_byte(0, 0x8787) & 0x02) == 0 {
        jp_cpu_sub(cpu, bus, roms, 0x1DEC);
        return;
    }
    bus.write_byte(0, 0x8787, 0xFF);
    call_cpu_sub(cpu, bus, roms, 0x199B, 0x0B17);
    bus_and(bus, 0x87F0, !0x08);
    bus.write_byte(0, 0x9A93, 0x00);
    let f8657 = bus.read_byte(0, 0x8657) & !0x01;
    bus.write_byte(0, 0x8657, f8657);
    if (f8657 & 0x02) != 0 {
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }
    call_cpu_sub(cpu, bus, roms, 0x0BCA, 0x0B2E);
    let f8401 = bus.read_byte(0, 0x8401);
    if (f8401 & 0x02) != 0 {
        call_cpu_sub(cpu, bus, roms, 0x188D, 0x0B39);
        call_cpu_sub(cpu, bus, roms, 0x18A7, 0x0B3C);
        call_cpu_sub(cpu, bus, roms, 0x0BF2, 0x0B3F);
        bus_and(bus, 0x8400, 0x07);
        bus_and(bus, 0x8401, 0x02);
        for i in 0..3u16 {
            bus.write_byte(0, 0x8402 + i, 0x00);
        }
        call_cpu_sub(cpu, bus, roms, 0x1606, 0x0B56);
        call_cpu_sub(cpu, bus, roms, 0x15DB, 0x0B59);
        call_cpu_sub(cpu, bus, roms, 0x1690, 0x0B5C);
        decomp_vblank_epilogue_0a71(cpu, bus);
        return;
    }

    bus.write_byte(0, 0x8401, f8401 | 0x02);
    for i in 0..0x80u16 {
        bus.write_byte(0, 0x9880 + i, 0x00);
        bus.write_byte(0, 0x9900 + i, 0x00);
        bus.write_byte(0, 0x9800 + i, 0x32);
    }
    bus.write_byte(0, 0x8423, 0xFE);
    call_cpu_sub(cpu, bus, roms, 0x146B, 0x0B8A);
    bus.write_byte(0, 0x8423, 0x01);
    call_cpu_sub(cpu, bus, roms, 0x146B, 0x0B92);
    call_cpu_sub(cpu, bus, roms, 0x0BDF, 0x0B95);
    call_cpu_sub(cpu, bus, roms, 0x1597, 0x0B98);
    call_cpu_sub(cpu, bus, roms, 0x1625, 0x0B9B);
    call_cpu_sub(cpu, bus, roms, 0x1640, 0x0B9E);
    bus_or(bus, 0xA003, 0x01);
    decomp_vblank_epilogue_0a71(cpu, bus);
}

/// $3482: Locks in the currently selected initial letter and advances to the next slot (or finishes after 3 letters).
fn decomp_highscore_lock_initial_3482<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) {
    let vram_ptr = bus.read_word(0, 0x89C9);
    let ch = bus.read_byte(0, vram_ptr) & !0x80;
    bus.write_byte(0, vram_ptr, ch);
    cpu.c = ch;
    cpu.set_de(vram_ptr);
    call_cpu_sub(cpu, bus, roms, 0x36E9, 0x348C);
    bus.write_word(0, 0x89C9, cpu.de());
    bus.write_byte(0, 0x89CD, 0x28);

    let idx = bus.read_byte(0, 0x89C6);
    let next_idx = idx.wrapping_add(1);
    bus.write_byte(0, 0x89C6, next_idx);

    let rank = bus.read_byte(0, 0x89C5);
    cpu.a = rank;
    cpu.set_hl(0x352F);
    inline_rst_08(cpu);
    let lo = bus.read_byte(0, cpu.hl());
    let hi = bus.read_byte(0, cpu.hl().wrapping_add(1));
    let initials_base = ((hi as u16) << 8) | (lo as u16);
    bus.write_byte(0, initials_base.wrapping_add(idx as u16), ch);

    if next_idx != 0x03 {
        call_cpu_sub(cpu, bus, roms, 0x35E7, 0x33EB);
        cpu.pc = 0x33EB;
        return;
    }

    // All 3 initials entered -> transition to $34B3 completion sequence!
    cpu.sp = bus.read_word(0, 0x8985);
    call_cpu_sub(cpu, bus, roms, 0x35E7, 0x34BA);
    if bus.read_byte(0, 0x85A7) != bus.read_byte(0, 0x8987) {
        if bus.read_byte(0, 0x9A94) == 0 {
            bus.write_byte(0, 0x9B3D, 0x02);
            cpu.pc = 0x34D2;
            return;
        }
        bus.write_byte(0, 0x9A94, 0x01);
        cpu.pc = 0x3510;
        return;
    }
    bus.write_byte(0, 0x9A94, 0x01);
    cpu.b = 0x04;
    cpu.push(bus, 0x34C7);
    cpu.pc = 0x34DD;
}

/// $3519: Blinks current initial cursor and decrements initial entry timeout ($89CD/$89CE).
fn decomp_highscore_blink_and_timeout_3519<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
) -> bool {
    let vram_ptr = bus.read_word(0, 0x89C9);
    let ch = bus.read_byte(0, vram_ptr) ^ 0x80;
    bus.write_byte(0, vram_ptr, ch);
    if (bus.read_byte(0, 0x8423) & 0x1F) != 0 {
        return false;
    }
    let cd = bus.read_byte(0, 0x89CD).wrapping_sub(1);
    bus.write_byte(0, 0x89CD, cd);
    if cd != 0 {
        let ce = bus.read_byte(0, 0x89CE).wrapping_sub(1);
        bus.write_byte(0, 0x89CE, ce);
        if ce != 0 {
            return false;
        }
    }
    // Timeout expired: auto-lock remaining initials
    while bus.read_byte(0, 0x89C6) < 3 {
        decomp_highscore_lock_initial_3482(cpu, bus, roms);
        if cpu.pc == 0x34DD || cpu.pc == 0x34D2 || cpu.pc == 0x3510 {
            return true;
        }
    }
    true
}

/// Executes CPU 0 (Main Z80) 100% in Human Rust without `src/recompiled_cpu0.rs`.
pub fn step_human_cpu0<B: Z80Bus>(
    cpu: &mut Z80,
    bus: &mut B,
    roms: &DigDugRoms,
    stats: &mut DecompStats,
) -> u32 {
    if cpu.halted {
        cpu.total_cycles += 4;
        return 4;
    }

    if try_run_decompiled_subroutine(cpu, bus, roms, stats) {
        flush_nested_stats(stats);
        return 16;
    }
    flush_nested_stats(stats);

    match cpu.pc {
        // $0000..$0005: Reset vector (DI; IM 1; JP $00E6)
        0x0000..=0x0005 => {
            cpu.iff1 = false;
            cpu.iff2 = false;
            cpu.im = 1;
            cpu.pc = 0x00E6;
            cpu.total_cycles += 22;
            22
        }

        // $0028..$0037: Unused RST padding ($FF -> RST $38)
        0x0028..=0x0037 => {
            let ret = cpu.pc.wrapping_add(1);
            cpu.push(bus, ret);
            cpu.pc = 0x0038;
            cpu.total_cycles += 11;
            11
        }

        // $0038: IM 1 VBlank Interrupt Vector (JP $0280)
        0x0038 => {
            cpu.pc = 0x0280;
            cpu.total_cycles += 10;
            10
        }

        // $005A..$0065: Watchdog delay loop (stepped so Namco 06XX NMIs can interrupt it)
        0x005A => {
            cpu.set_hl(0x6000);
            cpu.pc = 0x005D;
            cpu.total_cycles += 10;
            10
        }
        0x005D..=0x0065 => {
            bus.write_byte(0, 0x6830, cpu.a);
            let hl = cpu.hl().saturating_sub(16);
            cpu.set_hl(hl);
            cpu.a = cpu.l;
            cpu.or8(cpu.h);
            if hl != 0 {
                cpu.pc = 0x005D;
                cpu.total_cycles += 35;
                35
            } else {
                cpu.ret_subroutine(bus);
                cpu.total_cycles += 40;
                40
            }
        }

        // $00E6..$011B: Power-on hardware latch init -> jumps to self-test at $387E
        0x00E6..=0x011B => {
            cpu.a = 0x10;
            bus.write_byte(0, 0x7100, 0x10);
            bus.write_byte(0, 0x7000, 0x10);
            bus.write_byte(0, 0x6820, 0x10);
            bus.write_byte(0, 0x6821, 0x10);
            bus.write_byte(0, 0x6822, 0x01);
            bus.write_byte(0, 0x6823, 0x10);
            bus.write_byte(0, 0x6830, 0x11);
            bus.write_byte(0, 0x6825, 0xFF);
            bus.write_byte(0, 0x6826, 0xFF);
            bus.write_byte(0, 0x6827, 0xFF);
            for addr in 0x89E0..0x89EBu16 {
                bus.write_byte(0, addr, 0x00);
            }
            for addr in 0x89EF..0x89F3u16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.xor8(cpu.a);
            cpu.set_hl(0x89F3);
            cpu.b = 0x00;
            cpu.pc = 0x387E;
            cpu.total_cycles += 320;
            320
        }

        // $011C..$01B9: Post-self-test work RAM initialization & DIP bonus life setup
        0x011C..=0x01B9 => {
            cpu.iff1 = false;
            cpu.iff2 = false;
            bus.write_byte(0, 0x6820, 0x00);
            bus.write_byte(0, 0xA007, 0x00);
            bus.write_byte(0, 0x9B1B, 0x10);
            for addr in 0x8400..0x8800u16 {
                bus.write_byte(0, addr, 0x00);
            }
            for addr in 0x9000..0x9400u16 {
                bus.write_byte(0, addr, 0x00);
            }
            bus.write_byte(0, 0x9B33, 0x00);
            for addr in 0x9A80..0x9AA0u16 {
                bus.write_byte(0, addr, 0x00);
            }
            for addr in 0x9800..0x9980u16 {
                bus.write_byte(0, addr, 0x00);
            }
            bus.write_byte(0, 0x6823, 0x01);

            let d0 = bus.read_byte(0, 0x89D0);
            if d0 == 0x00 {
                cpu.pc = 0x0000;
                return 100;
            }
            if d0 == 0xFF {
                bus.write_byte(0, 0x87C0, 0x00);
            } else {
                bus.write_byte(0, 0x85AA, 0x00);
                bus.write_byte(0, 0x85AB, d0 & 0x0F);
                let d1 = bus.read_byte(0, 0x89D1);
                if d1 == 0xFF {
                    bus.write_byte(0, 0x87C0, 0x01);
                } else {
                    bus.write_byte(0, 0x85AC, 0x00);
                    bus.write_byte(0, 0x85AD, d1 & 0x0F);
                    let mode = if (d1 & 0xF0) == 0x10 { 0x02 } else { 0x03 };
                    bus.write_byte(0, 0x87C0, mode);
                }
            }
            cpu.sp = 0x9A00;
            cpu.push(bus, 0x01BA);
            cpu.pc = 0x005A;
            cpu.total_cycles += 12_000;
            12_000
        }

        // $01BA..$01D0: Setup NMI transfer ring buffer pointers ($8900/$8902) and video latches
        0x01BA..=0x01D0 => {
            bus.write_word(0, 0x8900, 0x8800);
            bus.write_word(0, 0x8902, 0x8800);
            for addr in 0xA000..0xA004u16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.push(bus, 0x01D1);
            cpu.pc = 0x005A;
            cpu.total_cycles += 110;
            110
        }

        // $01D1..$0237: Configure 06XX mode $C1, init default high scores, enable VBlank IRQ
        0x01D1..=0x0237 => {
            for addr in 0x8904..0x8908u16 {
                bus.write_byte(0, addr, 0x02);
            }
            cpu.set_hl(0x8904);
            cpu.set_de(0x7000);
            cpu.set_bc(0x0004);
            cpu.a = 0xC1;
            call_cpu_sub(cpu, bus, roms, 0x00A1, 0x01E9);
            call_cpu_sub(cpu, bus, roms, 0x15A6, 0x01EC);

            for addr in 0x8000..0x8040u16 {
                bus.write_byte(0, addr, 0x7F);
            }
            bus.write_byte(0, 0x83ED, 0x10);
            bus.write_byte(0, 0x83EE, 0x10);
            bus.write_byte(0, 0x83F7, 0x10);
            bus.write_byte(0, 0x83F8, 0x10);

            call_cpu_sub(cpu, bus, roms, 0x185A, 0x020C);
            call_cpu_sub(cpu, bus, roms, 0x144F, 0x020F);
            call_cpu_sub(cpu, bus, roms, 0x1432, 0x0212);

            bus.write_byte(0, 0x8787, 0xFF);
            for i in 0..3u16 {
                let b = bus.read_byte(0, 0x89AE - i);
                bus.write_byte(0, 0x8927 + i, b);
            }
            call_cpu_sub(cpu, bus, roms, 0x14DE, 0x0228);

            bus.write_byte(0, 0x87A9, 0x01);
            bus.write_byte(0, 0x87AA, 0x01);
            bus.write_byte(0, 0x87AB, 0x01);
            bus.write_byte(0, 0x9A05, 0x01);
            bus.write_byte(0, 0x6820, 0x01);
            cpu.pc = 0x0238;
            cpu.total_cycles += 1400;
            1400
        }

        // $0238..$0267: Main Foreground Loop (waits for VBlank IRQ and triggers $32F0 high-score entry)
        0x0238..=0x0267 => {
            cpu.iff1 = true;
            cpu.iff2 = true;
            cpu.sp = 0x9A00;
            let f8400 = bus.read_byte(0, 0x8400);
            let (score_hl, round_a) = if (f8400 & 0x02) == 0 {
                (0x8414u16, bus.read_byte(0, 0x840D))
            } else {
                (0x8417u16, bus.read_byte(0, 0x840E))
            };
            cpu.set_hl(score_hl);
            cpu.b = round_a;
            let cc = bus.read_byte(0, 0x87CC);
            if (cc & 0x02) != 0 && (cc & 0x04) == 0 {
                call_cpu_sub(cpu, bus, roms, 0x0BF2, 0x0263);
                cpu.a = round_a;
                cpu.set_hl(score_hl);
                cpu.push(bus, 0x0268);
                cpu.pc = 0x32F0;
                cpu.total_cycles += 90;
                return 90;
            }
            cpu.pc = 0x0272;
            cpu.total_cycles += 65;
            65
        }

        // $0268..$0271: Return point from $32F0 high-score name entry
        0x0268..=0x0271 => {
            bus.write_byte(0, 0xA003, 0x00);
            bus_and(bus, 0x87CC, !0x02);
            cpu.pc = 0x0272;
            cpu.total_cycles += 30;
            30
        }

        // $0272..$027F: Check $87CC bit 0 (render high-score table) and loop to $0238
        0x0272..=0x027F => {
            let cc = bus.read_byte(0, 0x87CC);
            if (cc & 0x01) != 0 {
                bus.write_byte(0, 0x87CC, cc & !0x01);
                call_cpu_sub(cpu, bus, roms, 0x35E3, 0x027E);
            }
            cpu.pc = 0x0238;
            cpu.total_cycles += 40;
            40
        }

        // =====================================================================
        // High-Score Name Entry State Machine ($32F0..$3574)
        // =====================================================================
        0x32F0..=0x330C => {
            bus.write_word(0, 0x8985, cpu.sp);
            bus.write_byte(0, 0x8988, cpu.a);
            let a7 = bus.read_byte(0, 0x85A7);
            bus.write_byte(0, 0x8987, a7);
            let hl = cpu.hl();
            for i in 0..3u16 {
                let v = bus.read_byte(0, hl.wrapping_add(i));
                bus.write_byte(0, 0x8982 - i, v);
            }
            bus.write_byte(0, 0x9B3D, 0x01);
            cpu.pc = 0x330D;
            cpu.total_cycles += 95;
            95
        }
        0x330D..=0x3312 => {
            if bus.read_byte(0, 0x9B3D) == 0 {
                cpu.pc = 0x3313;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3313..=0x3332 => {
            let mut b = 5u8;
            let mut hl = 0x89A0u16;
            let mut found = false;
            while b > 0 {
                let mut de = 0x8980u16;
                let mut c = 3u8;
                while c > 0 {
                    let p = bus.read_byte(0, de);
                    let t = bus.read_byte(0, hl);
                    if p < t {
                        found = true;
                        break;
                    }
                    if p > t {
                        while c > 0 {
                            hl = hl.wrapping_add(1);
                            c -= 1;
                        }
                        break;
                    }
                    hl = hl.wrapping_add(1);
                    de = de.wrapping_add(1);
                    c -= 1;
                    if c == 0 {
                        found = true;
                    }
                }
                if found {
                    break;
                }
                b -= 1;
            }
            if !found {
                cpu.push(bus, 0x3333);
                cpu.pc = 0x353B;
            } else {
                cpu.b = b;
                cpu.pc = 0x3335;
            }
            cpu.total_cycles += 140;
            140
        }
        0x3333..=0x3334 => {
            cpu.b = 0x00;
            cpu.pc = 0x3335;
            cpu.total_cycles += 10;
            10
        }
        0x3335..=0x33EA => {
            let rank = cpu.b.wrapping_add(1);
            cpu.b = rank;
            bus.write_byte(0, 0x89C5, rank);
            if rank == 0x06 {
                cpu.sp = bus.read_word(0, 0x8985);
                cpu.ret_subroutine(bus);
                return 40;
            }
            let shift_rows = 5u8.wrapping_sub(rank);
            let mut byte_offs = 0u16;
            if shift_rows != 0 {
                for i in 0..(shift_rows as u16) {
                    let v = bus.read_byte(0, 0x89C1 + i);
                    bus.write_byte(0, 0x89C0 + i, v);
                }
                byte_offs = (shift_rows as u16) * 3;
                for i in 0..byte_offs {
                    let s = bus.read_byte(0, 0x89A3 + i);
                    bus.write_byte(0, 0x89A0 + i, s);
                    let ch = bus.read_byte(0, 0x89B3 + i);
                    bus.write_byte(0, 0x89B0 + i, ch);
                }
            }
            for i in 0..3u16 {
                let s = bus.read_byte(0, 0x8980 + i);
                bus.write_byte(0, 0x89A0 + byte_offs + i, s);
                bus.write_byte(0, 0x89B0 + byte_offs + i, 0x37);
            }
            let r_idx = 5u8.wrapping_sub(rank) as u16;
            let round_val = bus.read_byte(0, 0x8988);
            bus.write_byte(0, 0x89C0 + r_idx, round_val);

            let joy_ptr = if ((bus.read_byte(0, 0x8400) | 0xF9).wrapping_add(1)) == 0 {
                0x85A9u16
            } else {
                0x85A8u16
            };
            bus.write_word(0, 0x89C7, joy_ptr);

            call_cpu_sub(cpu, bus, roms, 0x3E33, 0x33A4);
            cpu.set_hl(0x372D);
            for ret in [0x33AA, 0x33AD, 0x33B0] {
                call_cpu_sub(cpu, bus, roms, 0x36F3, ret);
            }
            cpu.set_hl(0x8980);
            cpu.set_de(0x832F);
            cpu.set_bc(0x0300);
            call_cpu_sub(cpu, bus, roms, 0x36C6, 0x33BC);
            for _ in 0..5 {
                call_cpu_sub(cpu, bus, roms, 0x36E9, 0x33C1);
            }
            cpu.a = bus.read_byte(0, 0x8988);
            cpu.c = 0x10;
            call_cpu_sub(cpu, bus, roms, 0x3661, 0x33CB);

            bus.write_word(0, 0x89C9, 0x812F);
            bus.write_byte(0, 0x89C6, 0x00);
            bus.write_word(0, 0x89CD, 0xF028);
            call_cpu_sub(cpu, bus, roms, 0x35E7, 0x33DE);

            if bus.read_byte(0, 0x85A7) != bus.read_byte(0, 0x8987) {
                while bus.read_byte(0, 0x89C6) < 3 {
                    decomp_highscore_lock_initial_3482(cpu, bus, roms);
                    if cpu.pc == 0x34DD || cpu.pc == 0x34D2 || cpu.pc == 0x3510 {
                        return 200;
                    }
                }
                return 200;
            }
            bus.write_byte(0, 0x9A94, 0xFF);
            cpu.b = 0x02;
            cpu.push(bus, 0x33EB);
            cpu.pc = 0x34DD;
            cpu.total_cycles += 400;
            400
        }
        0x33EB..=0x33EE => {
            cpu.c = bus.read_byte(0, 0x8423);
            cpu.pc = 0x33EF;
            cpu.total_cycles += 17;
            17
        }
        0x33EF..=0x33F7 => {
            if bus.read_byte(0, 0x85A7) != bus.read_byte(0, 0x8987) {
                while bus.read_byte(0, 0x89C6) < 3 {
                    decomp_highscore_lock_initial_3482(cpu, bus, roms);
                    if cpu.pc == 0x34DD || cpu.pc == 0x34D2 || cpu.pc == 0x3510 {
                        return 120;
                    }
                }
                return 120;
            }
            let f23 = bus.read_byte(0, 0x8423);
            if f23 != cpu.c {
                cpu.a = f23;
                cpu.pc = 0x33F8;
            }
            cpu.total_cycles += 30;
            30
        }
        0x33F8..=0x3481 => {
            if (cpu.a & 0x0F) == 0 {
                if decomp_highscore_blink_and_timeout_3519(cpu, bus, roms) {
                    return 80;
                }
            }
            let joy_ptr = bus.read_word(0, 0x89C7);
            if (bus.read_byte(0, joy_ptr) & 0x10) == 0 {
                decomp_highscore_lock_initial_3482(cpu, bus, roms);
                return 80;
            }
            let dir = bus.read_byte(0, joy_ptr) & 0x0E;
            let prev_dir = bus.read_byte(0, 0x8984);
            bus.write_byte(0, 0x8984, dir);

            let mut step_right = false;
            let mut step_left = false;
            if dir != prev_dir {
                if dir == 0x02 || dir == 0x0C {
                    bus.write_byte(0, 0x89CC, 0xFD);
                    step_right = true;
                } else if dir == 0x06 {
                    bus.write_byte(0, 0x89CB, 0xFD);
                    step_left = true;
                }
            } else if dir == 0x02 || dir == 0x0C {
                step_right = true;
            } else if dir == 0x06 {
                step_left = true;
            }

            if step_right {
                let cc = bus.read_byte(0, 0x89CC).wrapping_add(1);
                bus.write_byte(0, 0x89CC, cc);
                if cc == 0 {
                    bus.write_byte(0, 0x89CC, 0xF0);
                    bus.write_byte(0, 0x89CD, 0x28);
                    let vram_ptr = bus.read_word(0, 0x89C9);
                    let mut ch = bus.read_byte(0, vram_ptr).wrapping_add(1);
                    let low = ch & 0x3F;
                    if low >= 0x38 {
                        ch = 0x1A;
                    } else if low >= 0x34 {
                        ch = ch.wrapping_add(1);
                    }
                    bus.write_byte(0, vram_ptr, ch);
                    call_cpu_sub(cpu, bus, roms, 0x35E7, 0x33EB);
                }
            } else if step_left {
                let cb = bus.read_byte(0, 0x89CB).wrapping_add(1);
                bus.write_byte(0, 0x89CB, cb);
                if cb == 0 {
                    bus.write_byte(0, 0x89CB, 0xF0);
                    bus.write_byte(0, 0x89CD, 0x28);
                    let vram_ptr = bus.read_word(0, 0x89C9);
                    let mut ch = bus.read_byte(0, vram_ptr).wrapping_sub(1);
                    let low = ch & 0x3F;
                    if low == 0x19 {
                        ch = 0x37;
                    } else if low >= 0x34 {
                        ch = ch.wrapping_sub(1);
                    }
                    bus.write_byte(0, vram_ptr, ch);
                    call_cpu_sub(cpu, bus, roms, 0x35E7, 0x33EB);
                }
            }
            cpu.pc = 0x33EB;
            cpu.total_cycles += 75;
            75
        }
        0x34C7..=0x34D1 => {
            if bus.read_byte(0, 0x9A94) == 0 {
                bus.write_byte(0, 0x9B3D, 0x02);
                cpu.pc = 0x34D2;
            }
            cpu.total_cycles += 25;
            25
        }
        0x34D2..=0x34DC => {
            if bus.read_byte(0, 0x9B3D) == 0 {
                cpu.sp = bus.read_word(0, 0x8985);
                cpu.ret_subroutine(bus);
            }
            cpu.total_cycles += 25;
            25
        }
        0x34DD..=0x34E3 => {
            if (bus.read_byte(0, 0x8423) & 0x3F) == 0 {
                cpu.pc = 0x34E4;
            }
            cpu.total_cycles += 25;
            25
        }
        0x34E4..=0x34ED => {
            if (bus.read_byte(0, 0x8423) & 0x3F) != 0 {
                cpu.b = cpu.b.wrapping_sub(1);
                if cpu.b != 0 {
                    cpu.pc = 0x34DD;
                } else {
                    cpu.ret_subroutine(bus);
                }
            }
            cpu.total_cycles += 25;
            25
        }
        0x3510..=0x3517 => {
            if bus.read_byte(0, 0x9A94) != 0 {
                bus.write_byte(0, 0x9B3D, 0x02);
                cpu.pc = 0x34D2;
            }
            cpu.total_cycles += 25;
            25
        }
        0x353B..=0x3551 => {
            call_cpu_sub(cpu, bus, roms, 0x3E33, 0x353E);
            cpu.set_hl(0x3777);
            for ret in [0x3544, 0x3547, 0x354A, 0x354D] {
                call_cpu_sub(cpu, bus, roms, 0x36F3, ret);
            }
            bus.write_byte(0, 0x9A82, 0x01);
            cpu.pc = 0x3552;
            cpu.total_cycles += 180;
            180
        }
        0x3552..=0x356D => {
            let f23 = bus.read_byte(0, 0x8423);
            if (f23 & 0x1F) == 0 {
                call_cpu_sub(cpu, bus, roms, 0x3575, 0x355A);
            } else if (f23 & 0x0F) == 0 {
                call_cpu_sub(cpu, bus, roms, 0x35C9, 0x355F);
            }
            if bus.read_byte(0, 0x9A82) == 0 {
                call_cpu_sub(cpu, bus, roms, 0x3575, 0x3568);
                cpu.c = bus.read_byte(0, 0x8423).wrapping_add(0x3C);
                cpu.pc = 0x356E;
            }
            cpu.total_cycles += 40;
            40
        }
        0x356E..=0x3574 => {
            if bus.read_byte(0, 0x8423) == cpu.c {
                cpu.ret_subroutine(bus);
            }
            cpu.total_cycles += 25;
            25
        }

        // =====================================================================
        // Power-On Self-Test & Service Mode Loop ($387E..$3A84)
        // =====================================================================
        0x387E..=0x388E => {
            cpu.iff1 = false;
            cpu.iff2 = false;
            bus.write_byte(0, 0x7100, 0x10);
            bus.write_byte(0, 0x6823, 0x00); // Assert sub_cpu_reset = true
            bus.write_byte(0, 0xA002, 0x00);
            bus.write_byte(0, 0x6820, 0x00);
            cpu.a = 0xFF;
            cpu.pc = 0x388F;
            cpu.total_cycles += 60;
            60
        }
        0x388F..=0x3972 => {
            bus.write_byte(0, 0x7000, 0xFF);
            bus.write_byte(0, 0xB840, 0x04);
            for addr in 0x8000..0x8400u16 {
                bus.write_byte(0, addr, 0x00);
            }
            cpu.sp = 0x8400;
            for i in 0..0x20u16 {
                let v = bus.read_byte(0, 0x89E0 + i);
                bus.write_byte(0, 0x8000 + i, v);
            }
            for &bank in &[0x8400u16, 0x8800, 0x9000, 0x9800] {
                cpu.set_hl(bank);
                call_cpu_sub(cpu, bus, roms, 0x3DE3, 0x3911);
            }
            for i in 0..0x20u16 {
                let v = bus.read_byte(0, 0x8000 + i);
                bus.write_byte(0, 0x89E0 + i, v);
            }
            cpu.sp = 0x9A00;
            bus.write_word(0, 0x8902, 0x8800);
            bus.write_word(0, 0x8900, 0x8800);
            call_cpu_sub(cpu, bus, roms, 0x3E3B, 0x392E);
            cpu.set_hl(0x3E95);
            call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3934);

            bus.write_byte(0, 0x8A00, 0x00);
            bus.write_byte(0, 0x8A01, 0x00);
            bus.write_byte(0, 0x879A, 0x01);
            bus.write_byte(0, 0x6822, 0x01);
            bus.write_byte(0, 0x6825, 0x01);
            bus.write_byte(0, 0x6826, 0x01);
            bus.write_byte(0, 0x6827, 0x01);
            bus.write_byte(0, 0x6823, 0x01); // Release CPU 1 & CPU 2 from reset!
            cpu.pc = 0x3973;
            cpu.total_cycles += 800;
            800
        }
        0x3973..=0x397F => {
            bus.write_byte(0, 0x6830, cpu.a);
            let s0 = bus.read_byte(0, 0x8A00);
            if s0 != 0 {
                cpu.pc = if s0 == 0xFF { 0x3980 } else { 0x3E6A };
            }
            cpu.total_cycles += 25;
            25
        }
        0x3980..=0x3989 => {
            bus.write_byte(0, 0x6830, cpu.a);
            let s1 = bus.read_byte(0, 0x8A01);
            if s1 != 0 {
                cpu.pc = if s1 == 0xFF { 0x398A } else { 0x3E6A };
            }
            cpu.total_cycles += 25;
            25
        }
        0x398A..=0x39AF => {
            cpu.set_hl(0x3E9F);
            call_cpu_sub(cpu, bus, roms, 0x36F3, 0x3990);
            bus.write_byte(0, 0x8A00, 0x00);
            bus.write_byte(0, 0x8A01, 0x00);
            cpu.set_hl(0x3FE5);
            cpu.set_de(0x7000);
            cpu.set_bc(0x0003);
            z80_exx(cpu);
            cpu.a = 0xA1;
            bus.write_byte(0, 0x7100, 0xA1);
            cpu.pc = 0x39B0;
            cpu.total_cycles += 200;
            200
        }
        0x39B0..=0x39B6 => {
            if bus.read_byte(0, 0x7100) == 0x10 {
                cpu.pc = 0x39B7;
            }
            cpu.total_cycles += 25;
            25
        }
        0x39B7..=0x39D2 => {
            for i in 0..9u16 {
                let b = roms.cpu0[(0x3FEC + i) as usize];
                bus.write_byte(0, 0x8A18 + i, b);
            }
            cpu.im = 1;
            bus.write_byte(0, 0x6820, 0x00);
            bus.write_byte(0, 0x6820, 0x01);
            cpu.iff1 = true;
            cpu.iff2 = true;
            cpu.c = bus.read_byte(0, 0x8423).wrapping_add(0x04);
            cpu.pc = 0x39D3;
            cpu.total_cycles += 120;
            120
        }
        0x39D3..=0x39D8 => {
            if bus.read_byte(0, 0x8423) == cpu.c {
                bus.write_byte(0, 0x9B3D, 0x04);
                cpu.pc = 0x39DE;
            }
            cpu.total_cycles += 25;
            25
        }
        0x39DE..=0x39E3 => {
            if bus.read_byte(0, 0x9B3D) == 0 {
                cpu.c = bus.read_byte(0, 0x8423);
                cpu.pc = 0x39E8;
            }
            cpu.total_cycles += 25;
            25
        }
        0x39E4..=0x39ED => {
            if bus.read_byte(0, 0x8423) != cpu.c {
                cpu.pc = 0x39EE;
            }
            cpu.total_cycles += 25;
            25
        }
        0x39EE..=0x3A11 => {
            call_cpu_sub(cpu, bus, roms, 0x3AB8, 0x39F1);
            call_cpu_sub(cpu, bus, roms, 0x3A85, 0x39F4);
            call_cpu_sub(cpu, bus, roms, 0x3AF1, 0x39F7);
            call_cpu_sub(cpu, bus, roms, 0x3B4A, 0x39FA);
            call_cpu_sub(cpu, bus, roms, 0x3BC0, 0x39FD);
            call_cpu_sub(cpu, bus, roms, 0x3BDE, 0x3A00);
            call_cpu_sub(cpu, bus, roms, 0x3BEC, 0x3A03);
            call_cpu_sub(cpu, bus, roms, 0x3BFC, 0x3A06);
            call_cpu_sub(cpu, bus, roms, 0x3C47, 0x3A09);
            call_cpu_sub(cpu, bus, roms, 0x3CB3, 0x3A0C);

            if (bus.read_byte(0, 0x85A7) & 0x80) == 0 {
                cpu.c = bus.read_byte(0, 0x8423);
                cpu.pc = 0x39E8;
            } else {
                cpu.pc = 0x3A12;
            }
            cpu.total_cycles += 400;
            400
        }
        0x3A12..=0x3A1A => {
            call_cpu_sub(cpu, bus, roms, 0x3E85, 0x3A15);
            cpu.c = bus.read_byte(0, 0x8423).wrapping_add(0x64);
            cpu.pc = 0x3A1B;
            cpu.total_cycles += 120;
            120
        }
        0x3A1B..=0x3A20 => {
            if bus.read_byte(0, 0x8423) == cpu.c {
                cpu.pc = 0x3A21;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3A21..=0x3A2B => {
            if (bus.read_byte(0, 0x85A7) & 0x80) != 0 {
                bus.write_byte(0, 0x9B3D, 0x01);
                cpu.pc = 0x3A2C;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3A2C..=0x3A32 => {
            if bus.read_byte(0, 0x9B3D) == 0 {
                cpu.iff1 = false;
                cpu.iff2 = false;
                cpu.pc = 0x3A33;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3A33..=0x3A3C => {
            bus.write_byte(0, 0x6830, cpu.a);
            if bus.read_byte(0, 0x7100) == 0x10 {
                cpu.pc = 0x3A3D;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3A3D..=0x3A5A => {
            cpu.set_hl(0x8A18);
            cpu.set_de(0x7000);
            cpu.set_bc(0x0009);
            z80_exx(cpu);
            bus.write_byte(0, 0x7000, 0x00);
            cpu.a = 0xC1;
            bus.write_byte(0, 0x7100, 0xC1);
            cpu.pc = 0x3A5B;
            cpu.total_cycles += 120;
            120
        }
        0x3A5B..=0x3A61 => {
            if bus.read_byte(0, 0x7100) == 0x10 {
                cpu.pc = 0x3A62;
            }
            cpu.total_cycles += 25;
            25
        }
        0x3A62..=0x3A70 => {
            cpu.set_hl(0x7000);
            cpu.set_de(0x8A10);
            cpu.set_bc(0x0003);
            z80_exx(cpu);
            cpu.a = 0xB1;
            bus.write_byte(0, 0x7100, 0xB1);
            cpu.pc = 0x3A71;
            cpu.total_cycles += 80;
            80
        }
        0x3A71..=0x3A84 => {
            if bus.read_byte(0, 0x7100) == 0x10 {
                if bus.read_byte(0, 0x8A10) != 0 {
                    cpu.pc = 0x3A3D;
                } else {
                    cpu.pc = 0x011C;
                }
            }
            cpu.total_cycles += 25;
            25
        }

        // $3CFE..$3D06: Secret Easter-egg loop waiting for service switch exit
        0x3CFE..=0x3D06 => {
            if (bus.read_byte(0, 0x85A7) & 0x80) != 0 {
                cpu.pc = 0x3A12;
            }
            cpu.total_cycles += 25;
            25
        }

        // $3D78..$3DE2 & $3E65..$3E84: Diagnostic RAM/ROM error halt loops
        0x3D78..=0x3DE2 | 0x3E65..=0x3E84 => {
            bus.write_byte(0, 0x6830, cpu.a);
            cpu.total_cycles += 25;
            25
        }

        _ => {
            cpu.pc = 0x0238;
            cpu.total_cycles += 4;
            4
        }
    }
}


