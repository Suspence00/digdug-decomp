# Testing with a recorded game

A replay is a recording of button presses. We can feed those buttons into this
port and the original arcade program, then compare what happens. A small timing
difference can make the recorded player miss a turn and eventually die. This is
useful for finding differences, even when normal human play remains playable.

## Recording and reference result

We used negative1's public **1,058,800-point Dig Dug Rev 2** recording from
[MARP](http://replay.marpirc.net/index.cgi?short=%5Edigdug%24&mode=search&per_game=999).
It is a human input recording, rather than a TAS, and serves the same testing
purpose. No game ROMs are included or downloaded by these tools.

- [Recording archive](http://replay.marpirc.net/inp/7/c/d/rg1_digdug_1058800_wolf170.zip):
  `rg1_digdug_1058800_wolf170.inp` inside the archive.
- SHA-256 of the extracted INP:
  `b8efa6a89d270b640fec14ecc3b1cbd8471c39e1002f2ee9484838afd538ba69`.
- Matching player: [official WolfMAME 0.170](https://github.com/mahlemiut/wolfmame/releases/tag/wolf170).
- Duration: **171,521 input frames, about 47 minutes 10 seconds**.
- Recorded physical DIP switches: **A1/24**, with extra lives at 10K, 40K and
  every further 40K. The port's normal physical settings are 99/24.

The matching WolfMAME player consumed all input frames, reached **round 72**,
reproduced **1,058,800 points** including score rollover, then reached game over.
A read-only Lua observer checked game RAM; controls were supplied by the INP.
The original ROMs were the user's existing set.

Modern MAME 0.289 consumed only one input frame from this old file before running
its title-screen demo. Its successful process exit did **not** establish replay
compatibility. Use the recording's matching player when checking its outcome.

## Import and run

Keep downloaded recordings, emulators and results under `tools/mame/out/`, which
is ignored by Git. After extracting the INP there, convert it using Python 3:

```sh
python tools/mame/import_wolf170.py tools/mame/out/rg1_digdug_1058800_wolf170.inp --output tools/mame/out/rg1-inputs.txt --metadata tools/mame/out/rg1-inputs.json
```

The importer supports the specific Dig Dug / Wolf170 eight-port input layout.
It checks the header, compressed stream, timestamps, cabinet switches and every
control sample before writing output. Other MAME versions and unsupported
controls are rejected. The manifest records the source hash and switch settings.
This validates input extraction; it does not compare gameplay.

With your own ROMs installed in `roms/extracted`, run the full converted script:

```sh
cargo run --locked --release --bin replay -- --script tools/mame/out/rg1-inputs.txt --dip-a A1 --dip-b 24 --output tools/mame/out/rg1-native.csv
```

The runner starts from power-on with zero EAROM, applies the first input before
frame 1 and streams a compact CSV of player state. The CSV path must be new.
Use `--frames 7200` for a shorter investigation, or `--help` for all options.
Do not shift the script to compensate for a gameplay difference: joystick input
latches were independently checked against Wolf170 and have matching timing.

## What the native run establishes

The native run with matching A1/24 settings completed all **171,521
frames without a crash**. Its actual game lasted frames **915–8843**:
**7,929 in-game frames, round 2, 6,200 points and three death animations**.
The remaining frames ran during the title-screen demo. This establishes long
execution stability for this input, **not** 72 rounds of native gameplay.

Player coordinates match the reference at every captured frame from 1200
through 4341. Smaller temporary score and enemy-state differences occur earlier,
so this does not establish complete first-round equivalence.

| Event | Native frame | Wolf170 frame |
| --- | ---: | ---: |
| Round 1 cleared | 4194 | 4195 |
| Round 2 initialization begins | 4339 | 4340 |
| Player position resets | 4342 | 4344 |
| READY countdown begins | 4343 | 4345 |
| First round 2 movement | 4464 | 4467 |

Both machines receive the same direction inputs. Starting movement three frames
early changes which grid positions the player occupies when a turn is pressed.
That explains how this fixed recording can lose its route despite matching
controls. It does not identify every cause of the timing difference.

The investigation found two omitted original setup costs: `$188D` takes 5,410
Z80 cycles and `$18A7` takes 1,482. Independent ROM instruction checks now cover
both. Their total is only about 0.136 frame; adding these costs does not establish
that the complete transition difference is fixed.

Round setup now waits for those measured costs before its interrupt-enabling
tail runs. Checking that boundary also exposed a missing register clear, which
now matches the original instruction. Both player branches have ROM boundary
checks. These timing corrections have not resolved the measured transition
difference.

The longer diagnostic also found a reversed condition in rock/tunnel handling:
the port capped tunnel tiles when the original program left them alone. The
condition and working register results are corrected, with 784 ROM boundary
cases and a replay of the exact failing frame. Six title-screen demo setup
routines also used incorrect sprite ranges or skipped original setup work.
Their corrections pass 68 ROM comparisons, including complete frame handlers.
An enemy's vertical exit from ghost movement also returned before its original
cleanup/retry step. Seven full-handler ROM cases now cover its boundaries, and
the captured failing frame also passes after the correction.

The routine verifier now accepts the same physical switch settings and 1-based
script frames as the replay runner. See [verification](VERIFICATION.md) for
its limits and the old frame-number conversion.

After all corrections, the complete native recording still reaches round 2 and
6,200 points. The measured player route and transition table above are unchanged.
The corrected demo setup changes recorded state after game over. Terrain and
enemy working flags are checked by the ROM regressions and routine verifier;
the compact CSV does not contain all of that state.

## Checked segment

The first **5,400 frames from power-on**, using the recording's A1/24 settings
and script start 1, passed strict routine verification: **321,429,582 steps
compared, zero mismatches, timeouts, early reference halts or unsupported
instructions**. Reproduce that diagnostic with:

```sh
cargo run --locked --release --bin verify_decomp -- --frames 5400 --script tools/mame/out/rg1-inputs.txt --script-start 1 --dip-a A1 --dip-b 24 --strict-registers
```

This checks observed translated routine endpoints against original instructions
on the same hardware model. It does not establish whole-machine timing or all
47 minutes of gameplay equivalence. Local validation also passed all 108 Rust
tests with the supplied ROMs, 10 synthetic importer tests, and the game smoke check.

The runner prints actual gameplay coverage separately from input duration and
ignores inactive-player and title-screen values in round/score totals. Exit 0
means execution completed. It is not a reference-equivalence pass.

The next useful comparison is the round-clear and round-setup transition,
including when CPU 0 releases CPU 1 to move. Full replay synchronization remains
unresolved. See [known limitations](KNOWN_ISSUES.md) and
[frame timeline](MAME_TIMELINE.md) for the broader timing limits.
