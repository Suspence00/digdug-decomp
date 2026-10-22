# Coin, start and fire inputs

`src/namco51.rs` models the observed coin/start and credit-mode fire qualification
of the Namco 51XX. It is sampled once per native video frame. Both edges require two
consecutive matching samples. Repeated host reads do not advance qualification
or repeat an accepted press. A one-frame release between held coin presses does
not rearm the coin input.

Fire has two signals: a new press launches a harpoon, while the held button runs
the pump. Only the new-press signal uses qualification; the held signal remains
immediate. A confirmed press is latched for one host input packet, so repeated
reads cannot manufacture a press or repeat it in another packet. One neutral
frame does not rearm fire; two neutral frames followed by two held frames do.
This prevents a one-frame flicker from launching a shot while preserving held
pumping input.

A successful start disables further start-credit deductions until the game
sends 51XX data command `2` again. This prevents Start presses during a game from
spending spare credits. Data command `5` disables starts. Command `1` consumes
four coinage arguments so those bytes cannot accidentally rearm starts.

These are chip data commands, distinct from the 06XX control register that
selects chips, read/write direction and transfer clock division. Startup and
game-over cleanup send data command `2`; selecting the `C1` transfer alone does
not rearm starts. Reset clears qualification and command state.

## Evidence and checks

Independent runs with MAME 0.289, the supported Rev 2 ROMs, fixed DIPs and zero
EAROM established:

- One-frame coin, one-player start and two-player start pulses are rejected;
  two-frame pulses qualify on their second physical frame.
- Three coin presses separated by one neutral frame produce one credit;
  two or three neutral frames between presses allow three credits.
- Pressing Start during an active game preserves an unused credit.
- Direction input reaches the MCU response without this two-frame filter.
- In a naturally reached playable scene, one-frame fire taps launch nothing;
  two-frame presses launch one harpoon. A thirty-frame hold launches once, and
  a one-frame release does not rearm the next shot. Two release frames do.

The tracked `tools/mame/input_qualification.txt` replay reproduces short pulses,
release qualification and a two-player start. Its exact game credit-latch and
setup frames are covered by a ROM-dependent regression. Synthetic tests cover
independent lines, repeated reads, reset, command parsing and start-credit gating.
The existing lifecycle test also covers game over followed by another start.
See [timeline results and commands](MAME_TIMELINE.md#coinstart-qualification-replay).

`tools/mame/fire_qualification.txt` reaches a firing position without patching
game RAM, then exercises a short tap, a normal press, a long hold and both
release gaps. Native and MAME launch at frames 1403, 1425 and 1464. The matching
gameplay regression checks actual harpoon activation and completion, a living
player and preservation of the spare credit:

```sh
cargo test --locked --release --test gameplay_scenarios fire_taps_holds_and_releases_launch_only_qualified_shots -- --include-ignored
```

To capture the same scene independently:

```powershell
pwsh -NoProfile -File tools/mame/run_timeline.ps1 -Script tools/mame/fire_qualification.txt -Frames 1520
```

The timeline report still fails on the documented earlier boot/setup differences;
it does not list every shot. The full traces retain the harpoon-active flag
(`$8401` bit 3) and timer (`$863E`) for inspection. Separate synthetic checks
cover immediate held fire, stable packet reads, reset and independent coin/start
consumption. The existing high-score test protects one letter per held press.

MAME's [current 51XX implementation](https://github.com/mamedev/mame/blob/mame0289/src/mame/namco/namco51.cpp)
runs the MCU firmware. Its [historical high-level model](https://github.com/mamedev/mame/blob/mame0150/src/mame/machine/namco51.c)
documents the relevant data commands and start-enable behavior. The qualification
rules here were checked against running MAME, rather than inferred from the
historical model.

## Limits

This is a focused input model. Credits are still processed on a host packet
read, and a pending bitmask cannot queue several presses of the same line while
the host is not polling. Coinage ratios and full autonomous MCU/output-register
behavior are not modeled. Raw/credit response mode retains the existing
game-specific `A1/C1/E1` control-register shortcut, so arbitrary data-command mode
changes and joystick-remap commands are not generally supported.

Direction handling and raw/switch-mode fire retain their existing implementation;
the raw-mode fire qualification rule has not been established. Held pumping was
preserved, but this replay does not test tethering and inflating an enemy.
Matching these input events does not establish complete MCU or whole-machine
equivalence.
