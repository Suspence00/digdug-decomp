//! Synthetic data for tests; no arcade ROM or PROM bytes are included.
use crate::rom::DigDugRoms;

pub(crate) fn roms(program: &[u8]) -> DigDugRoms {
    let mut cpu0 = vec![0; 0x4000];
    cpu0[..program.len()].copy_from_slice(program);
    DigDugRoms {
        cpu0,
        cpu1: vec![0; 0x4000],
        cpu2: vec![0; 0x4000],
        gfx1_chars_raw: vec![],
        gfx2_sprites_raw: vec![],
        gfx3_bgtiles_raw: vec![],
        gfx4_playfield: vec![],
        palette_rgb: [0; 32],
        sprite_clut_rgb: [[0; 4]; 64],
        sprite_clut_pen: [[0; 4]; 64],
        tile_clut_rgb: [[0; 4]; 64],
        sound_wavetable: [[0; 32]; 8],
        chars_decoded: vec![],
        bgtiles_decoded: vec![],
        sprites_decoded: vec![],
    }
}
