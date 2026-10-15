# Contributing

Help make the native port easier to play, build and understand. Good contributions
include reproducible gameplay bug reports, small fixes and clearer game logic.
See the [known limitations](docs/KNOWN_ISSUES.md) before assuming arcade accuracy.

## Build and check

Install stable Rust. On Debian/Ubuntu, the window dependency also needs
`libx11-dev`, `libxkbcommon-dev`, `libxi-dev` and `libxcursor-dev`.

```sh
cargo test --locked --all-targets
```

These tests use synthetic data and need no arcade ROMs. Build and play with
`cargo run --locked --release --bin digdug` after following the README's ROM setup.

With your own supported ROMs, run the gameplay and translation regressions:

```sh
cargo test --locked --release --all-targets -- --include-ignored
```

ROM-dependent tests are ignored by default. Gameplay scenarios exercise deaths,
alternating players through game over, restarting and a staged round transition.
Player comparisons protect the extracted movement rules and processor boundary.
For a targeted translation investigation, use the [routine verifier](docs/VERIFICATION.md).

## Make a focused change

Explain what happened, what you expected, and the controls that reproduce it.
Mention the operating system and supported ROM set used. For a fix, describe the
behavior changed and how you checked it. Keep a small regression when it protects
a real bug; documentation and simple reversible edits do not need new tests.

When extracting translated game logic, preserve memory/register effects at its
existing boundary. The [player code guide](docs/PLAYER_SYSTEM.md) explains this
separation. Unknown addresses should remain explicit failures. State clearly
when a test stages memory instead of reaching a state through normal controls.

Keep ROM archives, extracted chips, graphics/audio dumps, emulator binaries and
private toolchains out of contributions. Generated disassembly listings and local
release binaries are excluded by the repository's ignore rules. Describe source
origins and remaining uncertainties accurately.
