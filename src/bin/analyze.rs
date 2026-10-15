use digdug_re::disasm::{analyze_cpu_rom, decode_one};
use digdug_re::rom::DigDugRoms;
use digdug_re::symbols::resolve_code_symbol;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

fn dump_disassembly_file(
    out_path: &Path,
    title: &str,
    cpu_id: usize,
    rom: &[u8],
    rom_limit: u16,
) -> std::io::Result<(usize, usize)> {
    let res = analyze_cpu_rom(rom, rom_limit, cpu_id);
    let mut f = File::create(out_path)?;

    writeln!(
        f,
        "; ============================================================================="
    )?;
    writeln!(f, "; {}", title)?;
    writeln!(
        f,
        "; Discovered reachable instructions: {}",
        res.instructions.len()
    )?;
    writeln!(
        f,
        "; Discovered subroutines / entry points: {}",
        res.subroutines.len()
    )?;
    writeln!(
        f,
        "; =============================================================================\n"
    )?;

    // Also do a linear sweep fallback for any unvisited code/tables so the listing covers 0..rom_limit
    let mut pc = 0u16;
    while pc < rom_limit {
        if let Some((sym, desc)) = resolve_code_symbol(cpu_id, pc) {
            writeln!(
                f,
                "\n; -----------------------------------------------------------------------------"
            )?;
            writeln!(f, "; SUBROUTINE <{}>: {}", sym, desc)?;
            if let Some(callers) = res.call_graph.get(&pc) {
                let caller_list: Vec<String> =
                    callers.iter().map(|c| format!("${:04X}", c)).collect();
                writeln!(f, "; XREFs (Callers): {}", caller_list.join(", "))?;
            }
            writeln!(
                f,
                "; -----------------------------------------------------------------------------"
            )?;
            writeln!(f, "{}:", sym)?;
        } else if res.subroutines.contains(&pc) {
            writeln!(f, "\nsub_{:04X}:", pc)?;
            if let Some(callers) = res.call_graph.get(&pc) {
                let caller_list: Vec<String> =
                    callers.iter().map(|c| format!("${:04X}", c)).collect();
                writeln!(f, "; XREFs (Callers): {}", caller_list.join(", "))?;
            }
        }

        if let Some(inst) = res.instructions.get(&pc) {
            let hex: Vec<String> = inst.bytes.iter().map(|b| format!("{:02X}", b)).collect();
            writeln!(
                f,
                "    {:04X}: {:<12} {}",
                pc,
                hex.join(" "),
                inst.mnemonic
            )?;
            pc = pc.wrapping_add(inst.bytes.len() as u16);
        } else {
            // Decode linearly so we can see dormant code (like 0x0018..0x0026!) or data tables
            let inst = decode_one(rom, pc, cpu_id);
            let hex: Vec<String> = inst.bytes.iter().map(|b| format!("{:02X}", b)).collect();
            writeln!(
                f,
                "  * {:04X}: {:<12} {} ; [unreached/table]",
                pc,
                hex.join(" "),
                inst.mnemonic
            )?;
            pc = pc.wrapping_add(inst.bytes.len() as u16);
        }
    }

    Ok((res.instructions.len(), res.subroutines.len()))
}

fn write_reverse_engineering_report(
    report_path: &Path,
    c0_inst: usize,
    c0_sub: usize,
    c1_inst: usize,
    c1_sub: usize,
    c2_inst: usize,
    c2_sub: usize,
) -> std::io::Result<()> {
    let mut f = File::create(report_path)?;
    writeln!(f, "# Dig Dug (1982) — Z80 Reverse Engineering & Rust Decompilation Report\n")?;
    writeln!(f, "## 1. Hardware & Tri-Z80 Architecture Overview\n")?;
    writeln!(f, "*Dig Dug* (Namco, 1982) runs on three parallel Zilog Z80 microprocessors clocked at **3.072 MHz** (`18.432 MHz / 6`) sharing a common 8 KB memory window (`$8000..$9FFF`):\n")?;
    writeln!(f, "| Processor | ROM Range | Reachable Instructions | Subroutines | Primary Responsibilities |")?;
    writeln!(f, "| :--- | :--- | ---: | ---: | :--- |")?;
    writeln!(f, "| **CPU 0 (Main)** | `$0000..$3FFF` (16 KB) | `{}` | `{}` | Power-on self-test, Namco `06XX`/`51XX`/`53XX` NMI I/O, state machine, player movement, pump/harpoon & rock physics, score/round HUD, EAROM high-score save |", c0_inst, c0_sub)?;
    writeln!(f, "| **CPU 1 (Sub / AI)** | `$0000..$1FFF` (8 KB) | `{}` | `{}` | Pooka & Fygar pathfinding, ghosting (`mecha-ru`) timers, Fygar fire-breathing state machine, monster speed scaling (`$1B9F`), sprite animation framing |", c1_inst, c1_sub)?;
    writeln!(f, "| **CPU 2 (Sound)** | `$0000..$0FFF` (4 KB) | `{}` | `{}` | 120 Hz NMI-driven sequencer driving the 3-voice Namco WSG wavetable synth (`$6800..$681F`), walking melody modulation, sound effects |", c2_inst, c2_sub)?;
    writeln!(f, "\n---\n")?;

    writeln!(f, "## 2. Reverse-Engineered Shared Memory Map (`$6800..$B840`)\n")?;
    writeln!(f, "| Address Range | Symbol | Description |")?;
    writeln!(f, "| :--- | :--- | :--- |")?;
    writeln!(f, "| `$6800..$681F` | `NAMCO_WSG_REGS` | 3-channel Namco WSG frequency (20-bit/16-bit), waveform select (`0..7`), and 4-bit volume registers |")?;
    writeln!(f, "| `$6820..$6823` | `LS259_MISC_LATCH` | Bit 0: CPU0 VBlank IRQ enable; Bit 1: CPU1 VBlank IRQ enable; Bit 2: CPU2 NMI disable; Bit 3: Sub-CPU Reset (`/RESET` for CPU1 & CPU2) |")?;
    writeln!(f, "| `$7000 / $7100` | `NAMCO_06XX` | Custom bus arbiter communicating with `51XX` (joystick/coin/credit MCU) and `53XX` (DIP switch reader) via CPU0 NMI (`$0066`) |")?;
    writeln!(f, "| `$8000..$83FF` | `VRAM_TILEMAP` | 32x32 (36x28 visible) foreground 8x8 1bpp HUD & tunnel character tilemap |")?;
    writeln!(f, "| `$8400` | `GAME_STATUS` | Bit 0: Two-player mode; Bit 1: Active player (`0` = P1, `1` = P2); Bit 6: Active game |")?;
    writeln!(f, "| `$840A / $840B` | `P1_LIVES / P2_LIVES` | Remaining lives for Player 1 and Player 2 |")?;
    writeln!(f, "| `$840D / $840E` | `P1_ROUND / P2_ROUND` | 8-bit round counters (`1..255`, wraps to `0` on Round 256 Kill Screen in Namco sets) |")?;
    writeln!(f, "| `$840F / $8410` | `ROUND_CLAMPED` | Active stage index & difficulty step (clamped to `$0F` after Round 15 by `$19F7`) |")?;
    writeln!(f, "| `$8504 / $850C` | `DIGDUG_X / DIGDUG_Y` | Player X and Y playfield coordinates |")?;
    writeln!(f, "| `$8505..$8512` | `MONSTER_XY` | Monster X (`$8505..$850A`) and Y (`$850D..$8512`) coordinates for up to 6 monsters |")?;
    writeln!(f, "| `$858D..$8592` | `MONSTER_INFLATE` | Harpoon pump inflation stage (`0..4`) for each monster slot |")?;
    writeln!(f, "| `$87BD` | `MONSTER_SPEED` | Base monster speed computed by CPU1 `$1B9F` (`calc_monster_difficulty`) |")?;
    writeln!(f, "| `$8B80..$8BFF` | `SPRITE_OBJRAM` | 64 sprites x 2 bytes: byte 0 = sprite code (bit 7 = 32x32 size), byte 1 = 6-bit CLUT color |")?;
    writeln!(f, "| `$9000..$937F` | `DIRT_GRID` | 32x28 underground dirt presence & tunnel collision map (initialized by CPU0 `$177C`) |")?;
    writeln!(f, "| `$9380..$93FF` | `SPRITE_POSRAM` | 64 sprites x 2 bytes: X and Y hardware screen coordinates |")?;
    writeln!(f, "| `$9B80..$9BFF` | `SPRITE_FLPRAM` | 64 sprites x 2 bytes: flip/size flags (`vflip`, `hflip`, `double_y`, `double_x`) and disable bit |")?;
    writeln!(f, "| `$A000..$A007` | `VIDEO_LATCH` | Bits 0-1: Background dirt map select (`dd1.10b`); Bit 2: TX color mode; Bit 3: BG disable; Bits 4-5: BG color bank |")?;
    writeln!(f, "| `$B800..$B840` | `EAROM_ER2055` | 64-byte non-volatile high-score memory |")?;
    writeln!(f, "\n---\n")?;

    writeln!(f, "## 3. Binary Diff Findings Across All 6 ROM Variants in `digdug.zip`\n")?;
    writeln!(f, "### A. The Dormant Round 256 Kill Screen Fix (`digdug` vs. `digdugat`)\n")?;
    writeln!(f, "In Namco Rev 2 (`digdug`), completing Round 255 (`$FF`) increments `$840D` (`P1_ROUND`) to `$00` inside `advance_round_and_draw_hud` (`$19A3`). Because the monster placement generator uses the round number to index stage tables, Round `0` spawns a Pooka directly on top of Dig Dug's spawn tile — creating the famous **Round 256 Kill Screen**.\n")?;
    writeln!(f, "However, disassembling `dd1a.1` (`CPU0 $0018..$0026`) reveals that **Namco's programmers had already written the exact fix into the Namco ROM**, leaving it completely uncalled:\n")?;
    writeln!(f, "```z80\n; CPU0 $0018 (Present in both Namco Rev 2 and Atari Rev 2, but uncalled in Namco!)\ndormant_round_rollover:\n    0018: C0           RET NZ          ; If (++round != 0), return\n    0019: 3E 9C        LD A, $9C       ; Otherwise roll round 256 (0) back to Round 156 ($9C)!\n    001B: 77           LD (HL), A\n    001C: C9           RET\n\n; CPU0 $0020 (Modulo-100 helper so rounds 100..255 display their lower 2 digits)\nmod_100:\n    0020: FE 64        CP $64          ; Compare A with 100\n    0022: D8           RET C           ; Return if A < 100\n    0023: D6 64        SUB $64         ; A -= 100\n    0025: 18 F9        JR $0020\n```\n")?;
    writeln!(f, "In Atari Rev 2 (`digdugat`), Atari activated these two dormant routines by inserting three 1-byte `RST` calls inside `advance_round_and_draw_hud` (`$19A3`), shifting all subsequent CPU0 code by `+3` bytes:\n")?;
    writeln!(f, "- `19AD: DF` (`RST $18` right after `INC (HL)` on `$840D` for Player 1)\n- `19B4: DF` (`RST $18` right after `INC (HL)` on `$840E` for Player 2)\n- `19CD: E7` (`RST $20` right before `CALL $1A24` binary-to-BCD conversion)\n")?;

    writeln!(f, "### B. The Monster Speed 8-Bit Overflow Bug (`digdug1` vs. `digdug`)\n")?;
    writeln!(f, "On CPU 1 (`sub_1B9F` / `calc_monster_difficulty`), the game looks up the base monster speed from ROM table `$1BFE` and stores it at `$87BD`. For rounds $\\ge 30$ (`$1E`), it adds `(round - 30)` to `$87BD`.\n")?;
    writeln!(f, "- In **Namco Rev 1 (`digdug1`)**, `ADD A, (HL)` at `$1BDD` was written back to `$87BD` with no carry check (`1BDD: 86 77`). Around Round 136+, `base_speed + (round - 30)` wraps past `255` to `0`, completely freezing all monsters!\n")?;
    writeln!(f, "- In **Namco Rev 2 (`digdug`)**, Namco inserted 4 bytes at `$1BDF` (`30 02 36 FF` -> `JR NC, $1BE3; LD (HL), $FF`), clamping `$87BD` at `255`.\n")?;

    writeln!(f, "### C. Sidam (`digsid`) & Zig Zag Bootleg (`dzigzag`)\n")?;
    writeln!(f, "- **Sidam (`digsid`)**: Starts gameplay directly on Round 4's dirt/enemy layout (`CPU0 $1998` patch) and raises the default high-score table from `10,000` to `25,000`.\n")?;
    writeln!(f, "- **Zig Zag Bootleg (`dzigzag`)**: Contains identical CPU0/CPU1/CPU2 code to `digdug1` (including the hidden ASCII string `\"NAMCO\"` at `CPU2 $0EEA`, proving it was bootlegged from Namco Rev 1 rather than Atari Rev 1) and adds a 4th Z80 (`dzz4.rom`) to emulate the custom Namco `51XX`/`53XX` MCUs.\n")?;

    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base_dir = Path::new("roms/extracted");
    let disasm_dir = Path::new("disasm");
    fs::create_dir_all(disasm_dir)?;

    let roms = DigDugRoms::load_from_extracted_dir(base_dir)?;

    let (c0_inst, c0_sub) = dump_disassembly_file(
        &disasm_dir.join("cpu0_main.asm"),
        "Dig Dug (Namco Rev 2) - CPU 0 (Main Z80, 16 KB, $0000..$3FFF)",
        0,
        &roms.cpu0,
        0x4000,
    )?;
    let (c1_inst, c1_sub) = dump_disassembly_file(
        &disasm_dir.join("cpu1_sub_ai.asm"),
        "Dig Dug (Namco Rev 2) - CPU 1 (Sub/AI Z80, 8 KB, $0000..$1FFF)",
        1,
        &roms.cpu1,
        0x2000,
    )?;
    let (c2_inst, c2_sub) = dump_disassembly_file(
        &disasm_dir.join("cpu2_sound.asm"),
        "Dig Dug (Namco Rev 2) - CPU 2 (Sound Z80, 4 KB, $0000..$0FFF)",
        2,
        &roms.cpu2,
        0x1000,
    )?;

    let report_path = disasm_dir.join("REVERSE_ENGINEERING_REPORT.md");
    write_reverse_engineering_report(&report_path, c0_inst, c0_sub, c1_inst, c1_sub, c2_inst, c2_sub)?;

    println!("=== Disassembly & Reverse Engineering Report Complete ===");
    println!(
        "CPU 0 (Main) : {} reachable instructions, {} subroutines -> disasm/cpu0_main.asm",
        c0_inst, c0_sub
    );
    println!(
        "CPU 1 (Sub)  : {} reachable instructions, {} subroutines -> disasm/cpu1_sub_ai.asm",
        c1_inst, c1_sub
    );
    println!(
        "CPU 2 (Sound): {} reachable instructions, {} subroutines -> disasm/cpu2_sound.asm",
        c2_inst, c2_sub
    );
    println!("Markdown Report -> disasm/REVERSE_ENGINEERING_REPORT.md");

    Ok(())
}
