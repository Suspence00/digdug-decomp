use std::path::Path;
use std::time::Instant;

use digdug_re::audio::NamcoWsgAudio;
use digdug_re::hardware::{
    DigDugMachine, INPUT_COIN, INPUT_DOWN, INPUT_FIRE, INPUT_LEFT, INPUT_RIGHT, INPUT_START1,
    INPUT_UP,
};
use digdug_re::rom::DigDugRoms;
use digdug_re::video::{
    render_arcade_frame, render_workbench_window, save_png, ARCADE_HEIGHT, ARCADE_WIDTH,
    ARCADE_WINDOW_WIDTH, TELEMETRY_WINDOW_WIDTH, WINDOW_HEIGHT,
};
use minifb::{Key, KeyRepeat, Window, WindowOptions};

fn show_rom_error_and_exit(err: &str) -> ! {
    let msg = format!(
        "{}\n\nHow to fix:\n\
         • Place 'digdug.zip' (MAME Rev 2 ROM set) in the same folder as digdug.exe, OR\n\
         • Extract the ROM files into 'roms/extracted/' next to digdug.exe.",
        err
    );
    eprintln!("\n[ERROR] {}", msg);
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        let title_w: Vec<u16> = "Dig Dug — Missing Arcade ROMs\0".encode_utf16().collect();
        let body_w: Vec<u16> = format!("{}\0", msg).encode_utf16().collect();
        MessageBoxW(
            std::ptr::null_mut(),
            body_w.as_ptr(),
            title_w.as_ptr(),
            MB_ICONERROR | MB_OK,
        );
    }
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_dir = Path::new("roms/extracted");

    let t0 = Instant::now();
    let initial_roms = match DigDugRoms::load_from_extracted_dir(rom_dir) {
        Ok(r) => r,
        Err(e) => show_rom_error_and_exit(&e),
    };
    println!("[0a] ROMs loaded in {:?}", t0.elapsed());

    let t1 = Instant::now();
    let mut machine = DigDugMachine::new(initial_roms);
    println!("[0b] Machine booted to title screen in {:?}", t1.elapsed());
    let mut arcade_fb = Box::new([0u32; ARCADE_WIDTH * ARCADE_HEIGHT]);
    let mut window_fb = vec![0u32; TELEMETRY_WINDOW_WIDTH * WINDOW_HEIGHT];
    let mut show_telemetry = false;

    // Optional automated headless verification mode (`cargo run --release --bin digdug -- --headless-test`)
    if args.iter().any(|a| a == "--headless-test") {
        println!("Running Dig Dug headless verification...");
        // Simulate inserting a coin, starting a 1-Player game, waiting for opening fanfare,
        // and then digging and firing the harpoon during active Round 1 gameplay!
        let mut saw_harpoon = false;
        let mut active_wsg_tick0 = 0usize;
        let mut active_wsg_tick1 = 0usize;
        for f in 0..700 {
            machine.bus.buttons = match f {
                5..=10 => INPUT_COIN,
                25..=30 => INPUT_START1,
                535..=575 => INPUT_RIGHT,
                580..=600 => INPUT_FIRE,
                615..=655 => INPUT_DOWN,
                660..=680 => INPUT_FIRE,
                _ => 0,
            };
            machine.run_frame();
            let s = machine.snapshot();
            if s.harpoon_active {
                saw_harpoon = true;
            }
            if (machine.bus.sound_regs_tick0[0x15] & 0x0F) > 0
                || (machine.bus.sound_regs_tick0[0x1A] & 0x0F) > 0
                || (machine.bus.sound_regs_tick0[0x1F] & 0x0F) > 0
            {
                active_wsg_tick0 += 1;
            }
            if (machine.bus.sound_regs[0x15] & 0x0F) > 0
                || (machine.bus.sound_regs[0x1A] & 0x0F) > 0
                || (machine.bus.sound_regs[0x1F] & 0x0F) > 0
            {
                active_wsg_tick1 += 1;
            }
        }
        println!(
            "  Saw harpoon fire: {} | 120Hz WSG ticks active: ({}, {})",
            saw_harpoon, active_wsg_tick0, active_wsg_tick1
        );
        assert!(
            saw_harpoon,
            "Harpoon / pump must fire and extend during active gameplay!"
        );
        assert!(
            active_wsg_tick0 > 100 && active_wsg_tick1 > 100,
            "Both 120Hz sub-frame Sound CPU ticks must produce active Namco WSG audio!"
        );
        render_arcade_frame(
            &machine.bus.shared_ram,
            machine.bus.video_latch,
            &machine.bus.roms,
            &mut arcade_fb,
        );
        let fb_w = render_workbench_window(&machine, &arcade_fb, &mut window_fb, true);
        assert_eq!(fb_w, TELEMETRY_WINDOW_WIDTH);
        if args.iter().any(|a| a == "--update-screenshots") {
            let _ = std::fs::create_dir_all("docs");
            save_png(
                Path::new("docs/screenshot_inspector.png"),
                &window_fb,
                TELEMETRY_WINDOW_WIDTH,
                WINDOW_HEIGHT,
            )
            .expect("Failed to write docs/screenshot_inspector.png");
        }

        let fb_w_clean = render_workbench_window(&machine, &arcade_fb, &mut window_fb, false);
        assert_eq!(fb_w_clean, ARCADE_WINDOW_WIDTH);
        if args.iter().any(|a| a == "--update-screenshots") {
            save_png(
                Path::new("docs/screenshot_arcade.png"),
                &window_fb,
                ARCADE_WINDOW_WIDTH,
                WINDOW_HEIGHT,
            )
            .expect("Failed to write docs/screenshot_arcade.png");
        }

        let lit_arcade_pixels = arcade_fb.iter().filter(|&&px| px != 0).count();
        let snap = machine.snapshot();
        println!(
            "  Frame #{}: game_started={}, CPU0 PC=${:04X}, CPU1 PC=${:04X}, CPU2 PC=${:04X}",
            machine.frame_count,
            machine.bus.game_started,
            machine.cpus[0].pc,
            machine.cpus[1].pc,
            machine.cpus[2].pc
        );
        println!(
            "  Subroutine calls: {}",
            machine.decomp.total_rust_calls,
        );
        println!(
            "  Call breakdown -> RST $08/$10: {}, harpoon_inflate_pop: {}, rock_physics: {}, ghost_dig_ai: {}, bcd_score_1up: {}, dirt_init: {}, hud_lives: {}, round_adv: {}, monster_spd: {}, wsg_snd: {}",
            machine.decomp.rst_table_calls,
            machine.decomp.harpoon_pump_calls,
            machine.decomp.rock_physics_calls,
            machine.decomp.ghost_dig_ai_calls,
            machine.decomp.bcd_score_calls,
            machine.decomp.dirt_init_calls,
            machine.decomp.hud_lives_calls,
            machine.decomp.round_advance_calls,
            machine.decomp.monster_speed_calls,
            machine.decomp.sound_engine_calls,
        );
        println!(
            "  Arcade CRT lit pixels: {} / {} (Active enemies: {}, Round: {}, Lives: {}, High Score: {})",
            lit_arcade_pixels,
            ARCADE_WIDTH * ARCADE_HEIGHT,
            snap.active_enemies,
            snap.p1_round,
            snap.p1_lives,
            snap.high_score
        );
        assert!(
            machine.bus.game_started,
            "Machine should have passed power-on self-test!"
        );
        assert!(
            lit_arcade_pixels > 1000,
            "Arcade CRT framebuffer should have rendered title/in-game graphics!"
        );
        assert!(
            machine.decomp.total_rust_calls > 100,
            "Subroutines should be actively executing!"
        );
        println!("Headless verification PASSED!");
        return;
    }

    println!(
        "[1/3] ({:?}) Opening Arcade Window ({}x{})...",
        t0.elapsed(),
        ARCADE_WINDOW_WIDTH,
        WINDOW_HEIGHT
    );

    #[cfg(windows)]
    let (target_x, target_y) = unsafe {
        use windows_sys::Win32::Foundation::{POINT, RECT};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect, SM_CXSCREEN,
            SM_CYSCREEN,
        };
        let fg_hwnd = GetForegroundWindow();
        let mut fg_rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if !fg_hwnd.is_null() {
            GetWindowRect(fg_hwnd, &mut fg_rect);
        }
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);

        let (cx, cy) = if fg_rect.right > fg_rect.left && fg_rect.bottom > fg_rect.top {
            (
                (fg_rect.left + fg_rect.right) / 2,
                (fg_rect.top + fg_rect.bottom) / 2,
            )
        } else if cursor.x != 0 || cursor.y != 0 {
            (cursor.x, cursor.y)
        } else {
            (screen_w / 2, screen_h / 2)
        };
        (
            (cx - (ARCADE_WINDOW_WIDTH as i32) / 2).max(20),
            (cy - (WINDOW_HEIGHT as i32) / 2).max(20),
        )
    };

    let mut window = Window::new(
        "Dig Dug  —  [5/C] Coin   [1/Enter] Start   [Arrows/WASD] Move   [Space/Z] Pump   [R] Reset   [Tab] Info",
        ARCADE_WINDOW_WIDTH,
        WINDOW_HEIGHT,
        WindowOptions {
            resize: true,
            topmost: false,
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )
    .expect("Failed to open arcade window");

    window.set_target_fps(60);

    // Immediately render and present the first frame so the window appears on screen right away!
    render_arcade_frame(
        &machine.bus.shared_ram,
        machine.bus.video_latch,
        &machine.bus.roms,
        &mut arcade_fb,
    );
    let fb_w = render_workbench_window(&machine, &arcade_fb, &mut window_fb, show_telemetry);
    window
        .update_with_buffer(&window_fb[..fb_w * WINDOW_HEIGHT], fb_w, WINDOW_HEIGHT)
        .expect("Failed to update initial window framebuffer");

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_SHOWNORMAL,
        };
        let hwnd = window.get_window_handle() as windows_sys::Win32::Foundation::HWND;
        if !hwnd.is_null() {
            window.set_position(target_x as isize, target_y as isize);
            ShowWindow(hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(hwnd);
        }
    }

    println!("[2/3] ({:?}) Starting Background Namco WSG Audio Thread...", t0.elapsed());
    let mut audio = NamcoWsgAudio::new();
    let test_window_frames = args.iter().any(|a| a == "--test-window");
    let mut loop_frames = 0u32;

    println!(
        "[3/3] ({:?}) Dig Dug Running! (Controls: [5/C] Coin, [1/Enter] Start, [Arrows/WASD] Move, [Space/Z] Pump, [Tab] Telemetry, [R] Reset)",
        t0.elapsed()
    );

    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Toggle Live Rust Telemetry drawer on [Tab]
        if window.is_key_pressed(Key::Tab, KeyRepeat::No) {
            show_telemetry = !show_telemetry;
            #[cfg(windows)]
            unsafe {
                use windows_sys::Win32::Foundation::RECT;
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    GetWindowRect, SetWindowPos, SWP_NOMOVE, SWP_NOZORDER, SWP_SHOWWINDOW,
                };
                let hwnd = window.get_window_handle() as windows_sys::Win32::Foundation::HWND;
                if !hwnd.is_null() {
                    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    if GetWindowRect(hwnd, &mut rect) != 0 {
                        let cur_w = rect.right - rect.left;
                        let cur_h = rect.bottom - rect.top;
                        let delta_w = (TELEMETRY_WINDOW_WIDTH as i32) - (ARCADE_WINDOW_WIDTH as i32);
                        let new_w = if show_telemetry {
                            cur_w + delta_w
                        } else {
                            (cur_w - delta_w).max(ARCADE_WINDOW_WIDTH as i32)
                        };
                        SetWindowPos(
                            hwnd,
                            std::ptr::null_mut(),
                            0,
                            0,
                            new_w,
                            cur_h,
                            SWP_NOMOVE | SWP_NOZORDER | SWP_SHOWWINDOW,
                        );
                    }
                }
            }
        }

        // Warm-reset cabinet on [R] (preserving non-volatile ER2055 EAROM high-score chip)
        if window.is_key_pressed(Key::R, KeyRepeat::No) {
            machine.warm_reset();
        }

        // Arcade cabinet inputs (4-way priority on joystick)
        let mut buttons = 0u8;
        if window.is_key_down(Key::Up) || window.is_key_down(Key::W) {
            buttons |= INPUT_UP;
        } else if window.is_key_down(Key::Down) || window.is_key_down(Key::S) {
            buttons |= INPUT_DOWN;
        } else if window.is_key_down(Key::Left) || window.is_key_down(Key::A) {
            buttons |= INPUT_LEFT;
        } else if window.is_key_down(Key::Right) || window.is_key_down(Key::D) {
            buttons |= INPUT_RIGHT;
        }

        if window.is_key_down(Key::Space)
            || window.is_key_down(Key::Z)
            || window.is_key_down(Key::LeftCtrl)
        {
            buttons |= INPUT_FIRE;
        }
        if window.is_key_down(Key::Key1) || window.is_key_down(Key::Enter) {
            buttons |= INPUT_START1;
        }
        if window.is_key_down(Key::Key5) || window.is_key_down(Key::C) {
            buttons |= INPUT_COIN;
        }
        machine.bus.buttons = buttons;

        // Step one 60Hz frame of the native Rust engine
        machine.run_frame();

        // Stream both 120Hz sub-frame ticks of 3-channel Namco WSG audio
        audio.push_frame(
            &machine.bus.sound_regs_tick0,
            &machine.bus.sound_regs,
            &machine.bus.roms.sound_wavetable,
        );

        // Render 224x288 arcade CRT + optional [Tab] Telemetry Drawer
        render_arcade_frame(
            &machine.bus.shared_ram,
            machine.bus.video_latch,
            &machine.bus.roms,
            &mut arcade_fb,
        );
        let fb_w = render_workbench_window(&machine, &arcade_fb, &mut window_fb, show_telemetry);

        window
            .update_with_buffer(&window_fb[..fb_w * WINDOW_HEIGHT], fb_w, WINDOW_HEIGHT)
            .expect("Failed to update window framebuffer");

        loop_frames += 1;
        if test_window_frames && loop_frames >= 10 {
            println!("({:?}) 10 window frames rendered and presented successfully!", t0.elapsed());
            break;
        }
    }
}
