//! Stream a long input script through the native cabinet and report real coverage.
use std::{collections::HashMap, fs::File, io::{BufWriter, Write}, path::Path, process::ExitCode};
use digdug_re::{hardware::{DigDugMachine, DIGDUG_DIP_A, DIGDUG_DIP_B}, rom::DigDugRoms};

const MAX_FRAMES: u64 = 1_000_000;
const USAGE: &str = "Usage: replay --script FILE [--frames N] [--script-start N] [--dip-a HEX] [--dip-b HEX] [--output NEW.csv]
Runs from power-on with zero EAROM. DIP arguments are physical switch bytes.
Defaults: entire script, script-start 1, DIP 99/24. Maximum 1,000,000 frames.
Output CSV must be a new file. Exit 0 means execution completed, not reference equivalence.
Invalid input/setup exits 64; a translation panic fails the process.";

#[derive(Debug, PartialEq, Eq)]
struct Options { script: String, frames: Option<u64>, start: u64, dips: [u8; 2], output: Option<String> }

fn parse(args: impl IntoIterator<Item=String>) -> Result<Option<Options>, String> {
    let mut args = args.into_iter();
    let mut values = HashMap::new();
    while let Some(key) = args.next() {
        if key == "--help" || key == "-h" { return Ok(None); }
        if !matches!(key.as_str(), "--script" | "--frames" | "--script-start" | "--dip-a" | "--dip-b" | "--output") {
            return Err(format!("unknown option: {key}"));
        }
        let value = args.next().filter(|v| !v.starts_with("--")).ok_or_else(|| format!("{key} needs a value"))?;
        if values.insert(key.clone(), value).is_some() { return Err(format!("duplicate option: {key}")); }
    }
    let number = |key: &str| -> Result<Option<u64>, String> {
        values.get(key).map(|v| v.parse().map_err(|_| format!("invalid {key}: {v}"))).transpose()
    };
    let start = number("--script-start")?.unwrap_or(1);
    let frames = number("--frames")?;
    if start == 0 || start > MAX_FRAMES || frames.is_some_and(|n| n == 0 || n > MAX_FRAMES || n < start) {
        return Err("frame counts must satisfy 1 <= script-start <= frames <= 1000000".into());
    }
    let mut dips = [!DIGDUG_DIP_A, !DIGDUG_DIP_B];
    for (index, key) in ["--dip-a", "--dip-b"].into_iter().enumerate() {
        if let Some(value) = values.get(key) {
            dips[index] = u8::from_str_radix(value, 16).map_err(|_| format!("invalid {key}: {value}"))?;
        }
    }
    Ok(Some(Options { script: values.remove("--script").ok_or("missing --script")?, frames,
        start, dips, output: values.remove("--output") }))
}

fn script(text: &str) -> Result<Vec<(u8, u64)>, String> {
    let mut steps = Vec::new();
    let mut total = 0u64;
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() { continue; }
        let fields: Vec<_> = line.split_whitespace().collect();
        let invalid = || format!("script line {} must be '<buttons hex> <positive frames>'", index + 1);
        if fields.len() != 2 { return Err(invalid()); }
        let buttons = u8::from_str_radix(fields[0], 16).map_err(|_| invalid())?;
        let frames: u64 = fields[1].parse().map_err(|_| invalid())?;
        total = total.checked_add(frames).filter(|&n| n <= MAX_FRAMES).ok_or("script exceeds frame limit")?;
        if frames == 0 { return Err(invalid()); }
        steps.push((buttons, frames));
    }
    if steps.is_empty() { return Err("empty script".into()); }
    Ok(steps)
}

fn masks(steps: &[(u8,u64)], start: u64) -> impl Iterator<Item=u8> + '_ {
    std::iter::repeat(0).take((start - 1) as usize).chain(steps.iter()
        .flat_map(|&(buttons, frames)| std::iter::repeat(buttons).take(frames as usize)))
}

fn score(ram: &[u8; 8192], offset: usize) -> Option<u32> {
    let mut n = 0;
    for i in (0..3).rev() {
        let b = ram[offset+i];
        if b & 15 > 9 || b >> 4 > 9 { return None; }
        n = n * 100 + u32::from((b >> 4) * 10 + (b & 15));
    }
    Some(n)
}

#[derive(Default)]
struct Coverage {
    game_frames: u64, games: u64, game_overs: u64, deaths: u64,
    first_game: Option<u64>, last_game: Option<u64>, max_round: [u8;2], max_score: [u32;2],
    was_game: bool, was_dying: bool,
}
impl Coverage {
    fn observe(&mut self, frame: u64, game: bool, dying: bool, player: u8, rounds: [u8;2], scores: [Option<u32>;2]) {
        if game {
            self.game_frames += 1;
            self.first_game.get_or_insert(frame);
            self.last_game = Some(frame);
            if !self.was_game { self.games += 1; }
            if dying && !self.was_dying { self.deaths += 1; }
            let i = usize::from(player == 2);
            self.max_round[i] = self.max_round[i].max(rounds[i]);
            if let Some(n) = scores[i] { self.max_score[i] = self.max_score[i].max(n); }
        } else if self.was_game { self.game_overs += 1; }
        self.was_game = game;
        self.was_dying = game && dying;
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse(std::env::args().skip(1))? else { println!("{USAGE}"); return Ok(()); };
    let steps = script(&std::fs::read_to_string(&options.script).map_err(|e| format!("{}: {e}", options.script))?)?;
    let total = steps.iter().map(|&(_, n)| n).sum::<u64>().checked_add(options.start - 1)
        .filter(|&n| n <= MAX_FRAMES).ok_or("script and neutral lead-in exceed frame limit")?;
    let frames = options.frames.unwrap_or(total);
    if frames > total { return Err("requested frames exceed available script; silence after EOF is not replay coverage".into()); }
    let roms = DigDugRoms::load_from_extracted_dir(Path::new("roms/extracted"))?;
    let mut machine = DigDugMachine::new_unbooted(roms);
    machine.bus.dip_switches = options.dips;
    let mut csv = options.output.as_ref().map(|path| File::create_new(path)
        .map(BufWriter::new).map_err(|e| format!("{path}: {e}"))).transpose()?;
    if let Some(out) = &mut csv {
        writeln!(out, "# native replay; physical DIP={:02X}/{:02X}; zero EAROM; script_start={}; frames={frames}", options.dips[0], options.dips[1], options.start).map_err(|e| e.to_string())?;
        writeln!(out, "frame,buttons,in_game,round1,round2,lives1,lives2,score1,score2,x,y,flags8400,flags8401,tether,pump,ready896c,ready85d5,sound9a81,input85b0,face85ae,speed85b1,fraction85b3,enemies").map_err(|e| e.to_string())?;
    }
    println!("Replaying {frames} input frames; physical DIP={:02X}/{:02X}; script starts at {}.", options.dips[0], options.dips[1], options.start);
    let mut coverage = Coverage::default();
    for (index, buttons) in masks(&steps, options.start).take(frames as usize).enumerate() {
        let frame = index as u64 + 1;
        machine.bus.buttons = buttons;
        machine.run_frame();
        let s = machine.snapshot();
        let r = &machine.bus.shared_ram;
        let game = machine.bus.game_started && r[0x79A] == 0 && s.in_game && !s.attract_demo;
        let scores = [score(r,0x414), score(r,0x417)];
        coverage.observe(frame, game, r[0x401]&0x80 != 0, s.active_player, [s.p1_round,s.p2_round], scores);
        if let Some(out) = &mut csv {
            writeln!(out, "{frame},{buttons:02X},{},{},{},{},{},{},{},{},{},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{:02X},{}",
                u8::from(game),s.p1_round,s.p2_round,s.p1_lives,s.p2_lives,
                scores[0].map_or(String::new(),|n|n.to_string()), scores[1].map_or(String::new(),|n|n.to_string()),
                s.digdug_x,s.digdug_y,r[0x400],r[0x401],r[0x403],r[0x404],r[0x96C],r[0x5D5],r[0x1A81],r[0x5B0],r[0x5AE],r[0x5B1],r[0x5B3],s.active_enemies).map_err(|e|e.to_string())?;
            if frame % 600 == 0 { out.flush().map_err(|e|e.to_string())?; }
        }
        if frame % 30_000 == 0 { eprintln!("  replay frame {frame}/{frames}"); }
    }
    if let Some(out) = &mut csv { out.flush().map_err(|e|e.to_string())?; }
    println!("Execution completed: {frames} frames ({:.2} simulated minutes).", frames as f64 * 0.0165 / 60.0);
    println!("In-game frames: {}; games started: {}; game overs: {}; death animations: {}.", coverage.game_frames, coverage.games, coverage.game_overs, coverage.deaths);
    println!("First/last in-game frame: {:?}/{:?}.", coverage.first_game, coverage.last_game);
    println!("Highest round entered: P1 {}, P2 {}. Highest six-digit raw score: P1 {}, P2 {}.", coverage.max_round[0],coverage.max_round[1],coverage.max_score[0],coverage.max_score[1]);
    println!("Scope: input execution and observed coverage. Later attract-mode frames do not establish later-round play; no reference-equivalence claim.");
    Ok(())
}

fn main() -> ExitCode {
    match run() { Ok(())=>ExitCode::SUCCESS, Err(e)=>{eprintln!("replay: {e}"); ExitCode::from(64)} }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripts_stream_in_the_named_frames_and_reject_bad_durations() {
        let s = script("# replay\n40 2 # coin\n00 1\n20 2\n").unwrap();
        assert_eq!(masks(&s,3).collect::<Vec<_>>(), [0,0,0x40,0x40,0,0x20,0x20]);
        for text in ["", "00 0", "01 -1", "100 2", "00 1 extra", "00 1000001", "00 1000000\n00 1"] { assert!(script(text).is_err(), "{text}"); }
    }
    #[test]
    fn invalid_cli_is_rejected_before_any_emulation() {
        let p=|a:&[&str]| parse(a.iter().map(|s|s.to_string()));
        assert!(p(&["--help"]).unwrap().is_none());
        assert_eq!(p(&["--script","x","--dip-a","A1","--dip-b","24"]).unwrap().unwrap().dips,[0xA1,0x24]);
        for a in [vec![],vec!["--script"],vec!["--script","x","--script","y"],vec!["--script","x","--frames","0"],vec!["--script","x","--script-start","0"],vec!["--script","x","--frames","1","--script-start","2"],vec!["--script","x","--dip-a","100"]] { assert!(p(&a).is_err(), "{a:?}"); }
    }
    #[test]
    fn attract_mode_and_boot_do_not_inflate_gameplay_coverage() {
        let mut c=Coverage::default();
        c.observe(1,false,true,1,[255,255],[Some(999999);2]);
        c.observe(10,true,false,1,[1,99],[Some(100),Some(999999)]);
        c.observe(11,true,true,1,[2,99],[Some(200),Some(999999)]);
        c.observe(12,true,true,1,[2,99],[Some(200),Some(999999)]);
        c.observe(13,false,false,1,[9,9],[Some(999999);2]);
        assert_eq!((c.game_frames,c.games,c.game_overs,c.deaths),(3,1,1,1));
        assert_eq!((c.first_game,c.last_game),(Some(10),Some(12)));
        assert_eq!((c.max_round,c.max_score),([2,0],[200,0]));
        c.observe(20,true,false,2,[9,3],[Some(999999),Some(300)]);
        assert_eq!((c.max_round,c.max_score),([2,3],[200,300]));
    }
}
