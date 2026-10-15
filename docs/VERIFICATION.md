# Routine verification

The existing verifier is a diagnostic for translation errors. It runs the native
Rust step and a separate Z80 interpreter from the same initial state, then compares
their endpoints. It requires your own supported ROMs in `roms/extracted`.

```sh
cargo run --locked --release --bin verify_decomp -- --frames 2000 --script tools/mame/inputscript.txt --strict-registers
```

`--help` lists the options without loading ROMs. Scripts contain a hexadecimal
button mask and positive frame duration per line; comments start with `#`.
The default script start is frame 200. The reference allowance defaults to
10,000,000 steps per boundary so startup self-tests can finish.

## Results

- **0:** compared endpoints pass within the verifier's scope.
- **1:** a state/register divergence was detected under the selected mode.
- **2:** the run is inconclusive, including timeouts, unsupported instructions,
  early reference HALTs, unresolved register differences or an empty run.
- **64:** invalid arguments, missing input or another run error.

`--strict-registers` rejects every compared register mismatch. Without it,
register-only mismatches may be sampled for onward execution. They are accepted
only if the compared registers and state actually converge; unexamined samples
remain inconclusive. Intermediate state differences during continuation are
checked before a later write can hide them.

`--report-only` prints the actual outcome but exits zero after a completed
run. It is for exploration; do not use it as a passing check. Invalid arguments
and run errors still fail.

## Limits

- Both paths share the same hardware bus. Shared hardware errors and cumulative
  full-machine drift can escape comparison.
- Boundaries come from the native step's resulting PC/SP. Nested helpers and
  paths never exercised by a run are not individually established.
- RAM in a 96-byte window below SP is ignored as a stack-residue heuristic.
  This is not proof that those bytes are irrelevant to all CPUs.
- Undocumented flag bits X/Y, refresh register R, instruction counts and cycle
  counts are excluded. General ordered memory/I/O effects are not compared.
- Sampled continuations execute one CPU without scheduling other CPUs, inputs
  or interrupts. They do not establish arcade timing.
- RAM self-tests summarize the healthy-RAM path; intermediate faulty-RAM behavior
  is not modeled. Program-ROM checksum paths do use the supplied program bytes.

A passing result means the observed scenario passes these checks. It does not
establish complete decompilation, every gameplay path, or original video/audio
and hardware equivalence. See [known limitations](KNOWN_ISSUES.md).
