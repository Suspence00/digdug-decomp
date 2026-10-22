# Pumping enemies

A confirmed press launches one harpoon. Once it hooks an enemy, keep holding
Pump to inflate it until it pops. Holding a direction while still pumping keeps
the player attached and stationary. Release Pump and move to break away.

An enemy that is released before popping gradually shrinks and can resume
walking. Releasing Pump without moving can leave the cable attached while this
happens. A popped enemy awards points once, then disappears and clears the
cable. Holding the button does not automatically launch another harpoon.

## What is checked

The two input scripts reach a first-round Pooka encounter through normal play.
They insert two credits, start one player, dig right and down, then fire into
the enemy's tunnel. Neither script patches game RAM.

| Scenario | Protected behavior |
| --- | --- |
| Keep pumping | Inflation progresses through stages 1, 2, 3 and 4; the enemy pops, awards 400 points once, and clears the cable. |
| Release and move | The player detaches and moves; the enemy survives, deflates and recovers without awarding pop points. Digging may still score. |
| Release while standing still | Inflation stops; the attached enemy shrinks, recovers and frees the tether without a pop award. |
| Hold Pump and a direction | The player stays attached and stationary while inflation continues; releasing Pump then permits movement. |

Every gameplay check also requires the player to remain alive and preserves
the unused credit. The tests check actual enemy state, score, player movement
and cable cleanup. They do not just check whether the fire button was read.

Run them with your own supported ROMs installed:

```sh
cargo test --locked --release --test pump_gameplay -- --include-ignored
```

The same approach and hold/release outcomes were independently checked in MAME
0.289. To capture those scenes:

```powershell
pwsh -NoProfile -File tools/mame/run_timeline.ps1 -Script tools/mame/pump_interaction.txt -Frames 1800
pwsh -NoProfile -File tools/mame/run_timeline.ps1 -Script tools/mame/pump_release.txt -Frames 1800
```

Native hooks this enemy two frames earlier in the measured replay. The gameplay
tests protect the outcomes above without requiring exact event frames. The
timeline comparison still reports the known boot, setup and movement differences;
its first reported mismatch does not summarize the whole pump interaction. Full
traces include the target pointer at `$87C8`, player tether at `$8403` bit 2,
monster slots at `$8525` with stride 16, and score state.

## Translation checks

Separate ROM-dependent boundary checks exercise pump phases, inflation stages,
enemy types, depth-dependent scoring and tether release. These start from an
active-game bus and explicitly stage the branch under test. They compare both
memory and supported processor registers with the original instructions. Unlike
the natural replays, they are focused fixtures, not complete gameplay sessions.

This work corrected the pump-stage routine's saved deflation value, timer-address
register and depth-selector flags. These were internal translation differences;
the natural scenarios already had the correct visible outcomes. Boundary tests
now protect the state that later code can depend on.

The test stack stays inside the original CPUs' stack areas. In particular, a
synthetic return address must not overwrite `$9A81`: doing so disables normal
score updates and can hide a scoring defect.

This covers one natural Pooka encounter and selected routine branches. It does
not establish every enemy encounter, late-round difficulty, Fygar flame
interaction, rock collision, or whole-machine timing. See the
[input model](NAMCO_INPUTS.md) and [known limitations](KNOWN_ISSUES.md).
