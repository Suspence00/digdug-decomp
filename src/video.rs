use crate::decomp::{MonsterStatus, RockStatus};
use crate::hardware::DigDugMachine;
use crate::rom::DigDugRoms;

pub const ARCADE_WIDTH: usize = 224;
pub const ARCADE_HEIGHT: usize = 288;
pub const ARCADE_SCALE: usize = 3;

pub const ARCADE_WINDOW_WIDTH: usize = 720;
pub const TELEMETRY_WINDOW_WIDTH: usize = 1420;
pub const WINDOW_WIDTH: usize = TELEMETRY_WINDOW_WIDTH;
pub const WINDOW_HEIGHT: usize = 912;

/// Exact 36x28 tilemap address lookup table (`$8000 + TILEADDR[row][col]`)
/// mapping the 32x32 Namco VRAM layout onto the 224x288 (28 cols x 36 rows) upright screen.
pub const TILEADDR: [[u16; 28]; 36] = [
    [989, 988, 987, 986, 985, 984, 983, 982, 981, 980, 979, 978, 977, 976, 975, 974, 973, 972, 971, 970, 969, 968, 967, 966, 965, 964, 963, 962],
    [1021, 1020, 1019, 1018, 1017, 1016, 1015, 1014, 1013, 1012, 1011, 1010, 1009, 1008, 1007, 1006, 1005, 1004, 1003, 1002, 1001, 1000, 999, 998, 997, 996, 995, 994],
    [928, 896, 864, 832, 800, 768, 736, 704, 672, 640, 608, 576, 544, 512, 480, 448, 416, 384, 352, 320, 288, 256, 224, 192, 160, 128, 96, 64],
    [929, 897, 865, 833, 801, 769, 737, 705, 673, 641, 609, 577, 545, 513, 481, 449, 417, 385, 353, 321, 289, 257, 225, 193, 161, 129, 97, 65],
    [930, 898, 866, 834, 802, 770, 738, 706, 674, 642, 610, 578, 546, 514, 482, 450, 418, 386, 354, 322, 290, 258, 226, 194, 162, 130, 98, 66],
    [931, 899, 867, 835, 803, 771, 739, 707, 675, 643, 611, 579, 547, 515, 483, 451, 419, 387, 355, 323, 291, 259, 227, 195, 163, 131, 99, 67],
    [932, 900, 868, 836, 804, 772, 740, 708, 676, 644, 612, 580, 548, 516, 484, 452, 420, 388, 356, 324, 292, 260, 228, 196, 164, 132, 100, 68],
    [933, 901, 869, 837, 805, 773, 741, 709, 677, 645, 613, 581, 549, 517, 485, 453, 421, 389, 357, 325, 293, 261, 229, 197, 165, 133, 101, 69],
    [934, 902, 870, 838, 806, 774, 742, 710, 678, 646, 614, 582, 550, 518, 486, 454, 422, 390, 358, 326, 294, 262, 230, 198, 166, 134, 102, 70],
    [935, 903, 871, 839, 807, 775, 743, 711, 679, 647, 615, 583, 551, 519, 487, 455, 423, 391, 359, 327, 295, 263, 231, 199, 167, 135, 103, 71],
    [936, 904, 872, 840, 808, 776, 744, 712, 680, 648, 616, 584, 552, 520, 488, 456, 424, 392, 360, 328, 296, 264, 232, 200, 168, 136, 104, 72],
    [937, 905, 873, 841, 809, 777, 745, 713, 681, 649, 617, 585, 553, 521, 489, 457, 425, 393, 361, 329, 297, 265, 233, 201, 169, 137, 105, 73],
    [938, 906, 874, 842, 810, 778, 746, 714, 682, 650, 618, 586, 554, 522, 490, 458, 426, 394, 362, 330, 298, 266, 234, 202, 170, 138, 106, 74],
    [939, 907, 875, 843, 811, 779, 747, 715, 683, 651, 619, 587, 555, 523, 491, 459, 427, 395, 363, 331, 299, 267, 235, 203, 171, 139, 107, 75],
    [940, 908, 876, 844, 812, 780, 748, 716, 684, 652, 620, 588, 556, 524, 492, 460, 428, 396, 364, 332, 300, 268, 236, 204, 172, 140, 108, 76],
    [941, 909, 877, 845, 813, 781, 749, 717, 685, 653, 621, 589, 557, 525, 493, 461, 429, 397, 365, 333, 301, 269, 237, 205, 173, 141, 109, 77],
    [942, 910, 878, 846, 814, 782, 750, 718, 686, 654, 622, 590, 558, 526, 494, 462, 430, 398, 366, 334, 302, 270, 238, 206, 174, 142, 110, 78],
    [943, 911, 879, 847, 815, 783, 751, 719, 687, 655, 623, 591, 559, 527, 495, 463, 431, 399, 367, 335, 303, 271, 239, 207, 175, 143, 111, 79],
    [944, 912, 880, 848, 816, 784, 752, 720, 688, 656, 624, 592, 560, 528, 496, 464, 432, 400, 368, 336, 304, 272, 240, 208, 176, 144, 112, 80],
    [945, 913, 881, 849, 817, 785, 753, 721, 689, 657, 625, 593, 561, 529, 497, 465, 433, 401, 369, 337, 305, 273, 241, 209, 177, 145, 113, 81],
    [946, 914, 882, 850, 818, 786, 754, 722, 690, 658, 626, 594, 562, 530, 498, 466, 434, 402, 370, 338, 306, 274, 242, 210, 178, 146, 114, 82],
    [947, 915, 883, 851, 819, 787, 755, 723, 691, 659, 627, 595, 563, 531, 499, 467, 435, 403, 371, 339, 307, 275, 243, 211, 179, 147, 115, 83],
    [948, 916, 884, 852, 820, 788, 756, 724, 692, 660, 628, 596, 564, 532, 500, 468, 436, 404, 372, 340, 308, 276, 244, 212, 180, 148, 116, 84],
    [949, 917, 885, 853, 821, 789, 757, 725, 693, 661, 629, 597, 565, 533, 501, 469, 437, 405, 373, 341, 309, 277, 245, 213, 181, 149, 117, 85],
    [950, 918, 886, 854, 822, 790, 758, 726, 694, 662, 630, 598, 566, 534, 502, 470, 438, 406, 374, 342, 310, 278, 246, 214, 182, 150, 118, 86],
    [951, 919, 887, 855, 823, 791, 759, 727, 695, 663, 631, 599, 567, 535, 503, 471, 439, 407, 375, 343, 311, 279, 247, 215, 183, 151, 119, 87],
    [952, 920, 888, 856, 824, 792, 760, 728, 696, 664, 632, 600, 568, 536, 504, 472, 440, 408, 376, 344, 312, 280, 248, 216, 184, 152, 120, 88],
    [953, 921, 889, 857, 825, 793, 761, 729, 697, 665, 633, 601, 569, 537, 505, 473, 441, 409, 377, 345, 313, 281, 249, 217, 185, 153, 121, 89],
    [954, 922, 890, 858, 826, 794, 762, 730, 698, 666, 634, 602, 570, 538, 506, 474, 442, 410, 378, 346, 314, 282, 250, 218, 186, 154, 122, 90],
    [955, 923, 891, 859, 827, 795, 763, 731, 699, 667, 635, 603, 571, 539, 507, 475, 443, 411, 379, 347, 315, 283, 251, 219, 187, 155, 123, 91],
    [956, 924, 892, 860, 828, 796, 764, 732, 700, 668, 636, 604, 572, 540, 508, 476, 444, 412, 380, 348, 316, 284, 252, 220, 188, 156, 124, 92],
    [957, 925, 893, 861, 829, 797, 765, 733, 701, 669, 637, 605, 573, 541, 509, 477, 445, 413, 381, 349, 317, 285, 253, 221, 189, 157, 125, 93],
    [958, 926, 894, 862, 830, 798, 766, 734, 702, 670, 638, 606, 574, 542, 510, 478, 446, 414, 382, 350, 318, 286, 254, 222, 190, 158, 126, 94],
    [959, 927, 895, 863, 831, 799, 767, 735, 703, 671, 639, 607, 575, 543, 511, 479, 447, 415, 383, 351, 319, 287, 255, 223, 191, 159, 127, 95],
    [29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2],
    [61, 60, 59, 58, 57, 56, 55, 54, 53, 52, 51, 50, 49, 48, 47, 46, 45, 44, 43, 42, 41, 40, 39, 38, 37, 36, 35, 34],
];

// Sleek Modern Minimalist Warm/Neutral Slate Palette (High-Contrast Readability)
const BG_OUTER: u32 = 0x141417;
const CARD_TOP: u32 = 0x232329;
const CARD_BOT: u32 = 0x1A1A20;
const CARD_BORDER: u32 = 0x383842;
const CARD_HIGHLIGHT: u32 = 0x484854;

const TEXT_PRIMARY: u32 = 0xFAFAFC;
const TEXT_SECONDARY: u32 = 0xD4D4DC;
const TEXT_MUTED: u32 = 0x9E9EAE;

const BADGE_EMERALD_BG: u32 = 0x143629;
const BADGE_EMERALD_FG: u32 = 0x4ADE80;
const BADGE_AMBER_BG: u32 = 0x3B2A14;
const BADGE_AMBER_FG: u32 = 0xFBBF24;
const BADGE_SLATE_BG: u32 = 0x2E2E38;
const BADGE_SLATE_FG: u32 = 0xF4F4F6;
const BADGE_ROSE_BG: u32 = 0x3F1A24;
const BADGE_ROSE_FG: u32 = 0xFB7185;

/// Renders the 224x288 upright Dig Dug arcade framebuffer from VRAM + Playfield PROM + 64 Sprites.
pub fn render_arcade_frame(
    shared_ram: &[u8; 0x2000],
    video_latch: u8,
    roms: &DigDugRoms,
    arcade_fb: &mut [u32; ARCADE_WIDTH * ARCADE_HEIGHT],
) {
    let bg_select = (video_latch & 0x03) as usize;
    let tx_color_mode = (video_latch & 0x04) != 0;
    let bg_disable = (video_latch & 0x08) != 0;
    let bg_color_bank = (video_latch & 0x30) as usize;

    // 1. Render 36x28 background dirt tiles + foreground HUD/tunnel 8x8 characters
    for row in 0..36 {
        for col in 0..28 {
            let vram_addr = TILEADDR[row][col] as usize;

            // Background playfield layer (`dd1.10b` + `dd1.11` + `136007.112`)
            let bg_chr = roms.gfx4_playfield[vram_addr + bg_select * 0x400] as usize;
            let bg_color_group = if bg_disable {
                0x0F
            } else {
                (bg_chr >> 4) & 0x0F
            };
            let bg_colors = &roms.tile_clut_rgb[(bg_color_bank + bg_color_group) & 0x3F];
            let bg_tile = &roms.bgtiles_decoded[bg_chr];

            // Foreground 1bpp character layer (`$8000..$83FF` + `dd1.9`)
            let fg_chr = shared_ram[vram_addr] as usize;
            let fg_tile = &roms.chars_decoded[fg_chr & 0x7F];
            let fg_pal_idx = if tx_color_mode {
                fg_chr & 0x0F
            } else {
                ((fg_chr >> 4) & 0x0E) | ((fg_chr >> 3) & 0x02)
            };
            let fg_rgb = roms.palette_rgb[fg_pal_idx & 0x1F];

            let base_y = row * 8;
            let base_x = col * 8;
            for py in 0..8 {
                let line_offset = (base_y + py) * ARCADE_WIDTH + base_x;
                for px in 0..8 {
                    let mut rgb = if bg_disable {
                        0x000000
                    } else {
                        bg_colors[(bg_tile[py][px] & 3) as usize]
                    };
                    if fg_tile[py][px] != 0 {
                        rgb = fg_rgb;
                    }
                    arcade_fb[line_offset + px] = rgb;
                }
            }
        }
    }

    // 2. Render 64 hardware sprites ($8B80..$8BFF code/color, $9380..$93FF pos, $9B80..$9BFF flip/size)
    // Following Galagino/MAME upright transformation:
    for idx in 0..64 {
        let off = idx * 2;
        let attr_hi = shared_ram[0x1B80 + off + 1];
        if (attr_hi & 0x02) != 0 {
            continue;
        }

        let raw_code = shared_ram[0x0B80 + off];
        let color = (shared_ram[0x0B80 + off + 1] & 0x3F) as usize;
        let mut flags = shared_ram[0x1B80 + off];

        let base_x = (shared_ram[0x1380 + off] as i32) - 16;
        let mut base_y = (shared_ram[0x1380 + off + 1] as i32) - 40 + 1;
        if base_y < 16 {
            base_y += 256;
        }

        let mut code = raw_code as usize;
        if (raw_code & 0x80) != 0 {
            flags |= 0x0C;
            code = ((code & 0xC0) | ((code & !0xC0) << 2)) & 0xFF;
        }

        let double_x = (flags & 0x08) != 0;
        let double_y = (flags & 0x04) != 0;
        let hflip = (flags & 0x02) != 0;
        let vflip = (flags & 0x01) != 0;

        // Build up to 4 16x16 sub-sprites (matching Galagino + MAME vflip/hflip ordering)
        let mut sub_sprites: [(i32, i32, usize); 4] = [(0, 0, 0); 4];
        let mut count = 0usize;

        let mut c00 = code;
        if double_x {
            c00 = (c00 + 2) & 0xFF;
        }
        sub_sprites[count] = (base_x, base_y, c00);
        count += 1;

        if double_x {
            sub_sprites[count] = (base_x + 16, base_y, code);
            count += 1;
            if hflip {
                let tmp = sub_sprites[count - 1].2;
                sub_sprites[count - 1].2 = sub_sprites[count - 2].2;
                sub_sprites[count - 2].2 = tmp;
            }
        }

        if double_y {
            sub_sprites[count] = (base_x, base_y + 16, (code + 3) & 0xFF);
            count += 1;
        }

        if double_x && double_y {
            sub_sprites[count] = (base_x + 16, base_y + 16, (code + 1) & 0xFF);
            count += 1;
            if hflip {
                let tmp = sub_sprites[count - 1].2;
                sub_sprites[count - 1].2 = sub_sprites[count - 2].2;
                sub_sprites[count - 2].2 = tmp;
            }
            if vflip {
                sub_sprites.swap(0, 2);
                sub_sprites.swap(1, 3);
                // Keep screen positions while swapping tile codes vertically
                let y0 = sub_sprites[2].1;
                let y2 = sub_sprites[0].1;
                sub_sprites[0].1 = y0;
                sub_sprites[1].1 = y0;
                sub_sprites[2].1 = y2;
                sub_sprites[3].1 = y2;
            }
        }

        let flip_bits = (flags & 0x03) as usize;
        let clut_rgb = &roms.sprite_clut_rgb[color];
        let clut_pen = &roms.sprite_clut_pen[color];
        if clut_rgb[0] != 0 && clut_pen[0] != 0x0F {
            continue;
        }

        for &(sx, sy, sub_code) in &sub_sprites[..count] {
            if sy <= -16 || sy >= ARCADE_HEIGHT as i32 || sx <= -16 || sx >= ARCADE_WIDTH as i32 {
                continue;
            }
            let spr_bitmap = &roms.sprites_decoded[flip_bits * 256 + (sub_code & 0xFF)];
            for py in 0..16 {
                let dst_y = sy + py as i32;
                // Clip top 2 rows and bottom 2 rows outside visible sprite region (rows 2..34)
                if dst_y < 16 || dst_y >= (ARCADE_HEIGHT as i32 - 16) {
                    continue;
                }
                for px in 0..16 {
                    let dst_x = sx + px as i32;
                    if dst_x < 0 || dst_x >= ARCADE_WIDTH as i32 {
                        continue;
                    }
                    let pen = (spr_bitmap[py][px] & 3) as usize;
                    if pen == 0 || clut_pen[pen] == 0x0F || clut_rgb[pen] == 0 {
                        continue;
                    }
                    arcade_fb[dst_y as usize * ARCADE_WIDTH + dst_x as usize] = clut_rgb[pen];
                }
            }
        }
    }
}

fn dir_name(dir: u8) -> &'static str {
    match dir & 0x07 {
        0 => "UP",
        2 => "RIGHT",
        4 => "DOWN",
        6 => "LEFT",
        _ => "IDLE",
    }
}

/// Renders either the Clean Arcade Window (`720x912`, `show_telemetry = false`)
/// or the Expanded Hardware & Game Guide Window (`1480x912`, `show_telemetry = true`).
pub fn render_workbench_window(
    machine: &DigDugMachine,
    arcade_fb: &[u32; ARCADE_WIDTH * ARCADE_HEIGHT],
    window_fb: &mut [u32],
    show_telemetry: bool,
) -> usize {
    let fb_w = if show_telemetry {
        TELEMETRY_WINDOW_WIDTH
    } else {
        ARCADE_WINDOW_WIDTH
    };

    // Fill active framebuffer slice
    window_fb[..fb_w * WINDOW_HEIGHT].fill(BG_OUTER);

    // 1. Sleek Top Control Instruction Bar (x: 20..700, y: 4..30)
    draw_card(window_fb, fb_w, 20, 4, 680, 26);
    let controls_top: [(&str, &str, usize); 7] = [
        ("5/C", "COIN", 26),
        ("1", "1P", 112),
        ("2", "2P", 170),
        ("WASD", "MOVE", 228),
        ("SPC", "PUMP", 322),
        ("R", "RESET", 410),
        ("TAB", "GUIDE", 492),
    ];
    for &(key_txt, label_txt, bx) in &controls_top {
        let bw = draw_badge(
            window_fb,
            fb_w,
            bx,
            8,
            key_txt,
            BADGE_SLATE_BG,
            BADGE_AMBER_FG,
        );
        draw_text(window_fb, fb_w, bx + bw + 6, 11, label_txt, TEXT_PRIMARY);
    }
    let snap = machine.snapshot();
    draw_badge(
        window_fb,
        fb_w,
        610,
        8,
        &format!("CRED {:02X}", snap.credits_bcd),
        BADGE_EMERALD_BG,
        BADGE_EMERALD_FG,
    );

    // 2. Draw Arcade Bezel Frame (x: 20..700, y: 32..904)
    let crt_x = 24;
    let crt_y = 36;
    let crt_w = ARCADE_WIDTH * ARCADE_SCALE; // 672
    let crt_h = ARCADE_HEIGHT * ARCADE_SCALE; // 864
    draw_card(window_fb, fb_w, crt_x - 4, crt_y - 4, crt_w + 8, crt_h + 8);

    // Blit 3x integer-scaled arcade framebuffer with subtle scanline shading
    for y in 0..ARCADE_HEIGHT {
        let src_row = &arcade_fb[y * ARCADE_WIDTH..(y + 1) * ARCADE_WIDTH];
        for sy in 0..ARCADE_SCALE {
            let dst_y = crt_y + y * ARCADE_SCALE + sy;
            let dst_row_start = dst_y * fb_w + crt_x;
            // Subtle 6% scanline darkening on the bottom sub-row of each 3x pixel for authentic CRT texture
            let darken = sy == 2;
            for x in 0..ARCADE_WIDTH {
                let mut c = src_row[x];
                if darken && c != 0 {
                    let r = (((c >> 16) & 0xFF) * 15) >> 4;
                    let g = (((c >> 8) & 0xFF) * 15) >> 4;
                    let b = ((c & 0xFF) * 15) >> 4;
                    c = (r << 16) | (g << 8) | b;
                }
                let px_base = dst_row_start + x * ARCADE_SCALE;
                window_fb[px_base] = c;
                window_fb[px_base + 1] = c;
                window_fb[px_base + 2] = c;
            }
        }
    }

    if !show_telemetry {
        return fb_w;
    }

    // 3. Right-side Sleek Modern Minimalist Live Gameplay Inspector
    let px = 714;
    let pw = 692;

    // =========================================================================
    // CARD 1: PLAYER, ROUND & BONUS STATUS (y: 4..216)
    // =========================================================================
    draw_card(window_fb, fb_w, px, 4, pw, 212);
    draw_title_5x7(
        window_fb,
        fb_w,
        px + 18,
        14,
        "DIG DUG - LIVE GAMEPLAY INSPECTOR",
        TEXT_PRIMARY,
        2,
    );

    let (mode_label, mode_bg, mode_fg) = if snap.in_game {
        if snap.active_player == 2 {
            ("ACTIVE - PLAYER 2", BADGE_AMBER_BG, BADGE_AMBER_FG)
        } else {
            ("ACTIVE - PLAYER 1", BADGE_EMERALD_BG, BADGE_EMERALD_FG)
        }
    } else if snap.attract_demo {
        ("ATTRACT DEMO", BADGE_AMBER_BG, BADGE_AMBER_FG)
    } else {
        ("TITLE / STANDBY", BADGE_SLATE_BG, BADGE_SLATE_FG)
    };
    let mut bx = px + 18;
    bx += draw_badge(window_fb, fb_w, bx, 38, mode_label, mode_bg, mode_fg) + 10;

    let shown_round = if snap.active_player == 2 {
        snap.p2_round.max(1)
    } else {
        snap.p1_round.max(1)
    };
    let map_layout = if snap.stage_layout_idx > 0 {
        snap.stage_layout_idx
    } else if shown_round <= 15 {
        shown_round
    } else {
        12 + ((shown_round - 16) & 3)
    };
    bx += draw_badge(
        window_fb,
        fb_w,
        bx,
        38,
        &format!("ROUND {} (MAP #{}/15)", shown_round, map_layout),
        BADGE_SLATE_BG,
        BADGE_AMBER_FG,
    ) + 10;
    let (player_lives, player_score_str) = if snap.active_player == 2 {
        (
            snap.p2_lives,
            format!("2UP: {:06}  HI: {:06}", snap.p2_score, snap.high_score),
        )
    } else {
        (
            snap.p1_lives,
            format!("1UP: {:06}  HI: {:06}", snap.p1_score, snap.high_score),
        )
    };
    bx += draw_badge(
        window_fb,
        fb_w,
        bx,
        38,
        &format!("LIVES: {}", player_lives),
        BADGE_SLATE_BG,
        TEXT_PRIMARY,
    ) + 10;
    draw_badge(
        window_fb,
        fb_w,
        bx,
        38,
        &player_score_str,
        BADGE_SLATE_BG,
        TEXT_PRIMARY,
    );

    draw_hline(window_fb, fb_w, px + 18, 64, pw - 36, CARD_BORDER);

    // Player Position, Depth Stratum & Action
    let (layer_label, layer_pts) = match snap.digdug_stratum {
        0 => ("SURFACE", 200),
        1 => ("LAYER 1", 200),
        2 => ("LAYER 2", 300),
        3 => ("LAYER 3", 400),
        _ => ("LAYER 4", 500),
    };
    let player_action = if snap.harpoon_tethered {
        "PUMPING ENEMY!"
    } else if snap.harpoon_active {
        "FIRING HARPOON"
    } else if snap.digdug_digging {
        "DIGGING DIRT"
    } else if snap.digdug_moving {
        "WALKING TUNNEL"
    } else {
        "IDLE / STOPPED"
    };
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        76,
        &format!(
            "DIG DUG     X={:3} Y={:3}   {} ({} PT POP)   {}",
            snap.digdug_x, snap.digdug_y, layer_label, layer_pts, player_action
        ),
        TEXT_PRIMARY,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        96,
        "Pop Points: Surface/L1 = 200 | L2 = 300 | L3 = 400 | L4 = 500  (Horiz Fygar = 2x)",
        TEXT_SECONDARY,
    );

    // Monster Speed + Visual Bar
    let spd_pct = (snap.monster_speed as usize * 100) / 255;
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        124,
        &format!("ENEMY PACE  {:3}/255 ({:2}%)", snap.monster_speed, spd_pct),
        TEXT_PRIMARY,
    );
    let bar_x = px + 236;
    let bar_w = 180;
    draw_rect(window_fb, fb_w, bar_x, 125, bar_w, 11, BADGE_SLATE_BG);
    let fill_w = (snap.monster_speed as usize * bar_w) / 255;
    let bar_col = if snap.escape_mode || snap.monster_speed >= 220 {
        BADGE_ROSE_FG
    } else if snap.monster_speed > 165 {
        BADGE_AMBER_FG
    } else {
        BADGE_EMERALD_FG
    };
    draw_rect(window_fb, fb_w, bar_x, 125, fill_w, 11, bar_col);
    let pace_txt = if snap.escape_mode {
        "ESCAPE BOOST! (LAST ENEMY FLEEING)"
    } else {
        "NORMAL ROUND SPEED"
    };
    draw_text(window_fb, fb_w, bar_x + bar_w + 14, 124, pace_txt, bar_col);

    // Bonus Veggie & Boulders Dropped
    let veg_status = if snap.veggie_active {
        format!(
            "ACTIVE IN CENTER! ({:.1}s LEFT)",
            snap.veggie_timer_frames as f32 / 60.0
        )
    } else if snap.veggie_collected {
        format!("COLLECTED! (+{} PTS)", snap.veggie_pts)
    } else {
        format!(
            "{}/2 ROCKS DROPPED (Drop 2 to spawn)",
            snap.rocks_dropped.min(2)
        )
    };
    let veg_col = if snap.veggie_active || snap.veggie_collected {
        BADGE_EMERALD_FG
    } else {
        BADGE_AMBER_FG
    };
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        154,
        &format!(
            "VEGGIE      {} ({} PTS)  ->  {}",
            snap.veggie_name, snap.veggie_pts, veg_status
        ),
        veg_col,
    );

    // Stage Map & Progression Note
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        184,
        &format!(
            "STAGE MAP   Map #{:2} of 15  (Rounds 1-15 unique; Rounds 16+ cycle Maps #12-#15)",
            map_layout
        ),
        TEXT_SECONDARY,
    );

    // =========================================================================
    // CARD 2: LIVE ENEMIES (8 SLOTS: #0-#3 POOKA, #4-#7 FYGAR) (y: 226..590)
    // =========================================================================
    draw_card(window_fb, fb_w, px, 226, pw, 364);
    draw_title_5x7(
        window_fb,
        fb_w,
        px + 18,
        238,
        "LIVE ENEMIES",
        TEXT_PRIMARY,
        2,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 176,
        239,
        "8 Hardware Slots (#0-#3 Pooka, #4-#7 Fygar)",
        TEXT_SECONDARY,
    );
    let alive_badge = if snap.escape_mode && snap.active_enemies > 0 {
        format!("ALIVE: {}/{} (FLEEING!)", snap.active_enemies, snap.spawned_enemies)
    } else {
        format!("ALIVE: {} / {}", snap.active_enemies, snap.spawned_enemies)
    };
    let alive_bg = if snap.escape_mode && snap.active_enemies > 0 {
        BADGE_ROSE_BG
    } else {
        BADGE_EMERALD_BG
    };
    let alive_fg = if snap.escape_mode && snap.active_enemies > 0 {
        BADGE_ROSE_FG
    } else {
        BADGE_EMERALD_FG
    };
    let alive_badge_w = alive_badge.len() * 8 + 14;
    draw_badge(
        window_fb,
        fb_w,
        px + pw - alive_badge_w - 18,
        236,
        &alive_badge,
        alive_bg,
        alive_fg,
    );

    draw_hline(window_fb, fb_w, px + 18, 264, pw - 36, CARD_BORDER);

    draw_text(
        window_fb,
        fb_w,
        px + 18,
        274,
        "SLOT   ENEMY     POS (X,Y)       PUMP (0-4)     LIVE AI STATUS",
        TEXT_MUTED,
    );

    for m in 0..8 {
        let my = 300 + m * 34;
        let is_fygar = (snap.monster_type_mask & (1 << m)) != 0;
        let mtype = if is_fygar { "FYGAR" } else { "POOKA" };
        let mx_pos = snap.monster_x[m];
        let my_pos = snap.monster_y[m];
        let inflate = snap.monster_inflate[m] & 0x07;
        let def_t = snap.monster_deflate_timer[m];
        let dir_str = dir_name(snap.monster_dir[m]);
        let status = snap.monster_status[m];

        let active = matches!(
            status,
            MonsterStatus::Walking
                | MonsterStatus::Ghosting
                | MonsterStatus::BreathingFire
                | MonsterStatus::Escaping
                | MonsterStatus::Inflating
                | MonsterStatus::Popping
                | MonsterStatus::Crushed
        );

        let (row_col, status_text) = match status {
            MonsterStatus::Walking => (TEXT_PRIMARY, format!("WALKING {}", dir_str)),
            MonsterStatus::Ghosting => (BADGE_AMBER_FG, "GHOSTING THROUGH DIRT".to_string()),
            MonsterStatus::BreathingFire => (BADGE_ROSE_FG, "BREATHING FIRE!".to_string()),
            MonsterStatus::Escaping => (BADGE_ROSE_FG, "ESCAPING TO TOP-LEFT!".to_string()),
            MonsterStatus::Inflating => (
                BADGE_EMERALD_FG,
                format!(
                    "PUMPED {}/4 (DEFLATES {:.1}s)",
                    inflate,
                    def_t as f32 / 60.0
                ),
            ),
            MonsterStatus::Popping => (BADGE_EMERALD_FG, "POPPING! (+PTS)".to_string()),
            MonsterStatus::Crushed => (BADGE_ROSE_FG, "CRUSHED BY BOULDER!".to_string()),
            MonsterStatus::DefeatedPopped => (TEXT_MUTED, "DEFEATED (POPPED)".to_string()),
            MonsterStatus::DefeatedCrushed => (TEXT_MUTED, "DEFEATED (CRUSHED)".to_string()),
            MonsterStatus::Escaped => (TEXT_MUTED, "ESCAPED STAGE".to_string()),
            MonsterStatus::Unused => (TEXT_MUTED, "--- (UNUSED SLOT)".to_string()),
        };

        let type_col = if !active {
            TEXT_MUTED
        } else if is_fygar {
            BADGE_EMERALD_FG
        } else {
            BADGE_AMBER_FG
        };

        draw_text(window_fb, fb_w, px + 18, my, &format!("#{m}"), row_col);
        draw_text(window_fb, fb_w, px + 74, my, mtype, type_col);

        if active {
            draw_text(
                window_fb,
                fb_w,
                px + 154,
                my,
                &format!("X={:3} Y={:3}", mx_pos, my_pos),
                row_col,
            );
            draw_pump_meter(window_fb, fb_w, px + 282, my + 1, inflate);
            draw_text(
                window_fb,
                fb_w,
                px + 338,
                my,
                &format!("{}/4", inflate),
                if inflate > 0 { BADGE_EMERALD_FG } else { TEXT_SECONDARY },
            );
        } else {
            draw_text(window_fb, fb_w, px + 154, my, "  ---   ---", TEXT_MUTED);
            draw_text(window_fb, fb_w, px + 300, my, "---", TEXT_MUTED);
        }

        draw_text(window_fb, fb_w, px + 394, my, &status_text, row_col);

        if m < 7 {
            draw_hline(window_fb, fb_w, px + 18, my + 21, pw - 36, 0x1E2028);
        }
    }

    // =========================================================================
    // CARD 3: STAGE BOULDERS / ROCKS (y: 600..802)
    // =========================================================================
    draw_card(window_fb, fb_w, px, 600, pw, 202);
    draw_title_5x7(
        window_fb,
        fb_w,
        px + 18,
        612,
        "STAGE BOULDERS",
        TEXT_PRIMARY,
        2,
    );

    let mut stage_rocks: Vec<(usize, u8, u8, RockStatus, u8)> = Vec::new();
    for r_idx in 0..8 {
        let st = snap.rock_status[r_idx];
        if st != RockStatus::Unused {
            stage_rocks.push((
                stage_rocks.len() + 1,
                snap.rock_x[r_idx],
                snap.rock_y[r_idx],
                st,
                snap.rock_wobble_timer[r_idx],
            ));
        }
    }
    let rock_badge = format!("INTACT: {} / {}", snap.rocks_intact, stage_rocks.len());
    let rock_badge_w = rock_badge.len() * 8 + 14;
    draw_badge(
        window_fb,
        fb_w,
        px + pw - rock_badge_w - 18,
        610,
        &rock_badge,
        BADGE_SLATE_BG,
        TEXT_PRIMARY,
    );

    draw_text(
        window_fb,
        fb_w,
        px + 204,
        613,
        "Crush: 1=1k | 2=2.5k | 3=4k | 4=6k pts",
        TEXT_SECONDARY,
    );
    draw_hline(window_fb, fb_w, px + 18, 638, pw - 36, CARD_BORDER);

    for (idx, (num, rx, ry, r_stat, timer)) in stage_rocks.iter().take(5).enumerate() {
        let ry_row = 650 + idx * 28;
        let (r_col, r_desc) = match r_stat {
            RockStatus::Resting => (TEXT_PRIMARY, "INTACT (RESTING IN DIRT)".to_string()),
            RockStatus::Wobbling => (
                BADGE_AMBER_FG,
                format!("WOBBLING! (DROPS IN {} FRAMES)", timer),
            ),
            RockStatus::Falling => (BADGE_ROSE_FG, "FALLING DOWN TUNNEL!".to_string()),
            RockStatus::Shattering => (BADGE_AMBER_FG, "CRUMBLING AT BOTTOM".to_string()),
            RockStatus::Shattered => (TEXT_MUTED, "DROPPED & SHATTERED".to_string()),
            RockStatus::Unused => (TEXT_MUTED, "---".to_string()),
        };
        draw_text(
            window_fb,
            fb_w,
            px + 18,
            ry_row,
            &format!("BOULDER #{}", num),
            r_col,
        );
        if *r_stat == RockStatus::Shattered {
            draw_text(window_fb, fb_w, px + 154, ry_row, "  ---   ---", TEXT_MUTED);
        } else {
            draw_text(
                window_fb,
                fb_w,
                px + 154,
                ry_row,
                &format!("X={:3} Y={:3}", rx, ry),
                r_col,
            );
        }
        draw_text(window_fb, fb_w, px + 300, ry_row, &r_desc, r_col);
    }

    // =========================================================================
    // CARD 4: CONTROLS (y: 812..904)
    // =========================================================================
    draw_card(window_fb, fb_w, px, 812, pw, 92);
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        822,
        "CONTROLS",
        TEXT_PRIMARY,
    );
    draw_hline(window_fb, fb_w, px + 18, 840, pw - 36, CARD_BORDER);

    draw_text(
        window_fb,
        fb_w,
        px + 18,
        850,
        "[5/C] Coin   [1] Start 1P   [2] Start 2P   [WASD/Arrows] Move   [Space/Z] Pump",
        TEXT_PRIMARY,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 18,
        872,
        "[Tab] Toggle Sidebar                  [R] Warm Reset         [Esc] Quit Game",
        TEXT_SECONDARY,
    );

    fb_w
}

fn draw_pump_meter(fb: &mut [u32], fb_w: usize, x: usize, y: usize, inflate: u8) {
    for step in 0..4u8 {
        let bx = x + (step as usize) * 12;
        let lit = inflate > step;
        let col = if !lit {
            BADGE_SLATE_BG
        } else if inflate >= 4 {
            BADGE_ROSE_FG
        } else if inflate == 3 {
            BADGE_AMBER_FG
        } else {
            BADGE_EMERALD_FG
        };
        draw_rect(fb, fb_w, bx, y, 10, 10, col);
        draw_hline(fb, fb_w, bx, y, 10, CARD_BORDER);
        draw_hline(fb, fb_w, bx, y + 9, 10, CARD_BORDER);
    }
}

fn draw_card(fb: &mut [u32], fb_w: usize, x: usize, y: usize, w: usize, h: usize) {
    for dy in 0..h {
        let py = y + dy;
        if py >= WINDOW_HEIGHT {
            break;
        }
        // Subtle vertical slate gradient from CARD_TOP to CARD_BOT
        let t = (dy * 256) / h.max(1);
        let r = (((CARD_TOP >> 16) & 0xFF) * (256 - t as u32)
            + ((CARD_BOT >> 16) & 0xFF) * (t as u32))
            >> 8;
        let g = (((CARD_TOP >> 8) & 0xFF) * (256 - t as u32)
            + ((CARD_BOT >> 8) & 0xFF) * (t as u32))
            >> 8;
        let b = ((CARD_TOP & 0xFF) * (256 - t as u32) + (CARD_BOT & 0xFF) * (t as u32)) >> 8;
        let fill = (r << 16) | (g << 8) | b;

        for dx in 0..w {
            let px = x + dx;
            if px >= fb_w {
                break;
            }
            let is_border = dy == 0 || dy == h - 1 || dx == 0 || dx == w - 1;
            let color = if dy == 0 && dx > 0 && dx < w - 1 {
                CARD_HIGHLIGHT
            } else if is_border {
                CARD_BORDER
            } else {
                fill
            };
            fb[py * fb_w + px] = color;
        }
    }
}

fn draw_rect(fb: &mut [u32], fb_w: usize, x: usize, y: usize, w: usize, h: usize, color: u32) {
    for dy in 0..h {
        let py = y + dy;
        if py >= WINDOW_HEIGHT {
            break;
        }
        for dx in 0..w {
            let px = x + dx;
            if px >= fb_w {
                break;
            }
            fb[py * fb_w + px] = color;
        }
    }
}

fn draw_hline(fb: &mut [u32], fb_w: usize, x: usize, y: usize, w: usize, color: u32) {
    if y >= WINDOW_HEIGHT {
        return;
    }
    for dx in 0..w {
        let px = x + dx;
        if px < fb_w {
            fb[y * fb_w + px] = color;
        }
    }
}

fn draw_badge(
    fb: &mut [u32],
    fb_w: usize,
    x: usize,
    y: usize,
    text: &str,
    bg: u32,
    fg: u32,
) -> usize {
    let w = text.len() * 8 + 14;
    let h = 18;
    draw_rect(fb, fb_w, x, y, w, h, bg);
    draw_hline(fb, fb_w, x, y, w, CARD_BORDER);
    draw_hline(fb, fb_w, x, y + h - 1, w, CARD_BORDER);
    draw_text(fb, fb_w, x + 7, y + 3, text, fg);
    w
}

/// Draws large crisp 8x12 mixed-case bitmap text (8px wide advance, 12px tall, 2px bold stems).
fn draw_text(fb: &mut [u32], fb_w: usize, x: usize, y: usize, text: &str, color: u32) {
    let mut cx = x;
    for ch in text.chars() {
        let glyph = glyph_8x12(ch);
        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            let py = y + row_idx;
            if py >= WINDOW_HEIGHT {
                break;
            }
            for col_idx in 0..7 {
                if (row_bits & (1 << (7 - col_idx))) != 0 {
                    let px = cx + col_idx;
                    if px < fb_w {
                        fb[py * fb_w + px] = color;
                    }
                }
            }
        }
        cx += 8;
    }
}

fn draw_title_5x7(
    fb: &mut [u32],
    fb_w: usize,
    x: usize,
    y: usize,
    text: &str,
    color: u32,
    scale: usize,
) {
    let mut cx = x;
    for ch in text.chars() {
        let glyph = glyph_5x7(ch);
        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..5 {
                if (row_bits & (1 << (4 - col_idx))) != 0 {
                    for sy in 0..scale {
                        for sx in 0..scale {
                            let px = cx + col_idx * scale + sx;
                            let py = y + row_idx * scale + sy;
                            if px < fb_w && py < WINDOW_HEIGHT {
                                fb[py * fb_w + px] = color;
                            }
                        }
                    }
                }
            }
        }
        cx += 6 * scale;
    }
}

/// High-legibility 8x12 bold mixed-case bitmap font (bits 7..1 active, 2px vertical stems).
fn glyph_8x12(ch: char) -> [u8; 12] {
    match ch {
        // Uppercase A-Z (rows 1..=9, 9px cap height, 2px vertical stems)
        'A' => [0x00, 0x38, 0x6C, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'B' => [0x00, 0xFC, 0xC6, 0xC6, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0xFC, 0x00, 0x00],
        'C' => [0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00],
        'D' => [0x00, 0xF8, 0xCC, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xCC, 0xF8, 0x00, 0x00],
        'E' => [0x00, 0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00, 0x00],
        'F' => [0x00, 0xFE, 0xC0, 0xC0, 0xF8, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0x00, 0x00],
        'G' => [0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xCE, 0xC6, 0xC6, 0xC6, 0x7E, 0x00, 0x00],
        'H' => [0x00, 0xC6, 0xC6, 0xC6, 0xFE, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'I' => [0x00, 0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00],
        'J' => [0x00, 0x3E, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00, 0x00],
        'K' => [0x00, 0xC6, 0xCC, 0xD8, 0xF0, 0xF0, 0xD8, 0xCC, 0xC6, 0xC6, 0x00, 0x00],
        'L' => [0x00, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0xFE, 0x00, 0x00],
        'M' => [0x00, 0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'N' => [0x00, 0xC6, 0xE6, 0xF6, 0xFE, 0xDE, 0xCE, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'O' => [0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        'P' => [0x00, 0xFC, 0xC6, 0xC6, 0xC6, 0xFC, 0xC0, 0xC0, 0xC0, 0xC0, 0x00, 0x00],
        'Q' => [0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xDE, 0x7C, 0x06, 0x00],
        'R' => [0x00, 0xFC, 0xC6, 0xC6, 0xC6, 0xFC, 0xD8, 0xCC, 0xC6, 0xC6, 0x00, 0x00],
        'S' => [0x00, 0x7C, 0xC6, 0xC0, 0x78, 0x0E, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00],
        'T' => [0x00, 0xFE, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00],
        'U' => [0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        'V' => [0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00],
        'W' => [0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0xC6, 0x00, 0x00],
        'X' => [0x00, 0xC6, 0xC6, 0x6C, 0x38, 0x38, 0x6C, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'Y' => [0x00, 0xC6, 0xC6, 0x6C, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00, 0x00],
        'Z' => [0x00, 0xFE, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xC0, 0xFE, 0x00, 0x00],

        // Lowercase a-z (distinct x-height rows 3..=9, ascenders 1..=9, descenders 3..=11)
        'a' => [0x00, 0x00, 0x00, 0x78, 0x0C, 0x7C, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00],
        'b' => [0x00, 0xC0, 0xC0, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xE6, 0xDC, 0x00, 0x00],
        'c' => [0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC0, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00],
        'd' => [0x00, 0x06, 0x06, 0x76, 0xCE, 0xC6, 0xC6, 0xC6, 0xCE, 0x76, 0x00, 0x00],
        'e' => [0x00, 0x00, 0x00, 0x7C, 0xC6, 0xFE, 0xC0, 0xC0, 0xC6, 0x7C, 0x00, 0x00],
        'f' => [0x00, 0x3C, 0x66, 0x60, 0xF8, 0x60, 0x60, 0x60, 0x60, 0x60, 0x00, 0x00],
        'g' => [0x00, 0x00, 0x00, 0x76, 0xCE, 0xC6, 0xC6, 0xCE, 0x76, 0x06, 0xC6, 0x7C],
        'h' => [0x00, 0xC0, 0xC0, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'i' => [0x00, 0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00],
        'j' => [0x00, 0x0C, 0x00, 0x1C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78],
        'k' => [0x00, 0xC0, 0xC0, 0xCC, 0xD8, 0xF0, 0xF0, 0xD8, 0xCC, 0xC6, 0x00, 0x00],
        'l' => [0x00, 0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00, 0x00],
        'm' => [0x00, 0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xD6, 0xD6, 0xC6, 0xC6, 0x00, 0x00],
        'n' => [0x00, 0x00, 0x00, 0xDC, 0xE6, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x00, 0x00],
        'o' => [0x00, 0x00, 0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        'p' => [0x00, 0x00, 0x00, 0xDC, 0xE6, 0xC6, 0xC6, 0xE6, 0xDC, 0xC0, 0xC0, 0xC0],
        'q' => [0x00, 0x00, 0x00, 0x76, 0xCE, 0xC6, 0xC6, 0xCE, 0x76, 0x06, 0x06, 0x06],
        'r' => [0x00, 0x00, 0x00, 0xDC, 0xE6, 0xC0, 0xC0, 0xC0, 0xC0, 0xC0, 0x00, 0x00],
        's' => [0x00, 0x00, 0x00, 0x7E, 0xC0, 0x7C, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00],
        't' => [0x00, 0x30, 0x30, 0xFC, 0x30, 0x30, 0x30, 0x30, 0x36, 0x1C, 0x00, 0x00],
        'u' => [0x00, 0x00, 0x00, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0x76, 0x00, 0x00],
        'v' => [0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xC6, 0x6C, 0x38, 0x10, 0x00, 0x00],
        'w' => [0x00, 0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xD6, 0xFE, 0xEE, 0x44, 0x00, 0x00],
        'x' => [0x00, 0x00, 0x00, 0xC6, 0x6C, 0x38, 0x38, 0x6C, 0xC6, 0xC6, 0x00, 0x00],
        'y' => [0x00, 0x00, 0x00, 0xC6, 0xC6, 0xC6, 0xCE, 0x76, 0x06, 0xC6, 0x7C, 0x00],
        'z' => [0x00, 0x00, 0x00, 0xFE, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xFE, 0x00, 0x00],

        // Digits 0-9
        '0' => [0x00, 0x7C, 0xC6, 0xCE, 0xDE, 0xF6, 0xE6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        '1' => [0x00, 0x18, 0x38, 0x78, 0x18, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00, 0x00],
        '2' => [0x00, 0x7C, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0xFE, 0x00, 0x00],
        '3' => [0x00, 0x7C, 0xC6, 0x06, 0x3C, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00],
        '4' => [0x00, 0x0C, 0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x0C, 0x0C, 0x00, 0x00],
        '5' => [0x00, 0xFE, 0xC0, 0xC0, 0xFC, 0x06, 0x06, 0x06, 0xC6, 0x7C, 0x00, 0x00],
        '6' => [0x00, 0x3C, 0x60, 0xC0, 0xFC, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        '7' => [0x00, 0xFE, 0xC6, 0x06, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x00, 0x00],
        '8' => [0x00, 0x7C, 0xC6, 0xC6, 0x7C, 0xC6, 0xC6, 0xC6, 0xC6, 0x7C, 0x00, 0x00],
        '9' => [0x00, 0x7C, 0xC6, 0xC6, 0xC6, 0x7E, 0x06, 0x0C, 0x18, 0x70, 0x00, 0x00],

        // Symbols & Punctuation
        ':' => [0x00, 0x00, 0x00, 0x38, 0x38, 0x00, 0x00, 0x38, 0x38, 0x00, 0x00, 0x00],
        ';' => [0x00, 0x00, 0x00, 0x38, 0x38, 0x00, 0x00, 0x38, 0x38, 0x18, 0x30, 0x00],
        '-' => [0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFE, 0x00, 0x00],
        '/' => [0x00, 0x06, 0x0C, 0x0C, 0x18, 0x30, 0x30, 0x60, 0xC0, 0x80, 0x00, 0x00],
        '|' => [0x00, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00],
        '(' => [0x00, 0x0C, 0x18, 0x30, 0x30, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00, 0x00],
        ')' => [0x00, 0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00, 0x00],
        '[' => [0x00, 0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00, 0x00],
        ']' => [0x00, 0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00, 0x00],
        '$' => [0x18, 0x7E, 0xD8, 0xD8, 0x7C, 0x1E, 0x1B, 0x1B, 0x7E, 0x18, 0x00, 0x00],
        '#' => [0x00, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0xFE, 0x6C, 0x6C, 0x00, 0x00, 0x00],
        '%' => [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x30, 0x66, 0xC6, 0x00, 0x00, 0x00, 0x00],
        '+' => [0x00, 0x00, 0x18, 0x18, 0x18, 0xFE, 0x18, 0x18, 0x18, 0x00, 0x00, 0x00],
        '=' => [0x00, 0x00, 0x00, 0xFE, 0x00, 0x00, 0xFE, 0x00, 0x00, 0x00, 0x00, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x38, 0x18, 0x30],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x38, 0x38, 0x00, 0x00],
        '!' => [0x00, 0x18, 0x3C, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x18, 0x00, 0x00],
        '?' => [0x00, 0x7C, 0xC6, 0x0C, 0x18, 0x30, 0x30, 0x00, 0x30, 0x30, 0x00, 0x00],
        '\'' => [0x00, 0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        '&' => [0x00, 0x38, 0x6C, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0xDC, 0x76, 0x00, 0x00],
        '>' => [0x00, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x00, 0x00],
        '<' => [0x00, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x60, 0x30, 0x18, 0x0C, 0x00, 0x00],
        _ => [0; 12],
    }
}

/// Clean 5x7 uppercase/symbol bitmap font used at 2x scale for the main banner.
fn glyph_5x7(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '_' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        '$' => [0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100],
        '#' => [0b01010, 0b11111, 0b01010, 0b01010, 0b11111, 0b01010, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '=' => [0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100],
        '@' => [0b01110, 0b10001, 0b10111, 0b10101, 0b10111, 0b10000, 0b01110],
        '&' => [0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101],
        '>' => [0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000],
        '<' => [0b00010, 0b00100, 0b01000, 0b10000, 0b01000, 0b00100, 0b00010],
        _ => [0, 0, 0, 0, 0, 0, 0],
    }
}

/// Writes an `0x00RRGGBB` framebuffer (`width x height`) to a standard 24-bit RGB PNG file
/// without requiring any external image crate dependencies.
pub fn save_png(path: &std::path::Path, fb: &[u32], width: usize, height: usize) -> std::io::Result<()> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                if (crc & 1) != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(tag);
        out.extend_from_slice(payload);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }

    let mut raw = Vec::with_capacity(height * (1 + width * 3));
    for y in 0..height {
        raw.push(0); // Filter type 0 (None)
        let row = &fb[y * width..(y + 1) * width];
        for &px in row {
            raw.push(((px >> 16) & 0xFF) as u8);
            raw.push(((px >> 8) & 0xFF) as u8);
            raw.push((px & 0xFF) as u8);
        }
    }

    // Wrap raw scanlines in uncompressed zlib/DEFLATE blocks + Adler-32
    let mut zlib = Vec::with_capacity(raw.len() + (raw.len() / 65535 + 1) * 5 + 6);
    zlib.push(0x78);
    zlib.push(0x01);
    let chunks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == chunks.len();
        zlib.push(if is_last { 0x01 } else { 0x00 });
        let len = chunk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in &raw {
        s1 = (s1 + b as u32) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let adler = (s2 << 16) | s1;
    zlib.extend_from_slice(&adler.to_be_bytes());

    let mut png = Vec::with_capacity(zlib.len() + 64);
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = [0u8; 13];
    ihdr[0..4].copy_from_slice(&(width as u32).to_be_bytes());
    ihdr[4..8].copy_from_slice(&(height as u32).to_be_bytes());
    ihdr[8] = 8; // 8 bits per channel
    ihdr[9] = 2; // Truecolor RGB
    write_chunk(&mut png, b"IHDR", &ihdr);
    write_chunk(&mut png, b"IDAT", &zlib);
    write_chunk(&mut png, b"IEND", &[]);

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, png)
}
