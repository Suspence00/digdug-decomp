//! Player contact with the tile grid. Tile painting remains in the legacy adapter.
use super::Position;

pub const UNDUG_DIRT: u8 = 0x7F;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtContact {
    pub undug_tiles: u8,
}

impl DirtContact {
    pub fn from_tiles(tiles: [u8; 4]) -> Self {
        Self { undug_tiles: tiles.iter().filter(|&&tile| tile == UNDUG_DIRT).count() as u8 }
    }

    pub fn in_existing_tunnel(self) -> bool { self.undug_tiles == 0 }
}

/// Sample toward the leading edge for down/right (including the legacy handling
/// of unknown codes), retaining byte wraparound at diagnostic coordinates.
pub fn sampling_position(position: Position, latch: u8) -> Position {
    if matches!(latch & 0x0F, 0 | 6) { position } else {
        Position { x: position.x.wrapping_add(6), y: position.y.wrapping_add(6) }
    }
}

pub fn tile_address(position: Position) -> u16 {
    let column = u16::from(position.x.wrapping_sub(0x11) >> 3);
    let row = u16::from(position.y >> 3);
    0x83A0u16.wrapping_sub(column * 32).wrapping_add(row)
}

/// The four tiles around a pixel sample, in the original pointer order.
pub fn footprint(position: Position) -> [u16; 4] {
    let first = tile_address(position);
    let next_column = first.wrapping_sub(32);
    [first, next_column, first.wrapping_add(1), next_column.wrapping_add(1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_cells_follow_the_rotated_column_major_tilemap() {
        assert_eq!(tile_address(Position { x: 0x11, y: 0 }), 0x83A0);
        assert_eq!(tile_address(Position { x: 0x19, y: 8 }), 0x8381);
        assert_eq!(footprint(Position { x: 0x71, y: 0x80 }), [0x8230, 0x8210, 0x8231, 0x8211]);
    }

    #[test]
    fn fresh_dirt_and_existing_tunnel_have_different_contact_states() {
        assert!(DirtContact::from_tiles([0, 0x8C, 0x0C, 0x7E]).in_existing_tunnel());
        assert_eq!(DirtContact::from_tiles([0x7F, 0, 0x7F, 0]).undug_tiles, 2);
    }

    #[test]
    fn leading_edge_sample_wraps_and_ignores_latch_flags() {
        let position = Position { x: 254, y: 255 };
        assert_eq!(sampling_position(position, 0x82), Position { x: 4, y: 5 });
        assert_eq!(sampling_position(position, 0x86), position);
    }
}
