//! Dumps the shared RAM window ($8000-$9FFF) of our emulator after N extra frames,
//! as one hex line, for differential comparison against MAME (see `mame_ref/`).
//!
//! Usage: `cargo run --release --bin dumpram -- <extra_frames>`
use std::path::Path;

use digdug_re::hardware::DigDugMachine;
use digdug_re::rom::DigDugRoms;

fn main() {
    let extra: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
        .expect("ROMs missing in roms/extracted");
    let mut machine = DigDugMachine::new(roms);
    for _ in 0..extra {
        machine.run_frame();
    }
    let hex: String = machine
        .bus
        .shared_ram
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect();
    println!("frame_count={}", machine.frame_count);
    println!("{}", hex);
}
