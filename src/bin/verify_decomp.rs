//! Differential diagnostics for the native Rust translation using user-supplied ROMs.
use std::path::Path;
use std::process::ExitCode;

use digdug_re::hardware::{DigDugMachine, DIGDUG_DIP_A, DIGDUG_DIP_B};
use digdug_re::rom::DigDugRoms;
use digdug_re::verify::{Mismatch, VerificationOutcome, VerifyLog};

const USAGE: &str = "Usage: verify_decomp [--frames N] [--script FILE] [--script-start F]
    [--dip-a HEX] [--dip-b HEX] [--max-instr N] [--top N] [--strict-registers] [--report-only]

Exit codes: 0 passed, 1 failed, 2 inconclusive, 64 invalid arguments or run error.
--strict-registers  Reject every endpoint register mismatch, even sampled convergence.
--report-only       Print the actual outcome but exit 0 after a completed diagnostic run.
--max-instr N       Reference steps per boundary (default 10000000, including startup tests).
--script-start F    Apply the first script input before frame F (1-based; default 200).
--dip-a/--dip-b     Physical switch bytes in hex (default 99/24).
--help              Show this help without loading ROMs.";

#[derive(Debug)]
struct Options {
    frames: u64,
    script: Option<String>,
    script_start: u64,
    dips: [u8; 2],
    max_instr: u64,
    top: usize,
    strict_registers: bool,
    report_only: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            frames: 1200,
            script: None,
            script_start: 200,
            dips: [!DIGDUG_DIP_A, !DIGDUG_DIP_B],
            max_instr: 10_000_000,
            top: 40,
            strict_registers: false,
            report_only: false,
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Options>, String> {
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--strict-registers" => options.strict_registers = true,
            "--report-only" => options.report_only = true,
            "--frames" | "--script" | "--script-start" | "--dip-a" | "--dip-b" | "--max-instr" | "--top" => {
                let value = args.next().filter(|v| !v.starts_with("--"))
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                let invalid = || format!("invalid value for {arg}: {value}");
                match arg.as_str() {
                    "--frames" => options.frames = value.parse().map_err(|_| invalid())?,
                    "--script" => options.script = Some(value),
                    "--script-start" => options.script_start = value.parse().map_err(|_| invalid())?,
                    "--dip-a" => options.dips[0] = u8::from_str_radix(&value, 16).map_err(|_| invalid())?,
                    "--dip-b" => options.dips[1] = u8::from_str_radix(&value, 16).map_err(|_| invalid())?,
                    "--max-instr" => options.max_instr = value.parse().map_err(|_| invalid())?,
                    "--top" => options.top = value.parse().map_err(|_| invalid())?,
                    _ => unreachable!(),
                }
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    if options.frames == 0 || options.max_instr == 0 || options.script_start == 0 {
        return Err("--frames, --max-instr and --script-start must be greater than zero".into());
    }
    Ok(Some(options))
}

fn parse_script(text: &str) -> Result<Vec<(u8, u32)>, String> {
    let mut script = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        let invalid = || format!("script line {} must be '<buttons hex> <positive frame count>'", index + 1);
        if fields.len() != 2 {
            return Err(invalid());
        }
        let buttons = u8::from_str_radix(fields[0], 16).map_err(|_| invalid())?;
        let frames: u32 = fields[1].parse().map_err(|_| invalid())?;
        if frames == 0 {
            return Err(invalid());
        }
        script.push((buttons, frames));
    }
    Ok(script)
}

fn script_buttons(script: &[(u8, u32)], mut elapsed: u64) -> u8 {
    for &(buttons, frames) in script {
        if elapsed < u64::from(frames) {
            return buttons;
        }
        elapsed -= u64::from(frames);
    }
    0
}

fn buttons_at(script: &[(u8, u32)], frame: u64, start: u64) -> u8 {
    frame.checked_sub(start).map_or(0, |elapsed| script_buttons(script, elapsed))
}

fn severity(m: &Mismatch) -> usize {
    if m.state_count > 0 {
        0
    } else if m.reg_propagates > 0 {
        1
    } else if m.reg_inconclusive > 0 || m.reg_unsampled > 0 {
        2
    } else {
        3
    }
}

fn report(log: &VerifyLog, options: &Options) {
    println!("=== Differential verification report ===");
    println!("frames run:        {}", options.frames);
    println!("physical DIP:      {:02X}/{:02X}; script starts at frame {} (1-based)",
        options.dips[0], options.dips[1], options.script_start);
    println!("steps compared:    {}", log.checked);
    println!("steps skipped (already halted): {}", log.skipped_halted);
    println!("reference timeouts: {} distinct units ({} total)",
        log.timeouts.len(), log.timeouts.values().sum::<u64>());
    println!("reference halted before boundary: {} distinct units ({} total)",
        log.reference_halts.len(), log.reference_halts.values().sum::<u64>());
    let mut unsupported: Vec<_> = log.unsupported.iter().collect();
    unsupported.sort_by_key(|(error, _)| *error);
    println!("unsupported reference instructions: {:?}", unsupported);
    let mut rows: Vec<_> = log.mismatches.iter().collect();
    rows.sort_by(|a, b| severity(a.1).cmp(&severity(b.1))
        .then(b.1.count.cmp(&a.1.count)).then(a.0.cmp(b.0)));
    let classes = ["STATE", "REG-PROPAGATES", "REG-INCONCLUSIVE", "REG-CONVERGED"];
    println!("mismatching units: {}", rows.len());
    for (index, class) in classes.iter().enumerate() {
        println!("  {class}: {}", rows.iter().filter(|(_, m)| severity(m) == index).count());
    }
    for ((cpu, pc), m) in rows.iter().take(options.top) {
        let class = classes[severity(m)];
        println!("\n--- [{class}] cpu{cpu} ${pc:04X} last-helper={} total={} state={} reg-propagates={} reg-converged={} reg-inconclusive={} reg-unsampled={} first@frame {}",
            m.hooked_name, m.count, m.state_count, m.reg_propagates, m.reg_converged,
            m.reg_inconclusive, m.reg_unsampled, m.first_frame);
        let example = match severity(m) {
            0 => &m.state_example,
            1 => &m.propagate_example,
            2 if !m.inconclusive_example.is_empty() => &m.inconclusive_example,
            _ => &m.reg_example,
        };
        for difference in example.iter().take(30) {
            println!("      {difference}");
        }
    }
    for (label, failures) in [("timeout", &log.timeouts), ("reference halt", &log.reference_halts)] {
        let mut rows: Vec<_> = failures.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for ((cpu, pc), count) in rows.iter().take(options.top) {
            println!("  {label}: cpu{cpu} ${pc:04X}: {count}");
        }
    }
    println!("\nOutcome: {:?} (strict registers: {})", log.outcome(options.strict_registers), options.strict_registers);
    println!("Scope: compared endpoints and sampled single-CPU continuations; timing and full-machine equivalence remain unverified.");
    if options.report_only {
        println!("Report-only mode: a completed diagnostic run exits 0 regardless of the outcome above.");
    }
}

fn run(options: &Options) -> Result<VerificationOutcome, String> {
    let script = match &options.script {
        Some(path) => parse_script(&std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read script {path}: {error}"))?)?,
        None => Vec::new(),
    };
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))?;
    let mut machine = DigDugMachine::new_unbooted(roms);
    machine.bus.dip_switches = options.dips;
    machine.verify = Some(Box::new(VerifyLog::new(options.max_instr)));
    let t0 = std::time::Instant::now();
    for frame in 1..=options.frames {
        machine.bus.buttons = buttons_at(&script, frame, options.script_start);
        machine.run_frame();
        if frame % 200 == 0 {
            eprintln!("  ... frame {}/{} ({:.1}s)", frame, options.frames, t0.elapsed().as_secs_f32());
        }
    }
    let log = machine.verify.take().unwrap();
    report(&log, options);
    Ok(log.outcome(options.strict_registers))
}

fn main() -> ExitCode {
    let options = match parse_args(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => { println!("{USAGE}"); return ExitCode::SUCCESS; }
        Err(error) => { eprintln!("{error}\n\n{USAGE}"); return ExitCode::from(64); }
    };
    match run(&options) {
        Ok(outcome) => ExitCode::from(if options.report_only { 0 } else { outcome.exit_code() }),
        Err(error) => { eprintln!("Verification could not complete: {error}"); ExitCode::from(64) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Result<Option<Options>, String> {
        parse_args(values.iter().map(|v| (*v).to_string()))
    }

    #[test]
    fn rejects_empty_runs_and_misspelled_or_missing_options() {
        for values in [vec!["--frames", "0"], vec!["--max-instr", "0"],
            vec!["--frames", "no"], vec!["--strict-regsters"], vec!["--script"],
            vec!["--script", "--report-only"], vec!["--script-start", "0"],
            vec!["--dip-a", "100"], vec!["--dip-b", "not-hex"]] {
            assert!(args(&values).is_err(), "accepted {values:?}");
        }
        assert!(args(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn strict_checks_and_report_only_are_explicit_options() {
        let defaults = args(&[]).unwrap().unwrap();
        assert!(!defaults.report_only && !defaults.strict_registers);
        let options = args(&["--strict-registers", "--report-only"]).unwrap().unwrap();
        assert!(options.strict_registers && options.report_only);
        let options = args(&["--dip-a", "A1", "--dip-b", "24"]).unwrap().unwrap();
        assert_eq!(options.dips, [0xA1, 0x24]);
    }

    #[test]
    fn script_validation_and_playback_preserve_frame_boundaries() {
        let script = parse_script("# synthetic input\n40 2 # coin\n00 1\n80 1\n").unwrap();
        let actual: Vec<_> = (0..6).map(|f| script_buttons(&script, f)).collect();
        assert_eq!(actual, [0x40, 0x40, 0, 0x80, 0, 0]);
        let actual: Vec<_> = (1..=8).map(|f| buttons_at(&script, f, 3)).collect();
        assert_eq!(actual, [0, 0, 0x40, 0x40, 0, 0x80, 0, 0]);
        assert_eq!(buttons_at(&script, 1, 1), 0x40);
        for text in ["40 0", "100 1", "40", "40 1 extra", "40 nope"] {
            assert!(parse_script(text).is_err(), "accepted {text}");
        }
    }
}
