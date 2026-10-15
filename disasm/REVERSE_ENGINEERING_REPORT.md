# Dig Dug (1982) — Z80 Reverse Engineering & Hybrid Rust Decompilation Report

## 1. Hardware & Tri-Z80 Architecture Overview

*Dig Dug* (Namco, 1982) runs on three parallel Zilog Z80 microprocessors clocked at **3.072 MHz** (`18.432 MHz / 6`) sharing a common 8 KB memory window (`$8000..$9FFF`):

| Processor | ROM Range | Reachable Instructions | Subroutines | Primary Responsibilities |
| :--- | :--- | ---: | ---: | :--- |
| **CPU 0 (Main)** | `$0000..$3FFF` (16 KB) | `7198` | `205` | Power-on self-test, Namco `06XX`/`51XX`/`53XX` NMI I/O, state machine, player movement, pump/harpoon & rock physics, score/round HUD, EAROM high-score save |
| **CPU 1 (Sub / AI)** | `$0000..$1FFF` (8 KB) | `2954` | `56` | Pooka & Fygar pathfinding, ghosting (`mecha-ru`) timers, Fygar fire-breathing state machine, monster speed scaling (`$1B9F`), sprite animation framing |
| **CPU 2 (Sound)** | `$0000..$0FFF` (4 KB) | `716` | `13` | 120 Hz NMI-driven sequencer driving the 3-voice Namco WSG wavetable synth (`$6800..$681F`), walking melody modulation, sound effects |

---

## 2. Reverse-Engineered Shared Memory Map (`$6800..$B840`)

| Address Range | Symbol | Description |
| :--- | :--- | :--- |
| `$6800..$681F` | `NAMCO_WSG_REGS` | 3-channel Namco WSG frequency (20-bit/16-bit), waveform select (`0..7`), and 4-bit volume registers |
| `$6820..$6823` | `LS259_MISC_LATCH` | Bit 0: CPU0 VBlank IRQ enable; Bit 1: CPU1 VBlank IRQ enable; Bit 2: CPU2 NMI disable; Bit 3: Sub-CPU Reset (`/RESET` for CPU1 & CPU2) |
| `$7000 / $7100` | `NAMCO_06XX` | Custom bus arbiter communicating with `51XX` (joystick/coin/credit MCU) and `53XX` (DIP switch reader) via CPU0 NMI (`$0066`) |
| `$8000..$83FF` | `VRAM_TILEMAP` | 32x32 (36x28 visible) foreground 8x8 1bpp HUD & tunnel character tilemap |
| `$8400` | `GAME_STATUS` | Bit 0: Two-player mode; Bit 1: Active player (`0` = P1, `1` = P2); Bit 6: Active game |
| `$840A / $840B` | `P1_LIVES / P2_LIVES` | Remaining lives for Player 1 and Player 2 |
| `$840D / $840E` | `P1_ROUND / P2_ROUND` | 8-bit round counters (`1..255`, wraps to `0` on Round 256 Kill Screen in Namco sets) |
| `$840F / $8410` | `ROUND_CLAMPED` | Active stage index & difficulty step (clamped to `$0F` after Round 15 by `$19F7`) |
| `$8504 / $850C` | `DIGDUG_X / DIGDUG_Y` | Player X and Y playfield coordinates |
| `$8505..$8512` | `MONSTER_XY` | Monster X (`$8505..$850A`) and Y (`$850D..$8512`) coordinates for up to 6 monsters |
| `$858D..$8592` | `MONSTER_INFLATE` | Harpoon pump inflation stage (`0..4`) for each monster slot |
| `$87BD` | `MONSTER_SPEED` | Base monster speed computed by CPU1 `$1B9F` (`calc_monster_difficulty`) |
| `$8B80..$8BFF` | `SPRITE_OBJRAM` | 64 sprites x 2 bytes: byte 0 = sprite code (bit 7 = 32x32 size), byte 1 = 6-bit CLUT color |
| `$9000..$937F` | `DIRT_GRID` | 32x28 underground dirt presence & tunnel collision map (initialized by CPU0 `$177C`) |
| `$9380..$93FF` | `SPRITE_POSRAM` | 64 sprites x 2 bytes: X and Y hardware screen coordinates |
| `$9B80..$9BFF` | `SPRITE_FLPRAM` | 64 sprites x 2 bytes: flip/size flags (`vflip`, `hflip`, `double_y`, `double_x`) and disable bit |
| `$A000..$A007` | `VIDEO_LATCH` | Bits 0-1: Background dirt map select (`dd1.10b`); Bit 2: TX color mode; Bit 3: BG disable; Bits 4-5: BG color bank |
| `$B800..$B840` | `EAROM_ER2055` | 64-byte non-volatile high-score memory |

---

## 3. Binary Diff Findings Across All 6 ROM Variants in `digdug.zip`

### A. The Dormant Round 256 Kill Screen Fix (`digdug` vs. `digdugat`)

In Namco Rev 2 (`digdug`), completing Round 255 (`$FF`) increments `$840D` (`P1_ROUND`) to `$00` inside `advance_round_and_draw_hud` (`$19A3`). Because the monster placement generator uses the round number to index stage tables, Round `0` spawns a Pooka directly on top of Dig Dug's spawn tile — creating the famous **Round 256 Kill Screen**.

However, disassembling `dd1a.1` (`CPU0 $0018..$0026`) reveals that **Namco's programmers had already written the exact fix into the Namco ROM**, leaving it completely uncalled:

```z80
; CPU0 $0018 (Present in both Namco Rev 2 and Atari Rev 2, but uncalled in Namco!)
dormant_round_rollover:
    0018: C0           RET NZ          ; If (++round != 0), return
    0019: 3E 9C        LD A, $9C       ; Otherwise roll round 256 (0) back to Round 156 ($9C)!
    001B: 77           LD (HL), A
    001C: C9           RET

; CPU0 $0020 (Modulo-100 helper so rounds 100..255 display their lower 2 digits)
mod_100:
    0020: FE 64        CP $64          ; Compare A with 100
    0022: D8           RET C           ; Return if A < 100
    0023: D6 64        SUB $64         ; A -= 100
    0025: 18 F9        JR $0020
```

In Atari Rev 2 (`digdugat`), Atari activated these two dormant routines by inserting three 1-byte `RST` calls inside `advance_round_and_draw_hud` (`$19A3`), shifting all subsequent CPU0 code by `+3` bytes:

- `19AD: DF` (`RST $18` right after `INC (HL)` on `$840D` for Player 1)
- `19B4: DF` (`RST $18` right after `INC (HL)` on `$840E` for Player 2)
- `19CD: E7` (`RST $20` right before `CALL $1A24` binary-to-BCD conversion)

### B. The Monster Speed 8-Bit Overflow Bug (`digdug1` vs. `digdug`)

On CPU 1 (`sub_1B9F` / `calc_monster_difficulty`), the game looks up the base monster speed from ROM table `$1BFE` and stores it at `$87BD`. For rounds $\ge 30$ (`$1E`), it adds `(round - 30)` to `$87BD`.

- In **Namco Rev 1 (`digdug1`)**, `ADD A, (HL)` at `$1BDD` was written back to `$87BD` with no carry check (`1BDD: 86 77`). Around Round 136+, `base_speed + (round - 30)` wraps past `255` to `0`, completely freezing all monsters!

- In **Namco Rev 2 (`digdug`)**, Namco inserted 4 bytes at `$1BDF` (`30 02 36 FF` -> `JR NC, $1BE3; LD (HL), $FF`), clamping `$87BD` at `255`.

### C. Sidam (`digsid`) & Zig Zag Bootleg (`dzigzag`)

- **Sidam (`digsid`)**: Starts gameplay directly on Round 4's dirt/enemy layout (`CPU0 $1998` patch) and raises the default high-score table from `10,000` to `25,000`.

- **Zig Zag Bootleg (`dzigzag`)**: Contains identical CPU0/CPU1/CPU2 code to `digdug1` (including the hidden ASCII string `"NAMCO"` at `CPU2 $0EEA`, proving it was bootlegged from Namco Rev 1 rather than Atari Rev 1) and adds a 4th Z80 (`dzz4.rom`) to emulate the custom Namco `51XX`/`53XX` MCUs.

