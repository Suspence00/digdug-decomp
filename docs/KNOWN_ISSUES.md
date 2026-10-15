# Known limitations

- **Timing is approximate.** The scheduler uses fixed execution-step counts and
  some translated routines group instructions. A local comparison reached the
  first-round spawn about 26 frames before MAME; the exact cause remains unresolved.
- **Coverage is incomplete.** Focused checks cover startup, movement, deaths,
  player handoffs and selected transitions. They do not establish every input
  sequence, late-round behavior or long-session stability.
- **Sound and visuals need human assessment.** Smoke checks confirm output is
  produced; pixel, waveform and original hardware timing equivalence are unproven.
- **Much of the source remains processor-oriented.** Movement geometry, turns,
  rock probes and tunnel contact have readable rules. Animation, speed scheduling,
  scoring and much of the wider lifecycle retain the original memory/register layout.

The [routine verifier](VERIFICATION.md) catches translation mistakes but shares
this port's hardware model. Passing it can miss errors in that shared model.
The fixes and regressions address identified bugs; they do not mean all bugs
have been found.
