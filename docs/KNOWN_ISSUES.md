# Known limitations

- **Timing is partly audited.** The watchdog delay and selected initialization
  routines now use board-clock pacing. The [frame timeline](MAME_TIMELINE.md#measured-baseline)
  matches first-round initialization/spawn durations relative to accepted start.
  Boot self-tests remain substantially faster, and the replay still shows a
  one-frame absolute credit/start offset and a movement difference. Unaudited
  routines and the 06XX timer retain approximate scheduling.
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
