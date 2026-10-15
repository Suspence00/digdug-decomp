# Player code guide

The movement rules have been extracted from the existing ROM-derived translation.
They are not an independent reimplementation of the game.

| File | Responsibility |
| --- | --- |
| `src/player.rs` | Positions, decoded directions, turn alignment, bounds, rock probes and two-pixel steps |
| `src/player/tunnels.rs` | Tile addresses, leading-edge sampling and dirt contact |
| `src/player/legacy.rs` | CPU register/flag and memory adapters for the extracted rules |
| `src/decomp.rs` | Dispatch, speed/animation, tile painting and the wider player lifecycle |

A movement request follows these rules:

1. Decode facing from the latch's low nibble: `0/2/4/6` mean up/right/down/left;
   `8` is neutral. These codes differ from controller button bits.
2. Check turn alignment. Vertical movement needs a column center. Horizontal
   movement needs a row center, with the surface tunnel handled separately.
   Rejected turns preserve the previous facing.
3. Probe one tile ahead for a rock and sample the leading-edge tile footprint.
   Dirt contact distinguishes digging from movement through an existing tunnel.
4. When the legacy speed/animation timer allows movement, attempt a complete
   two-pixel step. An out-of-bounds candidate restores the old position rather
   than clamping it to an edge.

`legacy.rs` preserves processor details that are still visible to callers:
register/flag results, direction-latch flags, and the original coordinate-write
sequence, including rollback. Keep those effects intact when changing a rule.

Synthetic rule tests run with the normal test command. The optional ROM-based
`tests/player_movement.rs` comparisons exercise boundary coordinates, direction
flags, rock slots, dirt patterns and coordinate-write order. They use this
project's interpreter and hardware model, so they do not establish independent
arcade timing or correctness of the entire player system.
