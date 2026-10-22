# Coin and start inputs

`src/namco51.rs` models the observed coin/start qualification of the Namco 51XX.
It is sampled once per native video frame. Both press and release require two
consecutive matching samples. Repeated host reads do not advance qualification
or repeat an accepted press. A one-frame release between held coin presses does
not rearm the coin input.

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

The tracked `tools/mame/input_qualification.txt` replay reproduces short pulses,
release qualification and a two-player start. Its exact game credit-latch and
setup frames are covered by a ROM-dependent regression. Synthetic tests cover
independent lines, repeated reads, reset, command parsing and start-credit gating.
The existing lifecycle test also covers game over followed by another start.
See [timeline results and commands](MAME_TIMELINE.md#coinstart-qualification-replay).

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

Fire and direction handling retain their existing implementation. In particular,
this change does not fix fire-edge qualification. Matching coin/start events does
not establish complete MCU or whole-machine equivalence.
