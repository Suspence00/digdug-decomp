use crate::hardware::DigDugMachine;
use crate::rom::DigDugRoms;

pub const ARCADE_WIDTH: usize = 224;
pub const ARCADE_HEIGHT: usize = 288;
pub const ARCADE_SCALE: usize = 3;

pub const ARCADE_WINDOW_WIDTH: usize = 720;
pub const TELEMETRY_WINDOW_WIDTH: usize = 1160;
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

// Sleek Modern Minimalist Warm/Neutral Slate Palette
const BG_OUTER: u32 = 0x141417;
const CARD_TOP: u32 = 0x222227;
const CARD_BOT: u32 = 0x1A1A1F;
const CARD_BORDER: u32 = 0x34343D;
const CARD_HIGHLIGHT: u32 = 0x3E3E48;

const TEXT_PRIMARY: u32 = 0xF4F4F5;
const TEXT_SECONDARY: u32 = 0xA1A1AA;
const TEXT_MUTED: u32 = 0x71717A;

const BADGE_EMERALD_BG: u32 = 0x143327;
const BADGE_EMERALD_FG: u32 = 0x34D399;
const BADGE_AMBER_FG: u32 = 0xFBBF24;
const BADGE_SLATE_BG: u32 = 0x2D2D35;
const BADGE_SLATE_FG: u32 = 0xE4E4E7;
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

/// Renders either the Clean Arcade Window (`720x912`, `show_telemetry = false`)
/// or the Expanded Hardware & Game State Window (`1160x912`, `show_telemetry = true`).
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

    // 1. Sleek Top Control Instruction Bar (x: 20..700, y: 6..29)
    draw_card(window_fb, fb_w, 20, 6, 680, 23);
    let controls_top: [(&str, &str, usize); 6] = [
        ("5/C", "COIN", 28),
        ("1/ENT", "START", 118),
        ("WASD", "MOVE", 224),
        ("SPC/Z", "PUMP", 320),
        ("R", "RESET", 424),
        ("TAB", "INFO", 512),
    ];
    for &(key_txt, label_txt, bx) in &controls_top {
        draw_badge(
            window_fb,
            fb_w,
            bx,
            10,
            key_txt,
            BADGE_SLATE_BG,
            BADGE_AMBER_FG,
        );
        let label_x = bx + key_txt.len() * 6 + 16;
        draw_text(window_fb, fb_w, label_x, 14, label_txt, TEXT_PRIMARY, 1);
    }
    let snap = machine.snapshot();
    draw_badge(
        window_fb,
        fb_w,
        604,
        10,
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

    // 3. Right-side Sleek Modern Minimalist Hardware & Game State Panel
    let px = 716;
    let pw = 420;

    // --- CARD 1: Header & Arcade Cabinet Status ---
    draw_card(window_fb, fb_w, px, 6, pw, 126);
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        20,
        "DIG DUG (NAMCO, 1982)",
        TEXT_PRIMARY,
        2,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        42,
        "TRIPLE-Z80 ARCADE HARDWARE & MEMORY INSPECTOR",
        TEXT_SECONDARY,
        1,
    );

    draw_text(window_fb, fb_w, px + 16, 70, "GAME MODE", TEXT_MUTED, 1);
    let mode_label = if snap.in_game {
        "ACTIVE GAMEPLAY"
    } else if snap.attract_demo {
        "ATTRACT DEMO"
    } else {
        "TITLE / STANDBY"
    };
    draw_badge(
        window_fb,
        fb_w,
        px + 112,
        66,
        mode_label,
        BADGE_EMERALD_BG,
        BADGE_EMERALD_FG,
    );

    draw_text(window_fb, fb_w, px + 16, 98, "ROUND 256", TEXT_MUTED, 1);
    draw_badge(
        window_fb,
        fb_w,
        px + 112,
        94,
        "KILL-SCREEN FIX ACTIVE (-> RND 156)",
        BADGE_SLATE_BG,
        BADGE_SLATE_FG,
    );

    // --- CARD 2: Tri-CPU State & Namco Custom Silicon ---
    draw_card(window_fb, fb_w, px, 144, pw, 134);
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        156,
        "TRI-CPU STATE & NAMCO WSG AUDIO",
        TEXT_PRIMARY,
        1,
    );
    draw_hline(window_fb, fb_w, px + 16, 170, pw - 32, CARD_BORDER);

    let c0 = &machine.cpus[0];
    let c1 = &machine.cpus[1];
    let c2 = &machine.cpus[2];
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        180,
        &format!(
            "CPU0 MAIN : PC ${:04X}  SP ${:04X}  AF ${:04X}  IRQ:{}",
            c0.pc,
            c0.sp,
            c0.af(),
            if machine.bus.irq_enable[0] { "ON " } else { "OFF" }
        ),
        TEXT_PRIMARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        198,
        &format!(
            "CPU1 AI   : PC ${:04X}  SP ${:04X}  AF ${:04X}  {}",
            c1.pc,
            c1.sp,
            c1.af(),
            if machine.bus.sub_cpu_reset {
                "RST"
            } else if machine.bus.irq_enable[1] {
                "RUN"
            } else {
                "IDL"
            }
        ),
        TEXT_PRIMARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        216,
        &format!(
            "CPU2 SND  : PC ${:04X}  SP ${:04X}  AF ${:04X}  NMI:{}",
            c2.pc,
            c2.sp,
            c2.af(),
            if !machine.bus.irq_enable[2] { "ON " } else { "OFF" }
        ),
        TEXT_PRIMARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        238,
        &format!(
            "06XX CMD: ${:02X}   51XX MODE: {}   VID LATCH: ${:02X}",
            machine.bus.namco_command, machine.bus.namco_mode, machine.bus.video_latch
        ),
        TEXT_SECONDARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        256,
        &format!(
            "WSG CH0-2 VOL: [{:X}, {:X}, {:X}]   FRAME: #{}",
            machine.bus.sound_regs[0x15] & 0xF,
            machine.bus.sound_regs[0x1A] & 0xF,
            machine.bus.sound_regs[0x1F] & 0xF,
            machine.frame_count
        ),
        TEXT_SECONDARY,
        1,
    );

    // --- CARD 3: Live Reverse-Engineered Shared RAM ($8000-$9FFF) ---
    draw_card(window_fb, fb_w, px, 290, pw, 224);
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        302,
        "SHARED RAM TELEMETRY ($8000-$9FFF)",
        TEXT_PRIMARY,
        1,
    );
    draw_hline(window_fb, fb_w, px + 16, 316, pw - 32, CARD_BORDER);

    draw_text(
        window_fb,
        fb_w,
        px + 16,
        326,
        &format!(
            "CREDITS: {:02X}   P1 ROUND ($840D): {:3}   LIVES ($840A): {}",
            snap.credits_bcd, snap.p1_round, snap.p1_lives
        ),
        TEXT_PRIMARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        344,
        &format!(
            "DIG DUG ($98A2): X={:3} Y={:3}   1UP: {:06}  HI: {:06}",
            snap.digdug_x, snap.digdug_y, snap.p1_score, snap.high_score
        ),
        TEXT_SECONDARY,
        1,
    );

    // Monster Speed $87BD + Progress Bar
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        366,
        &format!("MONSTER SPEED ($87BD): {:3}/255", snap.monster_speed),
        TEXT_PRIMARY,
        1,
    );
    let bar_x = px + 240;
    let bar_w = 160;
    draw_rect(window_fb, fb_w, bar_x, 366, bar_w, 8, BADGE_SLATE_BG);
    let fill_w = (snap.monster_speed as usize * bar_w) / 255;
    let bar_col = if snap.monster_speed == 255 {
        BADGE_ROSE_FG
    } else if snap.monster_speed > 180 {
        BADGE_AMBER_FG
    } else {
        BADGE_EMERALD_FG
    };
    draw_rect(window_fb, fb_w, bar_x, 366, fill_w, 8, bar_col);

    // Monster table ($8525 + i*16)
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        388,
        "SLOT  TYPE    POS ($8529/$852A)   STATE / PUMP",
        TEXT_MUTED,
        1,
    );
    for m in 0..6 {
        let my = 404 + m * 16;
        let mtype = if (snap.monster_type_mask & (1 << m)) != 0 {
            "FYGAR"
        } else {
            "POOKA"
        };
        let mx_pos = snap.monster_x[m];
        let my_pos = snap.monster_y[m];
        let inflate = snap.monster_inflate[m] & 0x07;
        let is_ghost = (snap.monster_ghost_mask & (1 << m)) != 0;
        let active = mx_pos != 0 && my_pos != 0;
        let col = if active && is_ghost {
            BADGE_AMBER_FG
        } else if active {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        let state_str = if is_ghost && active {
            "GHOSTING ".to_string()
        } else {
            format!("STAGE {}/4", inflate)
        };
        draw_text(
            window_fb,
            fb_w,
            px + 16,
            my,
            &format!(
                " #{m}   {mtype}   X={:3}  Y={:3}        {}",
                mx_pos, my_pos, state_str
            ),
            col,
            1,
        );
    }

    // --- CARD 4: Subroutine Execution Telemetry ---
    draw_card(window_fb, fb_w, px, 526, pw, 204);
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        538,
        &format!(
            "SUBROUTINE TELEMETRY ({} TOTAL CALLS)",
            machine.decomp.total_rust_calls
        ),
        TEXT_PRIMARY,
        1,
    );
    draw_hline(window_fb, fb_w, px + 16, 552, pw - 32, CARD_BORDER);

    let hooks: [(&str, u64); 7] = [
        ("CPU0/1/2 $0008/$10 rst_table_add", machine.decomp.rst_table_calls),
        (
            "CPU0 $0FF0..$2894 harpoon_inflate_pop",
            machine.decomp.harpoon_pump_calls,
        ),
        (
            "CPU0 $1C10..$290C rock_crush_excavate",
            machine.decomp.rock_physics_calls,
        ),
        (
            "CPU0 $243F..CPU1 $15D3 ghost_dig_ai",
            machine.decomp.ghost_dig_ai_calls + machine.decomp.monster_speed_calls,
        ),
        (
            "CPU0 $0C51..$1845 bcd_score_1up_hud",
            machine.decomp.bcd_score_calls + machine.decomp.hud_lives_calls,
        ),
        (
            "CPU0 $177C..$19F7 round_and_grid_init",
            machine.decomp.round_advance_calls + machine.decomp.dirt_init_calls,
        ),
        ("CPU2 $0018 namco_wsg_sound_engine", machine.decomp.sound_engine_calls),
    ];

    for (i, (label, count)) in hooks.iter().enumerate() {
        let hy = 564 + i * 19;
        let active_col = if *count > 0 {
            BADGE_EMERALD_FG
        } else {
            TEXT_SECONDARY
        };
        draw_text(window_fb, fb_w, px + 16, hy, label, active_col, 1);
        draw_text(
            window_fb,
            fb_w,
            px + 320,
            hy,
            &format!("{:8}", count),
            TEXT_PRIMARY,
            1,
        );
    }
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        706,
        &format!(
            "CALLS/FRAME: {:4}   LAST: {}",
            machine.decomp.calls_this_frame, machine.decomp.last_hooked_name
        ),
        TEXT_MUTED,
        1,
    );

    // --- CARD 5: Interactive Controls ---
    draw_card(window_fb, fb_w, px, 742, pw, 162);
    draw_text(window_fb, fb_w, px + 16, 754, "CONTROLS", TEXT_PRIMARY, 1);
    draw_hline(window_fb, fb_w, px + 16, 768, pw - 32, CARD_BORDER);

    draw_text(
        window_fb,
        fb_w,
        px + 16,
        784,
        "[5 / C] INSERT COIN          [1 / ENTER] 1P START",
        TEXT_SECONDARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        808,
        "[ARROWS / WASD] MOVE         [SPACE / Z] PUMP HARPOON",
        TEXT_SECONDARY,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        832,
        "[TAB] TOGGLE INFO PANEL      [R] WARM RESET CABINET",
        BADGE_AMBER_FG,
        1,
    );
    draw_text(
        window_fb,
        fb_w,
        px + 16,
        856,
        "[ESC] QUIT GAME",
        TEXT_MUTED,
        1,
    );

    fb_w
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

fn draw_badge(fb: &mut [u32], fb_w: usize, x: usize, y: usize, text: &str, bg: u32, fg: u32) {
    let w = text.len() * 6 + 12;
    let h = 15;
    draw_rect(fb, fb_w, x, y, w, h, bg);
    draw_hline(fb, fb_w, x, y, w, CARD_BORDER);
    draw_hline(fb, fb_w, x, y + h - 1, w, CARD_BORDER);
    draw_text(fb, fb_w, x + 6, y + 4, text, fg, 1);
}

fn draw_text(fb: &mut [u32], fb_w: usize, x: usize, y: usize, text: &str, color: u32, scale: usize) {
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

/// Clean 5x7 uppercase/symbol bitmap font for the Sleek Modern Minimalist UI panel.
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
