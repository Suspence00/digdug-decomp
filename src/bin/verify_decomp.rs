//! Differential verification driver: boots the machine from power-on with every CPU step
//! cross-checked against the reference Z80 interpreter (original ROM bytes), optionally while
//! playing back a scripted input sequence, then prints every routine whose Rust translation
//! disagrees with the ROM.
//!
//! Usage:
//!   cargo run --release --bin verify_decomp -- [--frames N] [--script FILE] [--script-start F]
//!                                              [--max-instr N] [--top N]
use std::path::Path;

use digdug_re::hardware::DigDugMachine;
use digdug_re::rom::DigDugRoms;
use digdug_re::verify::VerifyLog;

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
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
    };
    let frames: u64 = get("--frames").and_then(|s| s.parse().ok()).unwrap_or(1200);
    let script_path = get("--script");
    let script_start: u64 = get("--script-start").and_then(|s| s.parse().ok()).unwrap_or(200);
    let max_instr: u64 = get("--max-instr").and_then(|s| s.parse().ok()).unwrap_or(300_000);
    let top: usize = get("--top").and_then(|s| s.parse().ok()).unwrap_or(40);

    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))
        .expect("ROMs missing in roms/extracted");
    let mut machine = DigDugMachine::new_unbooted(roms);
    machine.verify = Some(Box::new(VerifyLog::new(max_instr)));

    let script = script_path.as_deref().map(parse_script).unwrap_or_default();
    // Expand the script into a per-frame button table.
    let mut per_frame: Vec<u8> = Vec::new();
    for (b, n) in &script {
        for _ in 0..*n {
            per_frame.push(*b);
        }
    }

    let t0 = std::time::Instant::now();
    for f in 0..frames {
        machine.bus.buttons = if f >= script_start {
            per_frame.get((f - script_start) as usize).copied().unwrap_or(0)
        } else {
            0
        };
        machine.run_frame();
        if f % 200 == 199 {
            eprintln!("  ... frame {}/{} ({:.1}s)", f + 1, frames, t0.elapsed().as_secs_f32());
        }
    }

    let log = machine.verify.take().unwrap();
    println!("=== Differential verification report ===");
    println!("frames run:        {}", frames);
    println!("steps verified:    {}", log.checked);
    println!("steps skipped (halted): {}", log.skipped_halted);
    println!(
        "ref timeouts:      {} distinct units ({} total)",
        log.timeouts.len(),
        log.timeouts.values().sum::<u64>()
    );
    println!("unsupported ops:   {:?}", log.unsupported);
    println!("mismatching units: {}", log.mismatches.len());

    // Severity: STATE (real) > REG-PROPAGATES (real) > REG-BENIGN (dead value).
    let sev = |m: &digdug_re::verify::Mismatch| -> u8 {
        if m.state_count > 0 {
            0
        } else if m.reg_propagates > 0 {
            1
        } else {
            2
        }
    };
    let mut rows: Vec<_> = log.mismatches.iter().collect();
    rows.sort_by(|a, b| sev(a.1).cmp(&sev(b.1)).then(b.1.count.cmp(&a.1.count)));
    let n_state = rows.iter().filter(|r| sev(r.1) == 0).count();
    let n_prop = rows.iter().filter(|r| sev(r.1) == 1).count();
    let n_benign = rows.iter().filter(|r| sev(r.1) == 2).count();
    println!(
        "  STATE (real): {}   REG-PROPAGATES (real): {}   REG-BENIGN (dead values): {}",
        n_state, n_prop, n_benign
    );
    for ((cpu, pc), m) in rows.iter().take(top) {
        let class = ["STATE", "REG-PROPAGATES", "REG-BENIGN"][sev(m) as usize];
        println!(
            "\n--- [{}] cpu{} ${:04X}  [{}]  total={} state={} (first_state@frame {}) reg-propagates={} reg-benign={} reg-unsampled={}  first@frame {}",
            class, cpu, pc, m.hooked_name, m.count, m.state_count, m.first_state_frame, m.reg_propagates,
            m.reg_benign, m.reg_unsampled, m.first_frame
        );
        let ex = match sev(m) {
            0 => &m.state_example,
            1 => &m.propagate_example,
            _ => &m.reg_example,
        };
        for d in ex.iter().take(30) {
            println!("      {}", d);
        }
    }
    if !log.timeouts.is_empty() {
        let mut t: Vec<_> = log.timeouts.iter().collect();
        t.sort_by(|a, b| b.1.cmp(a.1));
        println!("\nTop reference timeouts (unit start pc: count):");
        for ((cpu, pc), n) in t.iter().take(15) {
            println!("   cpu{} ${:04X}: {}", cpu, pc, n);
        }
    }
}
