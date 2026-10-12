use std::fs;
use std::path::{Path, PathBuf};

/// Complete decoded ROM and PROM assets for Dig Dug (Namco Rev 2).
#[derive(Clone)]
pub struct DigDugRoms {
    /// CPU 0 (Main data tables & ROM): 16 KB (0x0000..0x3FFF)
    pub cpu0: Vec<u8>,
    /// CPU 1 (Sub/AI data tables & ROM): 8 KB (0x0000..0x1FFF), padded to 16 KB
    pub cpu1: Vec<u8>,
    /// CPU 2 (Sound data tables & ROM): 4 KB (0x0000..0x0FFF), padded to 16 KB
    pub cpu2: Vec<u8>,

    /// Raw foreground 1bpp font/HUD ROM (dd1.9, 2048 bytes)
    pub gfx1_chars_raw: Vec<u8>,
    /// Raw 16x16 2bpp sprite ROMs (dd1.15, dd1.14, dd1.13, dd1.12, 16384 bytes)
    pub gfx2_sprites_raw: Vec<u8>,
    /// Raw 8x8 2bpp background dirt tile ROM (dd1.11, 4096 bytes)
    pub gfx3_bgtiles_raw: Vec<u8>,
    /// Playfield dirt map lookup ROM (dd1.10b, 4096 bytes = 4 playfields x 1024 bytes)
    pub gfx4_playfield: Vec<u8>,

    /// Master 32-entry RGB palette PROM (136007.113, 32 bytes) -> 0x00RRGGBB
    pub palette_rgb: [u32; 32],
    /// Sprite color lookup PROM (136007.111, 256 bytes = 64 groups x 4 pens -> RGB)
    pub sprite_clut_rgb: [[u32; 4]; 64],
    /// Sprite color lookup raw pen index (0..15; 15 is transparent)
    pub sprite_clut_pen: [[u8; 4]; 64],
    /// Tile color lookup PROM (136007.112, 256 bytes = 64 groups x 4 pens -> RGB)
    pub tile_clut_rgb: [[u32; 4]; 64],

    /// Namco WSG sound waveform PROM (136007.110, 256 bytes = 8 waveforms x 32 4-bit samples)
    pub sound_wavetable: [[u8; 32]; 8],

    /// Pre-decoded upright (ROT90) 8x8 foreground characters: 128 chars x 8 rows x 8 cols (0 or 1)
    pub chars_decoded: Vec<[[u8; 8]; 8]>,
    /// Pre-decoded upright (ROT90) 8x8 background dirt tiles: 256 tiles x 8 rows x 8 cols (0..3)
    pub bgtiles_decoded: Vec<[[u8; 8]; 8]>,
    /// Pre-decoded upright (ROT90) 16x16 sprites: [flip_bits * 256 + sprite_code][16 rows][16 cols] (0..3)
    pub sprites_decoded: Vec<[[u8; 16]; 16]>,
}

impl DigDugRoms {
    /// Locates the ROM directory by checking `base_dir`, `./roms`, `./`, and walking up from
    /// `std::env::current_exe()`. If `digdug.zip` is found next to the executable or in the
    /// working directory, it is automatically extracted into `roms/extracted/`.
    fn resolve_rom_dir(base_dir: &Path) -> PathBuf {
        let mut search_roots: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            search_roots.push(cwd);
        }
        if let Ok(exe_path) = std::env::current_exe() {
            let mut ancestor = exe_path.parent();
            while let Some(dir) = ancestor {
                if !search_roots.iter().any(|r| r == dir) {
                    search_roots.push(dir.to_path_buf());
                }
                ancestor = dir.parent();
            }
        }

        // 1. Check for already-extracted ROM files (`dd1a.1`)
        for root in &search_roots {
            for sub in [
                root.join(base_dir),
                root.join("roms").join("extracted"),
                root.join("roms"),
                root.clone(),
            ] {
                if sub.join("dd1a.1").exists() {
                    return sub;
                }
            }
        }

        // 2. Check for `digdug.zip` and auto-extract into `roms/extracted/`
        for root in &search_roots {
            for zip_candidate in [
                root.join("digdug.zip"),
                root.join("roms").join("digdug.zip"),
            ] {
                if zip_candidate.exists() {
                    let target_dir = root.join("roms").join("extracted");
                    if fs::create_dir_all(&target_dir).is_ok() {
                        // Use built-in `tar -xf` (available on Windows 10/11, macOS, and Linux)
                        let status = std::process::Command::new("tar")
                            .arg("-xf")
                            .arg(&zip_candidate)
                            .arg("-C")
                            .arg(&target_dir)
                            .status();
                        if status.map(|s| s.success()).unwrap_or(false)
                            && target_dir.join("dd1a.1").exists()
                        {
                            return target_dir;
                        }
                        #[cfg(windows)]
                        {
                            let ps_status = std::process::Command::new("powershell")
                                .arg("-NoProfile")
                                .arg("-Command")
                                .arg(format!(
                                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                                    zip_candidate.display(),
                                    target_dir.display()
                                ))
                                .status();
                            if ps_status.map(|s| s.success()).unwrap_or(false)
                                && target_dir.join("dd1a.1").exists()
                            {
                                return target_dir;
                            }
                        }
                    }
                }
            }
        }

        base_dir.to_path_buf()
    }

    pub fn load_from_extracted_dir(base_dir: &Path) -> Result<Self, String> {
        let resolved_dir = Self::resolve_rom_dir(base_dir);
        let read_chip = |rel: &str, expected_len: usize| -> Result<Vec<u8>, String> {
            let p: PathBuf = resolved_dir.join(rel);
            let data = fs::read(&p).map_err(|e| {
                format!(
                    "Could not find '{}' ({}).\n\nTo play Dig Dug, place your legally obtained 'digdug.zip' (or extracted ROM files in 'roms/extracted/') in the same folder as digdug.exe.",
                    p.display(),
                    e
                )
            })?;
            if data.len() != expected_len {
                return Err(format!(
                    "ROM chip '{}' has length {}, expected {}",
                    p.display(),
                    data.len(),
                    expected_len
                ));
            }
            Ok(data)
        };

        let mut cpu0 = vec![0xFFu8; 0x4000];
        cpu0[0x0000..0x1000].copy_from_slice(&read_chip("dd1a.1", 0x1000)?);
        cpu0[0x1000..0x2000].copy_from_slice(&read_chip("dd1a.2", 0x1000)?);
        cpu0[0x2000..0x3000].copy_from_slice(&read_chip("dd1a.3", 0x1000)?);
        cpu0[0x3000..0x4000].copy_from_slice(&read_chip("dd1a.4", 0x1000)?);

        let mut cpu1 = vec![0xFFu8; 0x4000];
        cpu1[0x0000..0x1000].copy_from_slice(&read_chip("dd1a.5", 0x1000)?);
        cpu1[0x1000..0x2000].copy_from_slice(&read_chip("dd1a.6", 0x1000)?);

        let mut cpu2 = vec![0xFFu8; 0x4000];
        cpu2[0x0000..0x1000].copy_from_slice(&read_chip("dd1.7", 0x1000)?);

        let gfx1_chars_raw = read_chip("dd1.9", 0x0800)?;

        let mut gfx2_sprites_raw = vec![0u8; 0x4000];
        gfx2_sprites_raw[0x0000..0x1000].copy_from_slice(&read_chip("dd1.15", 0x1000)?);
        gfx2_sprites_raw[0x1000..0x2000].copy_from_slice(&read_chip("dd1.14", 0x1000)?);
        gfx2_sprites_raw[0x2000..0x3000].copy_from_slice(&read_chip("dd1.13", 0x1000)?);
        gfx2_sprites_raw[0x3000..0x4000].copy_from_slice(&read_chip("dd1.12", 0x1000)?);

        let gfx3_bgtiles_raw = read_chip("dd1.11", 0x1000)?;
        let gfx4_playfield = read_chip("dd1.10b", 0x1000)?;

        // Decode 32-entry RGB palette PROM (136007.113) using exact resistor weights:
        // R: 1k (0x21), 470 (0x47), 220 (0x97)
        // G: 1k (0x21), 470 (0x47), 220 (0x97)
        // B: 470 (0x47), 220 (0x97)
        let prom_pal = read_chip("136007.113", 32)?;
        let mut palette_rgb = [0u32; 32];
        for i in 0..32 {
            let b = prom_pal[i];
            let r = 0x21 * ((b >> 0) & 1) as u32
                + 0x47 * ((b >> 1) & 1) as u32
                + 0x97 * ((b >> 2) & 1) as u32;
            let g = 0x21 * ((b >> 3) & 1) as u32
                + 0x47 * ((b >> 4) & 1) as u32
                + 0x97 * ((b >> 5) & 1) as u32;
            let bl = 0x47 * ((b >> 6) & 1) as u32 + 0x97 * ((b >> 7) & 1) as u32;
            palette_rgb[i] = (r << 16) | (g << 8) | bl;
        }

        // Sprite CLUT PROM (136007.111): 256 bytes = 64 color groups x 4 pens -> palette[16..32]
        let prom_spr_clut = read_chip("136007.111", 256)?;
        let mut sprite_clut_rgb = [[0u32; 4]; 64];
        let mut sprite_clut_pen = [[0u8; 4]; 64];
        for grp in 0..64 {
            for pen in 0..4 {
                let idx = (prom_spr_clut[grp * 4 + pen] & 0x0F) as usize;
                sprite_clut_pen[grp][pen] = idx as u8;
                sprite_clut_rgb[grp][pen] = palette_rgb[16 + idx];
            }
        }

        // Tile CLUT PROM (136007.112): 256 bytes = 64 color groups x 4 pens -> palette[0..16]
        let prom_tile_clut = read_chip("136007.112", 256)?;
        let mut tile_clut_rgb = [[0u32; 4]; 64];
        for grp in 0..64 {
            for pen in 0..4 {
                let idx = (prom_tile_clut[grp * 4 + pen] & 0x0F) as usize;
                tile_clut_rgb[grp][pen] = palette_rgb[idx];
            }
        }

        // Sound Wavetable PROM (136007.110): 8 waveforms x 32 4-bit samples
        let prom_snd = read_chip("136007.110", 256)?;
        let mut sound_wavetable = [[0u8; 32]; 8];
        for w in 0..8 {
            for s in 0..32 {
                sound_wavetable[w][s] = prom_snd[w * 32 + s] & 0x0F;
            }
        }

        // Pre-decode 128 8x8 foreground characters (ROT90 upright)
        let mut chars_decoded = vec![[[0u8; 8]; 8]; 128];
        for ch in 0..128 {
            let slice = &gfx1_chars_raw[ch * 8..(ch + 1) * 8];
            for y in 0..8 {
                for x in 0..8 {
                    let byte = slice[7 - x];
                    chars_decoded[ch][y][x] = if (byte & (1 << y)) != 0 { 1 } else { 0 };
                }
            }
        }

        // Pre-decode 256 8x8 2bpp background dirt tiles (ROT90 upright)
        let mut bgtiles_decoded = vec![[[0u8; 8]; 8]; 256];
        for t in 0..256 {
            let slice = &gfx3_bgtiles_raw[t * 16..(t + 1) * 16];
            for y in 0..8 {
                for x in 0..8 {
                    let byte = slice[15 - x - 2 * (y & 4)];
                    let c0 = if (byte & (0x08 >> (y & 3))) != 0 { 1 } else { 0 };
                    let c1 = if (byte & (0x80 >> (y & 3))) != 0 { 2 } else { 0 };
                    bgtiles_decoded[t][y][x] = c0 + c1;
                }
            }
        }

        // Pre-decode 256 16x16 2bpp sprites in 4 flip orientations (ROT90 upright)
        // index = flip_bits * 256 + sprite_code
        let mut sprites_decoded = vec![[[0u8; 16]; 16]; 4 * 256];
        for s in 0..256 {
            let slice = &gfx2_sprites_raw[s * 64..(s + 1) * 64];
            let mut base = [[0u8; 16]; 16];
            for y in 0..16 {
                for x in 0..16 {
                    let idx = ((y & 8) << 1) + (((x & 8) ^ 8) << 2) + (7 - (x & 7)) + 2 * (y & 4);
                    let c0 = if (slice[idx] & (0x08 >> (y & 3))) != 0 { 1 } else { 0 };
                    let c1 = if (slice[idx] & (0x80 >> (y & 3))) != 0 { 2 } else { 0 };
                    base[y][x] = c0 + c1;
                }
            }
            for flip_bits in 0..4 {
                let flip_y = (flip_bits & 1) != 0;
                let flip_x = (flip_bits & 2) != 0;
                for y in 0..16 {
                    let sy = if flip_y { 15 - y } else { y };
                    for x in 0..16 {
                        let sx = if flip_x { 15 - x } else { x };
                        sprites_decoded[flip_bits * 256 + s][y][x] = base[sy][sx];
                    }
                }
            }
        }

        Ok(Self {
            cpu0,
            cpu1,
            cpu2,
            gfx1_chars_raw,
            gfx2_sprites_raw,
            gfx3_bgtiles_raw,
            gfx4_playfield,
            palette_rgb,
            sprite_clut_rgb,
            sprite_clut_pen,
            tile_clut_rgb,
            sound_wavetable,
            chars_decoded,
            bgtiles_decoded,
            sprites_decoded,
        })
    }
}
