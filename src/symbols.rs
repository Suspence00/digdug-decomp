/// Hardware memory-mapped I/O, RAM, and subroutine symbol tables for Dig Dug's 3x Z80 architecture.

pub fn resolve_mem_symbol(addr: u16) -> Option<&'static str> {
    match addr {
        // Sound registers (Namco WSG 3-voice waveform, frequency & volume registers)
        0x6805 => Some("WSG_CH0_WAVE"),
        0x680A => Some("WSG_CH1_WAVE"),
        0x680F => Some("WSG_CH2_WAVE"),
        0x6810..=0x6814 => Some("WSG_CH0_FREQ"),
        0x6815 => Some("WSG_CH0_VOL"),
        0x6816..=0x6819 => Some("WSG_CH1_FREQ"),
        0x681A => Some("WSG_CH1_VOL"),
        0x681B..=0x681E => Some("WSG_CH2_FREQ"),
        0x681F => Some("WSG_CH2_VOL"),
        0x6800..=0x681F => Some("WSG_SOUND_REG"),

        // LS259 Misc Control Latch (8R)
        0x6820 => Some("LATCH_IRQ1_ENABLE"),       // Main CPU VBlank IRQ enable/ack
        0x6821 => Some("LATCH_IRQ2_ENABLE"),       // Sub CPU VBlank IRQ enable/ack
        0x6822 => Some("LATCH_SOUND_NMI_DISABLE"), // Sound CPU NMI disable (0 = enabled)
        0x6823 => Some("LATCH_SUB_CPU_RESET"),     // 0 = hold Sub/Sound CPUs & 51XX/53XX in reset
        0x6825 => Some("LATCH_53XX_MOD0"),
        0x6826 => Some("LATCH_53XX_MOD1"),
        0x6827 => Some("LATCH_53XX_MOD2"),

        // Watchdog timer
        0x6830 => Some("WATCHDOG_RESET"),

        // Namco 06XX custom I/O bus interface (talks to 51XX inputs/coins & 53XX DIP switches)
        0x7000..=0x700F => Some("NAMCO_06XX_DATA"),
        0x7100 => Some("NAMCO_06XX_CTRL"),

        // Shared RAM 0: Foreground Tilemap VRAM (0x8000..0x83FF) + Work RAM (0x8400..0x87FF)
        0x8000 => Some("VRAM_TILEMAP_BASE"),
        0x83D9 => Some("VRAM_HUD_1UP_U"),
        0x8400 => Some("WORK_RAM_BASE"),

        // Shared RAM 1: Object / Work RAM (0x8800..0x8B7F) + Sprite Code/Color (0x8B80..0x8BFF)
        0x8800 => Some("OBJ_WORK_RAM_BASE"),
        0x8900 => Some("IO_NMI_TRANSFER_PTR"),
        0x8B80 => Some("SPRITE_CODE_COLOR_BASE"),

        // Shared RAM 2: Position / Work RAM (0x9000..0x937F) + Sprite X/Y (0x9380..0x93FF)
        0x9000 => Some("POS_WORK_RAM_BASE"),
        0x9380 => Some("SPRITE_POS_XY_BASE"),

        // Shared RAM 3: Flip / Work RAM (0x9800..0x9B7F) + Sprite Flip/Enable (0x9B80..0x9BFF)
        0x9800 => Some("FLP_WORK_RAM_BASE"),
        0x9A80 => Some("CPU1_STACK_TOP"),
        0x9B3C => Some("SOUND_NMI_BUSY_FLAG"),
        0x9B80 => Some("SPRITE_FLIP_ENABLE_BASE"),

        // Video Control Latch (LS259 @ 5R)
        0xA000 => Some("VIDLATCH_BG_SELECT_BIT0"),
        0xA001 => Some("VIDLATCH_BG_SELECT_BIT1"),
        0xA002 => Some("VIDLATCH_TX_COLOR_MODE"),
        0xA003 => Some("VIDLATCH_BG_DISABLE"),
        0xA004 => Some("VIDLATCH_BG_COLOR_BANK0"),
        0xA005 => Some("VIDLATCH_BG_COLOR_BANK1"),
        0xA007 => Some("VIDLATCH_FLIP_SCREEN"),

        // High-score Non-Volatile EAROM (ER2055)
        0xB800..=0xB83F => Some("EAROM_DATA"),
        0xB840 => Some("EAROM_CTRL"),

        _ => None,
    }
}

pub fn resolve_code_symbol(cpu_id: usize, addr: u16) -> Option<(&'static str, &'static str)> {
    match (cpu_id, addr) {
        // CPU 0 (Main Z80)
        (0, 0x0000) => Some(("cpu0_reset", "Reset vector: DI; IM 1; JP 0x00E6")),
        (0, 0x0008) => Some(("rst_08_table_add", "RST 08h: HL += 2*A pointer table indexing")),
        (0, 0x0010) => Some(("rst_10_hl_plus_a", "RST 10h: HL += A byte table indexing")),
        (0, 0x0018) => Some((
            "dormant_round_rollover",
            "Dormant Round 256 -> 156 rollover routine (unused in Namco, activated in Atari!)",
        )),
        (0, 0x0028) => Some(("rst_28_jump_table", "RST 28h: Indirect jump via table at (SP)")),
        (0, 0x0038) => Some(("cpu0_vblank_irq", "IM 1 VBlank Interrupt Vector -> JP 0x0280")),
        (0, 0x0066) => Some((
            "cpu0_nmi_06xx_io",
            "NMI Handler: Asynchronous data transfer with Namco 06XX (51XX/53XX I/O)",
        )),
        (0, 0x00E6) => Some(("cpu0_boot_init", "Main CPU hardware & RAM self-test / boot init")),
        (0, 0x0280) => Some(("cpu0_vblank_handler", "Main CPU VBlank frame tick handler")),

        // CPU 1 (Sub / Enemy AI Z80)
        (1, 0x0000) => Some(("cpu1_reset", "Sub CPU Reset vector: DI; IM 1; JP 0x0068")),
        (1, 0x0008) => Some(("cpu1_rst_08", "RST 08h: HL += 2*A table indexing")),
        (1, 0x0010) => Some(("cpu1_rst_10", "RST 10h: HL += A table indexing")),
        (1, 0x0038) => Some(("cpu1_vblank_irq", "Sub CPU VBlank IRQ wrapper -> CALL 0x0080")),
        (1, 0x0066) => Some(("cpu1_nmi_noop", "Sub CPU NMI handler (immediate RETN + init entry at 0x0068)")),
        (1, 0x0068) => Some(("cpu1_boot_init", "Sub CPU boot initialization & main AI loop")),
        (1, 0x0080) => Some(("cpu1_vblank_handler", "Sub CPU VBlank handler (enemy AI & sprite updates)")),

        // CPU 2 (Sound Z80)
        (2, 0x0000) => Some(("cpu2_reset", "Sound CPU Reset vector: LD SP, 0x8B80; JP 0x0085")),
        (2, 0x0008) => Some(("cpu2_rst_08", "RST 08h: HL += 2*A table indexing")),
        (2, 0x0010) => Some(("cpu2_rst_10", "RST 10h: HL += A table indexing")),
        (2, 0x0066) => Some(("cpu2_nmi_audio_tick", "Sound CPU periodic NMI timer interrupt (120 Hz audio tick)")),
        (2, 0x0085) => Some(("cpu2_main_loop", "Sound CPU initialization & background sound sequencer loop")),
        _ => None,
    }
}
