//! Capture native frame endpoints and compare them with an independent MAME run.
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::ExitCode;

use digdug_re::hardware::{DigDugMachine, DIGDUG_DIP_A, DIGDUG_DIP_B};
use digdug_re::rom::DigDugRoms;
use digdug_re::timeline;

const USAGE: &str = "Usage:
  timeline capture --script FILE --output FILE [--script-start N] [--frames N]
  timeline compare --native FILE --reference FILE

Captures start at power-on with zero EAROM and the native DIP settings.
Frame 1 is the first completed frame; --script-start defaults to 700 and --frames to 1600.
Script lines are '<buttons hex> <positive frame count>'; # introduces a comment.
Exit codes: 0 passed/captured, 1 diverged, 2 inconclusive, 64 invalid input or run error.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Capture { script: String, output: String, script_start: u64, frames: u64 },
    Compare { native: String, reference: String },
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let mode = match args.next().as_deref() {
        Some("--help" | "-h") => return Ok(Command::Help),
        Some("capture") => "capture",
        Some("compare") => "compare",
        _ => return Err("expected capture or compare (see --help)".into()),
    };
    let mut values = std::collections::HashMap::new();
    while let Some(key) = args.next() {
        let valid = match mode {
            "capture" => matches!(key.as_str(), "--script" | "--output" | "--script-start" | "--frames"),
            _ => matches!(key.as_str(), "--native" | "--reference"),
        };
        if !valid { return Err(format!("unknown option: {key}")); }
        let value = args.next().filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{key} requires a value"))?;
        if values.insert(key.clone(), value).is_some() {
            return Err(format!("duplicate option: {key}"));
        }
    }
    let required = |key: &str| values.get(key).cloned().ok_or_else(|| format!("missing {key}"));
    if mode == "compare" {
        return Ok(Command::Compare { native: required("--native")?, reference: required("--reference")? });
    }
    let number = |key: &str, default: u64| -> Result<u64, String> {
        values.get(key).map_or(Ok(default), |v| v.parse::<u64>().map_err(|_| format!("invalid {key}: {v}")))
    };
    let script_start = number("--script-start", 700)?;
    let frames = number("--frames", 1600)?;
    if script_start == 0 || frames < script_start || frames > timeline::MAX_FRAMES {
        return Err(format!("require 1 <= --script-start <= --frames <= {}", timeline::MAX_FRAMES));
    }
    Ok(Command::Capture { script: required("--script")?, output: required("--output")?, script_start, frames })
}

fn parse_script(text: &str) -> Result<Vec<(u8, u64)>, String> {
    let mut script = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() { continue; }
        let fields: Vec<_> = line.split_whitespace().collect();
        let invalid = || format!("script line {} must be '<buttons hex> <positive frame count>'", index + 1);
        if fields.len() != 2 { return Err(invalid()); }
        let buttons = u8::from_str_radix(fields[0], 16).map_err(|_| invalid())?;
        let frames: u64 = fields[1].parse().map_err(|_| invalid())?;
        if frames == 0 { return Err(invalid()); }
        script.push((buttons, frames));
    }
    if script.is_empty() { return Err("input script is empty".into()); }
    Ok(script)
}

fn buttons_at(script: &[(u8, u64)], frame: u64, start: u64) -> u8 {
    let Some(mut elapsed) = frame.checked_sub(start) else { return 0; };
    for &(buttons, duration) in script {
        if elapsed < duration { return buttons; }
        elapsed -= duration;
    }
    0
}

fn capture(script_path: &str, output: &str, script_start: u64, frames: u64) -> Result<(), String> {
    let script = parse_script(&std::fs::read_to_string(script_path).map_err(|e| format!("{script_path}: {e}"))?)?;
    if Path::new(script_path).canonicalize().ok() == Path::new(output).canonicalize().ok()
        && Path::new(output).exists() {
        return Err("output must not overwrite the input script".into());
    }
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))?;
    let rom_hash = roms.cpu0[..0x4000].iter().chain(&roms.cpu1[..0x2000]).chain(&roms.cpu2[..0x1000])
        .fold(2_166_136_261u32, |hash, &byte| (hash ^ u32::from(byte)).wrapping_mul(16_777_619));
    let mut machine = DigDugMachine::new_unbooted(roms);
    let mut out = BufWriter::new(File::create(output).map_err(|e| format!("{output}: {e}"))?);
    let mut header = format!("{}\n# dip_a={DIGDUG_DIP_A:02X} dip_b={DIGDUG_DIP_B:02X} earom=", timeline::TRACE_HEADER);
    for byte in &machine.bus.earom { write!(header, "{byte:02X}").unwrap(); }
    writeln!(header, " rom_hash={rom_hash:08X} script_start={script_start} frames={frames}").unwrap();
    out.write_all(header.as_bytes()).map_err(|e| e.to_string())?;
    let mut line = String::with_capacity(16_430);
    for frame in 1..=frames {
        machine.bus.buttons = buttons_at(&script, frame, script_start);
        machine.run_frame();
        line.clear();
        write!(line, "{frame} {:02X} ", machine.bus.buttons).unwrap();
        for byte in &machine.bus.shared_ram { write!(line, "{byte:02X}").unwrap(); }
        line.push('\n');
        out.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())?;
    println!("Captured {frames} native frames from power-on: {output}");
    Ok(())
}

fn run() -> Result<u8, String> {
    match parse_args(std::env::args().skip(1))? {
        Command::Help => { println!("{USAGE}"); Ok(0) }
        Command::Capture { script, output, script_start, frames } => {
            capture(&script, &output, script_start, frames)?;
            Ok(0)
        }
        Command::Compare { native, reference } => {
            let read = |path: &str| -> Result<_, String> {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                timeline::parse(&text).map_err(|e| format!("{path}: {e}"))
            };
            let native = read(&native)?;
            let reference = read(&reference)?;
            let report = timeline::compare(&native, &reference);
            println!("{report}");
            Ok(report.outcome.exit_code())
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => { eprintln!("timeline: {error}"); ExitCode::from(64) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_inputs_apply_before_the_named_frame_without_boundary_overlap() {
        let script = parse_script("# controls\n40 2 # coin\n00 1\n80 1\n").unwrap();
        let observed: Vec<_> = (1..=8).map(|frame| buttons_at(&script, frame, 3)).collect();
        assert_eq!(observed, [0, 0, 0x40, 0x40, 0, 0x80, 0, 0]);
        for invalid in ["", "00 0", "100 1", "01 1 extra", "02 -1", "00"] {
            assert!(parse_script(invalid).is_err(), "accepted {invalid:?}");
        }
    }

    #[test]
    fn malformed_cli_is_rejected_before_loading_roms_or_creating_output() {
        let parse = |args: &[&str]| parse_args(args.iter().map(|v| v.to_string()));
        assert_eq!(parse(&["--help"]).unwrap(), Command::Help);
        for args in [
            vec!["capture", "--script", "in", "--output", "out", "--frames", "0"],
            vec!["capture", "--script", "in", "--output", "out", "--script-start", "0"],
            vec!["capture", "--script", "in", "--output", "out", "--frames", "100001"],
            vec!["compare", "--native", "a", "--native", "b", "--reference", "c"],
            vec!["compare", "--native", "a", "--reference"],
            vec!["compare", "--native", "a", "--reference", "b", "--report-only"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }
}
