//! Runs a scripted input sequence on our emulator and prints every CPU access to the Namco
//! 06XX/51XX window ($7000..$71FF) tagged with the script step, for diffing against MAME.
//!
//! Usage: `cargo run --release --bin trace51 -- mame_ref/inputscript.txt`
//! Output lines: `S<step> <R|W> <addr hex> <value hex>`
use std::path::Path;

use digdug_re::hardware::DigDugMachine;
use digdug_re::rom::DigDugRoms;

fn parse_script(path: &str) -> Vec<(u8, u32)> {
    std::fs::read_to_string(path)
        .expect("cannot read input script")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut it = l.split_whitespace();
            let b = u8::from_str_radix(it.next().unwrap(), 16).unwrap();
            let n: u32 = it.next().unwrap().parse().unwrap();
            (b, n)
        })
        .collect()
}

fn main() {
    let script_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mame_ref/inputscript.txt".into());
    let script = parse_script(&script_path);
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
        .expect("ROMs missing in roms/extracted");
    let mut machine = DigDugMachine::new(roms);
    machine.bus.trace = Some(Vec::new());
    for (step, (buttons, frames)) in script.iter().enumerate() {
        machine.bus.buttons = *buttons;
        for _ in 0..*frames {
            machine.run_frame();
        }
        for (kind, addr, val) in machine.bus.trace.as_mut().unwrap().drain(..) {
            println!("S{} {} {:04X} {:02X}", step, kind, addr, val);
        }
    }
}
