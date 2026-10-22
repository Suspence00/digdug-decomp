# Independent frame timeline

The timeline tool compares the native port with MAME from power-on. Unlike the
routine verifier, its reference runs an independent hardware model for the whole
scenario. It reports milestone frame differences and the first difference in
selected gameplay state. A passing comparison covers these observations only.

Use this as a diagnostic for unintended changes to gameplay rules and pacing.
Exact agreement is not a release requirement: prioritize visible bugs, controls,
and maintainability when deciding which differences deserve work. Boot duration
and individual frame offsets are useful evidence, not goals by themselves.

## Run it

Supply the supported Rev 2 ROMs in `roms/extracted`, plus a local MAME installation
and its Dig Dug ROM set, including the Namco 51XX/53XX firmware. These files are
not supplied by this project. The runner uses the existing local layout by default:

```powershell
pwsh -NoProfile -File tools/mame/run_timeline.ps1
```

For another installation:

```powershell
pwsh -NoProfile -File tools/mame/run_timeline.ps1 -MamePath C:\MAME\mame.exe -MameRomPath C:\MAME\roms
```

The runner builds `timeline`, captures 1,600 frames on each machine, and writes a
unique directory under `tools/mame/out/timeline/`. It preserves the input script,
Lua capture script/options, MAME version and logs, both traces, and `report.txt`.
Generated output is ignored by Git. It uses isolated configuration and NVRAM
directories, so runs do not consume or change your normal MAME high scores.

`-Script`, `-ScriptStart`, and `-Frames` select another replay and observation
window. Script lines use the existing `<buttons hex> <positive frame count>`
format, including `80` for two-player start. Blank lines and `#` comments are
accepted. Scripts must include coin, start and movement to exercise every event;
short or idle captures cannot establish a passing gameplay comparison.

## Conditions and frame numbering

- Both machines start at reset; native capture uses `new_unbooted`, bypassing the
  interactive launcher's automatic boot skip.
- Initial EAROM is exactly 64 zero bytes. The fixed native DIP constants are
  `66/DB`; MAME's corresponding physical switch values are `99/24`.
- A program-ROM fingerprint must agree before comparison: FNV-1a32 over CPU 0
  `$0000..$3FFF`, CPU 1 `$0000..$1FFF`, and CPU 2 `$0000..$0FFF`, in that order.
  This detects accidental ROM mismatches; it is not a cryptographic identity check.
- Frame 1 is the first completed frame. Input for frame N is applied before that
  frame executes. By default, script step zero starts before frame 700; its first
  60 neutral frames put the coin press at frame 760.
- MAME installs the capture at emulated time zero using `-autoboot_delay 0` and
  samples the [machine-frame notifier](https://docs.mamedev.org/luascript/ref-common.html).
  Every digital control is overridden to keep live input out of the replay.

MAME rebuilds its physical input cache before the Lua frame callback. The script
therefore queues overrides one poll ahead and verifies the actual `IN0/IN1` port
values for each upcoming frame. A requested press on frame 1 cannot be installed
after the initial poll; this and unsupported frontend direction combinations
produce an inconclusive result. Use a neutral first frame.

Traces now use `digdug-timeline-v2`. Version 1 requested inputs after the poll,
which made them physically arrive one frame later than their trace labels.
Old traces are rejected rather than compared with the corrected capture format.

Frame counts refer to each machine's video frames, not wall-clock milliseconds.
MAME's supported machine refresh is approximately 60.606 Hz; the interactive
native window is paced at 60 Hz. Native execution uses fixed step groups, with
audited initialization and I/O work paced by the board clock as described below.

## Read the result

The report separates absolute frame deltas from deltas relative to each machine's
start acceptance. Negative values mean the native event occurred earlier. This
helps distinguish boot drift from extra drift introduced during game setup.

- **0:** all required milestones and selected same-frame observations match.
- **1:** a milestone or selected state observation diverges.
- **2:** inconclusive, including missing events, incomplete captures, mismatched
  conditions or different applied input masks.
- **64:** malformed arguments/trace, missing files, or a capture/setup error.

The runner propagates these statuses. A diagnostic mismatch is expected while
the documented timing limitations remain; do not treat a generated report alone
as a passing check. To compare saved captures without MAME or ROM loading:

```powershell
cargo run --locked --release --bin timeline -- compare --native PATH\native.timeline --reference PATH\reference.timeline
```

The traces contain the complete 8 KB shared-RAM window after every frame, but the
comparator deliberately selects game status, credit, initialization, lives,
round and player observations. It excludes boot-test residue, stacks and attract
animation from the state comparison. This does not prove cycle scheduling,
every RAM byte, ordered bus effects, graphics or audio equivalence. The
[routine verifier](VERIFICATION.md) remains useful for locating translation errors.

## Measured baseline

On 2026-10-21, the corrected 1,600-frame replay against MAME 0.289 produced the
following result. Program fingerprint: `FAE4D33B`; all conditions above matched.
The comparison still returns **1** because boot, some setup state and movement
observations remain different. Coin/start acceptance and initialization/spawn
milestones now match on the same absolute frames.

| Milestone | Native frame | MAME frame | Native minus MAME | Delta relative to accepted start |
| --- | ---: | ---: | ---: | ---: |
| Title ready | 146 | 715 | -569 | — |
| Coin accepted | 762 | 762 | 0 | 0 |
| Start accepted | 828 | 828 | 0 | 0 |
| New-game setup begins | 829 | 829 | 0 | 0 |
| Round initialization begins | 854 | 854 | 0 | 0 |
| Entities spawned | 857 | 857 | 0 | 0 |
| First input movement | 1133 | 1134 | -1 | -1 |

Round initialization begins 26 frames after accepted start on both machines;
entities spawn after 29 frames on both. Previously the native port spawned after
only five frames, accelerating setup by 24 frames. The initial runner reported
a 26-frame absolute spawn lead; one frame was its input-cache measurement error,
and one came from missing coin/start qualification. Both sources of that extra
two-frame lead have been resolved in this replay.

The first selected state mismatch is now player one's reserve lives at `$840A`
on frame 850: native `03`, MAME `00`. This identifies an intermediate setup-state
difference; it does not by itself establish an incorrect final life count.
First movement occurs much later because the
replay waits before pressing a direction; it does not measure the earliest frame
movement is possible.

## Coin/start qualification replay

The [coin/start input model](NAMCO_INPUTS.md) was checked with one-frame pulses,
a one-frame release between coin presses, and two-player start:

```powershell
pwsh -NoProfile -File tools/mame/run_timeline.ps1 -Script tools/mame/input_qualification.txt
```

Native and MAME both change the game's credit latch `$85A5` at frames 768 (`01`),
776 (`02`), and 795 (`00`). Both accept start at 795, begin round initialization
at 821, and spawn at 824. The replay still returns **1**: title readiness differs,
the first selected setup-state difference is `$840A` at frame 817, and first
input movement occurs at native 1128 versus MAME 1125. The input fix is supported
by the matching credit events and dedicated regressions, not by a claim that the
whole-machine comparison passes.

## Audited initialization pacing

The board runs three 3.072 MHz CPUs and spans exactly 50,688 CPU T-states per
video frame ([MAME machine configuration](https://github.com/mamedev/mame/blob/mame0289/src/mame/namco/galaga.cpp#L1724)).
`src/timing.rs` uses that clock with a signed fractional remainder, preserving
elapsed work across frames without rounding every loop iteration. Other CPUs
and 06XX transfers continue while CPU 0 pays the duration of audited work.

The original `$005A` watchdog loop now consumes its 958,479 T-states, about
18.9 frames. Seven bulk initialization routines and the three 06XX NMI paths
also carry independently checked instruction costs. The `$0442` and `$044E`
continuations defer the IRQ-enabling epilogue until initialization work has
elapsed. ROM-dependent tests compare the new boundaries and timing against the
original instructions; a gameplay replay protects the measured 26/29-frame
durations without patching game state.

This is incremental timing support. Unaudited routines retain the legacy step
budget, grouped memory effects remain atomic, and the 06XX timer is still
approximate. This change does not establish full cycle accuracy or correct the
large boot self-test discrepancy.
