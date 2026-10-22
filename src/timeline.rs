//! Frame-level native/MAME diagnostics. Both captures start at power-on and sample
//! shared RAM after each frame. Passing compares the selected observations only;
//! it does not establish cycle, video, audio, or whole-RAM equivalence.

use std::fmt;

pub const RAM_BYTES: usize = 0x2000;
/// Bound memory retained by a diagnostic capture (about 82 MB per full trace).
pub const MAX_FRAMES: u64 = 10_000;
// Version 2 requires physical input overrides to be in place before MAME's
// frame input poll. Version 1 captures labelled requests one frame too early.
pub const TRACE_HEADER: &str = "# digdug-timeline-v2";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metadata {
    pub dip_a: u8,
    pub dip_b: u8,
    pub earom: [u8; 64],
    /// FNV-1a32 over CPU0 $0000..$3FFF, CPU1 $0000..$1FFF,
    /// and CPU2 $0000..$0FFF, in that order.
    pub rom_hash: u32,
    /// The first script mask applies before executing this one-based frame.
    pub script_start: u64,
    pub frames: u64,
}

#[derive(Clone, Debug)]
pub struct FrameSample {
    pub frame: u64,
    pub buttons: u8,
    pub ram: Box<[u8; RAM_BYTES]>,
}

impl FrameSample {
    fn read(&self, address: u16) -> u8 {
        self.ram[usize::from(address - 0x8000)]
    }

    fn in_game(&self) -> bool {
        self.read(0x8400) & 0x40 != 0 && self.read(0x8657) & 0x02 == 0
    }

    fn spawned(&self) -> bool {
        self.in_game()
            && self.read(0x8400) & 0x30 == 0x30
            && (0..8).any(|slot| {
                let base = 0x8525 + slot * 0x10;
                self.read(base + 3) & 0x80 == 0
                    && self.read(base + 4) > 1
                    && self.read(base + 5) > 1
            })
    }
}

#[derive(Clone, Debug)]
pub struct Timeline {
    pub metadata: Metadata,
    pub samples: Vec<FrameSample>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "timeline line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

fn error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError { line, message: message.into() }
}

fn hex_byte(text: &str) -> Option<u8> {
    if text.len() == 2 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        u8::from_str_radix(text, 16).ok()
    } else {
        None
    }
}

fn decode_hex<const N: usize>(text: &str, line: usize, field: &str) -> Result<[u8; N], ParseError> {
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error(line, format!("{field} must contain exactly {} hexadecimal digits", N * 2)));
    }
    let mut result = [0; N];
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = hex_byte(&text[i * 2..i * 2 + 2]).unwrap();
    }
    Ok(result)
}

fn parse_metadata(line: &str, line_number: usize) -> Result<Metadata, ParseError> {
    let mut fields = std::collections::BTreeMap::new();
    let text = line.strip_prefix("# ").ok_or_else(|| error(line_number, "missing metadata line"))?;
    for field in text.split_whitespace() {
        let (key, value) = field.split_once('=').ok_or_else(|| error(line_number, "metadata must use key=value fields"))?;
        if !matches!(key, "dip_a" | "dip_b" | "earom" | "rom_hash" | "script_start" | "frames") {
            return Err(error(line_number, format!("unknown metadata field {key}")));
        }
        if fields.insert(key, value).is_some() {
            return Err(error(line_number, format!("duplicate metadata field {key}")));
        }
    }
    let get = |name: &str| fields.get(name).copied().ok_or_else(|| error(line_number, format!("missing metadata field {name}")));
    let dip_a = hex_byte(get("dip_a")?).ok_or_else(|| error(line_number, "dip_a must be two hexadecimal digits"))?;
    let dip_b = hex_byte(get("dip_b")?).ok_or_else(|| error(line_number, "dip_b must be two hexadecimal digits"))?;
    let earom = decode_hex(get("earom")?, line_number, "earom")?;
    let hash = get("rom_hash")?;
    if hash.len() != 8 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error(line_number, "rom_hash must be eight hexadecimal digits"));
    }
    let rom_hash = u32::from_str_radix(hash, 16).unwrap();
    let decimal = |name: &str| -> Result<u64, ParseError> {
        let text = get(name)?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(error(line_number, format!("{name} must be a positive decimal integer")));
        }
        text.parse().map_err(|_| error(line_number, format!("invalid {name}")))
    };
    let script_start = decimal("script_start")?;
    let frames = decimal("frames")?;
    if !(1..=MAX_FRAMES).contains(&frames) || !(1..=frames).contains(&script_start) {
        return Err(error(line_number, format!("require 1 <= script_start <= frames <= {MAX_FRAMES}")));
    }
    Ok(Metadata { dip_a, dip_b, earom, rom_hash, script_start, frames })
}

impl Timeline {
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        parse(text)
    }

    pub fn is_complete(&self) -> bool {
        self.samples.len() as u64 == self.metadata.frames
    }
}

/// A capture ending between complete rows is retained as incomplete, so the
/// comparator can return an inconclusive result. Broken rows are invalid input.
pub fn parse(text: &str) -> Result<Timeline, ParseError> {
    let mut lines = text.lines();
    if lines.next() != Some(TRACE_HEADER) {
        return Err(error(1, format!("expected {TRACE_HEADER}")));
    }
    let metadata = parse_metadata(lines.next().ok_or_else(|| error(2, "missing metadata line"))?, 2)?;
    let mut samples = Vec::new();
    for (index, line) in lines.enumerate() {
        let line_number = index + 3;
        if line.trim().is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let frame_text = fields.next().ok_or_else(|| error(line_number, "missing frame number"))?;
        let frame: u64 = frame_text.parse().map_err(|_| error(line_number, "invalid frame number"))?;
        if frame != samples.len() as u64 + 1 || frame > metadata.frames {
            return Err(error(line_number, "frames must be contiguous from 1 and not exceed metadata frames"));
        }
        let buttons = fields.next().and_then(hex_byte).ok_or_else(|| error(line_number, "buttons must be two hexadecimal digits"))?;
        if frame < metadata.script_start && buttons != 0 {
            return Err(error(line_number, "nonzero input before script_start"));
        }
        let ram = decode_hex(fields.next().ok_or_else(|| error(line_number, "missing RAM snapshot"))?, line_number, "RAM snapshot")?;
        if fields.next().is_some() {
            return Err(error(line_number, "unexpected field after RAM snapshot"));
        }
        samples.push(FrameSample { frame, buttons, ram: Box::new(ram) });
    }
    Ok(Timeline { metadata, samples })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComparisonOutcome {
    Passed,
    Failed,
    Inconclusive,
}

impl ComparisonOutcome {
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::Failed => 1,
            Self::Inconclusive => 2,
        }
    }
}

impl fmt::Display for ComparisonOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self { Self::Passed => "PASSED", Self::Failed => "FAILED", Self::Inconclusive => "INCONCLUSIVE" })
    }
}

#[derive(Clone, Debug)]
pub struct EventComparison {
    pub name: &'static str,
    pub native_frame: Option<u64>,
    pub reference_frame: Option<u64>,
    /// Native minus MAME, in video frames. Positive means native is later.
    pub delta: Option<i64>,
    pub native_since_start: Option<i64>,
    pub reference_since_start: Option<i64>,
    pub relative_delta: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateDivergence {
    pub frame: u64,
    pub field: &'static str,
    pub address: u16,
    pub mask: u8,
    pub native: u8,
    pub reference: u8,
}

#[derive(Clone, Debug)]
pub struct ComparisonReport {
    pub outcome: ComparisonOutcome,
    pub events: Vec<EventComparison>,
    pub first_divergence: Option<StateDivergence>,
    pub diagnostics: Vec<String>,
}

const EVENT_NAMES: [&str; 7] = [
    "title ready", "coin accepted", "start accepted", "new-game setup", "round init begins", "entities spawned", "first input movement",
];

fn event_frames(timeline: &Timeline) -> [Option<u64>; 7] {
    let mut found = [None; 7];
    // A pseudo-random RAM-test byte can look like the title tile. Require three
    // consecutive title samples after the lives configuration is initialized
    // and the service-mode latch is clear. Keep the first frame of that run.
    found[0] = timeline.samples.windows(3).find(|window| window.iter().all(|sample| {
        sample.read(0x83D9) & 0x7F == 46
            && (1..=5).contains(&sample.read(0x8409))
            && sample.read(0x879A) == 0
    })).map(|window| window[0].frame);
    let mut coin_input_seen = false;
    let mut start_input_seen = false;
    for (index, sample) in timeline.samples.iter().enumerate() {
        coin_input_seen |= sample.buttons & 0x40 != 0;
        start_input_seen |= sample.buttons & 0xA0 != 0;
        if found[0].is_none_or(|frame| sample.frame < frame) || sample.frame < timeline.metadata.script_start {
            continue;
        }
        let credit = sample.read(0x85A5);
        let valid_credit = |value: u8| value <= 0x99 && value & 0x0F <= 9;
        let previous_credit = index.checked_sub(1)
            .map(|previous| timeline.samples[previous].read(0x85A5))
            .filter(|&value| valid_credit(value)).unwrap_or(0);
        if found[1].is_none() && coin_input_seen && valid_credit(credit) && credit > previous_credit {
            found[1] = Some(sample.frame);
        }
        if found[2].is_none() && found[1].is_some() && start_input_seen && sample.in_game() {
            found[2] = Some(sample.frame);
        }
        if found[2].is_some() {
            if found[3].is_none() && sample.in_game() && sample.read(0x8400) & 0x08 != 0 {
                found[3] = Some(sample.frame);
            }
            if found[4].is_none() && sample.in_game() && sample.read(0x8400) & 0x80 != 0 {
                found[4] = Some(sample.frame);
            }
            if found[5].is_none() && sample.spawned() {
                found[5] = Some(sample.frame);
            }
            if found[6].is_none() && found[5].is_some_and(|frame| frame < sample.frame)
                && sample.spawned() && sample.buttons & 0x0F != 0 && index > 0
            {
                let previous = &timeline.samples[index - 1];
                if sample.read(0x98A2) != previous.read(0x98A2) || sample.read(0x98A3) != previous.read(0x98A3) {
                    found[6] = Some(sample.frame);
                }
            }
        }
    }
    found
}

fn first_divergence(native: &Timeline, reference: &Timeline, events: &[EventComparison]) -> Option<StateDivergence> {
    let ready = events[0].native_frame?.max(events[0].reference_frame?);
    let mut coin_seen = false;
    for (n, r) in native.samples.iter().zip(&reference.samples) {
        coin_seen |= n.buttons & 0x40 != 0;
        if n.frame < ready || !coin_seen {
            continue;
        }
        // Ignore RAM self-test patterns, stack residue, attract animation and
        // counters that describe internal implementation rather than these events.
        let mut fields: Vec<(&'static str, u16, u8)> = vec![
            ("latched BCD credits", 0x85A5, 0xFF),
            ("active game / player mode", 0x8400, 0x43),
        ];
        if n.in_game() && r.in_game() {
            fields.extend([
                ("round initialization flags", 0x8400, 0xB8),
                ("player one reserve lives", 0x840A, 0xFF),
                ("player two reserve lives", 0x840B, 0xFF),
                ("player one round", 0x840D, 0xFF),
                ("player two round", 0x840E, 0xFF),
            ]);
        }
        if n.spawned() && r.spawned() {
            fields.extend([
                ("player X", 0x98A2, 0xFF),
                ("player Y", 0x98A3, 0xFF),
                ("death / harpoon / digging flags", 0x8401, 0x98),
            ]);
        }
        for (field, address, mask) in fields {
            let nv = n.read(address) & mask;
            let rv = r.read(address) & mask;
            if nv != rv {
                return Some(StateDivergence { frame: n.frame, field, address, mask, native: nv, reference: rv });
            }
        }
    }
    None
}

/// Compare absolute event frames and the selected same-frame state observations.
/// Start-relative deltas explain which drift accumulated before start acceptance.
pub fn compare(native: &Timeline, reference: &Timeline) -> ComparisonReport {
    let mut report = ComparisonReport {
        outcome: ComparisonOutcome::Inconclusive,
        events: Vec::new(),
        first_divergence: None,
        diagnostics: Vec::new(),
    };
    let n = &native.metadata;
    let r = &reference.metadata;
    if n != r {
        if n.dip_a != r.dip_a || n.dip_b != r.dip_b { report.diagnostics.push("DIP settings differ".into()); }
        if n.earom != r.earom { report.diagnostics.push("initial EAROM differs".into()); }
        if n.rom_hash != r.rom_hash { report.diagnostics.push("program ROM hashes differ".into()); }
        if n.script_start != r.script_start { report.diagnostics.push("script start frames differ".into()); }
        if n.frames != r.frames { report.diagnostics.push("capture frame budgets differ".into()); }
        return report;
    }
    for (name, timeline) in [("native", native), ("MAME", reference)] {
        if !timeline.is_complete() {
            report.diagnostics.push(format!("{name} capture is incomplete: {} of {} frames", timeline.samples.len(), timeline.metadata.frames));
        }
    }
    if let Some((n, r)) = native.samples.iter().zip(&reference.samples).find(|(n, r)| n.buttons != r.buttons) {
        report.diagnostics.push(format!("input masks differ at frame {}: native {:02X}, MAME {:02X}", n.frame, n.buttons, r.buttons));
        return report;
    }
    let native_events = event_frames(native);
    let reference_events = event_frames(reference);
    for (index, name) in EVENT_NAMES.iter().enumerate() {
        let native_frame = native_events[index];
        let reference_frame = reference_events[index];
        let native_since_start = native_frame.zip(native_events[2]).map(|(frame, start)| frame as i64 - start as i64);
        let reference_since_start = reference_frame.zip(reference_events[2]).map(|(frame, start)| frame as i64 - start as i64);
        report.events.push(EventComparison {
            name,
            native_frame,
            reference_frame,
            delta: native_frame.zip(reference_frame).map(|(n, r)| n as i64 - r as i64),
            native_since_start,
            reference_since_start,
            relative_delta: native_since_start.zip(reference_since_start).map(|(n, r)| n - r),
        });
        if native_frame.is_none() || reference_frame.is_none() {
            report.diagnostics.push(format!("missing event: {name} (native {}, MAME {})",
                native_frame.map_or_else(|| "absent".into(), |f| f.to_string()),
                reference_frame.map_or_else(|| "absent".into(), |f| f.to_string())));
        }
    }
    report.first_divergence = first_divergence(native, reference, &report.events);
    if report.diagnostics.is_empty() {
        report.outcome = if report.events.iter().any(|event| event.delta != Some(0)) || report.first_divergence.is_some() {
            ComparisonOutcome::Failed
        } else {
            ComparisonOutcome::Passed
        };
    }
    report
}

impl fmt::Display for ComparisonReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Dig Dug frame timeline: {}", self.outcome)?;
        writeln!(f, "Deltas are native minus MAME; positive means native is later.")?;
        if !self.events.is_empty() {
            writeln!(f, "{:<23} {:>8} {:>8} {:>8} {:>12}", "event", "native", "MAME", "delta", "start delta")?;
            for event in &self.events {
                let unsigned = |value: Option<u64>| value.map_or_else(|| "missing".into(), |v| v.to_string());
                let signed = |value: Option<i64>| value.map_or_else(|| "-".into(), |v| format!("{v:+}"));
                writeln!(f, "{:<23} {:>8} {:>8} {:>8} {:>12}", event.name,
                    unsigned(event.native_frame), unsigned(event.reference_frame), signed(event.delta), signed(event.relative_delta))?;
            }
        }
        if let Some(d) = &self.first_divergence {
            writeln!(f, "First selected state divergence: frame {}, {} (${:04X}, mask ${:02X}): native ${:02X}, MAME ${:02X}",
                d.frame, d.field, d.address, d.mask, d.native, d.reference)?;
        } else if !self.events.is_empty() {
            writeln!(f, "No divergence found in the selected same-frame state observations.")?;
        }
        for diagnostic in &self.diagnostics { writeln!(f, "{diagnostic}")?; }
        writeln!(f, "Scope: end-of-frame events and selected gameplay fields; no cycle, whole-RAM, pixel or audio equivalence claim.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    fn fixture(start: u64) -> Timeline {
        let metadata = Metadata { dip_a: 0x66, dip_b: 0xDB, earom: [0; 64], rom_hash: 1234, script_start: 1, frames: 8 };
        let mut ram = [0; RAM_BYTES];
        let mut samples = Vec::new();
        for frame in 1..=8 {
            ram[0x3D9] = 46;
            ram[0x409] = 3;
            let buttons = match frame { 2 => 0x40, 3..=4 => 0x20, 6..=8 => 0x02, _ => 0 };
            if frame == 2 { ram[0x5A5] = 1; }
            if frame == start { ram[0x5A5] = 0; ram[0x400] = 0x40; }
            if frame == start + 1 { ram[0x400] = 0x48; ram[0x40A] = 2; ram[0x40D] = 1; }
            if frame == start + 2 {
                ram[0x400] = 0xF8;
                ram[0x529] = 0x40; ram[0x52A] = 0x50;
                ram[0x18A2] = 0x80; ram[0x18A3] = 0x90;
            }
            if frame >= start + 3 { ram[0x18A2] += 2; }
            samples.push(FrameSample { frame, buttons, ram: Box::new(ram) });
        }
        Timeline { metadata, samples }
    }

    fn encode(timeline: &Timeline) -> String {
        let m = &timeline.metadata;
        let mut text = format!("{TRACE_HEADER}\n# dip_a={:02X} dip_b={:02X} earom={} rom_hash={:08X} script_start={} frames={}\n",
            m.dip_a, m.dip_b, "00".repeat(64), m.rom_hash, m.script_start, m.frames);
        for sample in &timeline.samples {
            write!(text, "{} {:02X} ", sample.frame, sample.buttons).unwrap();
            for byte in sample.ram.iter() { write!(text, "{byte:02X}").unwrap(); }
            text.push('\n');
        }
        text
    }

    #[test]
    fn complete_matching_replay_passes_and_roundtrips() {
        let native = fixture(3);
        let reference = parse(&encode(&native)).unwrap();
        assert!(reference.is_complete());
        assert_eq!(reference.samples[7].ram, native.samples[7].ram);
        let report = compare(&native, &reference);
        assert_eq!(report.outcome, ComparisonOutcome::Passed);
        assert_eq!(report.events.iter().map(|event| event.native_frame).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3), Some(4), Some(5), Some(5), Some(6)]);
    }

    #[test]
    fn reports_absolute_drift_separately_from_time_since_start() {
        let report = compare(&fixture(4), &fixture(3));
        assert_eq!(report.outcome, ComparisonOutcome::Failed);
        assert_eq!(report.events[4].delta, Some(1));
        assert_eq!(report.events[4].relative_delta, Some(0));
        assert_eq!(report.first_divergence.as_ref().unwrap().frame, 3);
    }

    #[test]
    fn state_difference_fails_even_when_events_match() {
        let reference = fixture(3);
        let mut native = reference.clone();
        native.samples[5].ram[0x18A2] += 2;
        let report = compare(&native, &reference);
        assert_eq!(report.outcome, ComparisonOutcome::Failed);
        assert!(report.events.iter().all(|event| event.delta == Some(0)));
        assert_eq!(report.first_divergence.as_ref().unwrap().address, 0x98A2);
    }

    #[test]
    fn ignores_ram_test_residue_and_unselected_internal_counters() {
        let reference = fixture(3);
        let mut native = reference.clone();
        native.samples[0].ram[0x5A5] = 0xAA;
        native.samples[0].ram[0x400] = 0xFF;
        for sample in &mut native.samples { sample.ram[0x423] = 0xCC; }
        assert_eq!(compare(&native, &reference).outcome, ComparisonOutcome::Passed);
    }

    #[test]
    fn isolated_title_pattern_during_ram_test_is_not_a_ready_event() {
        let mut timeline = fixture(3);
        timeline.samples[0].ram[0x409] = 0;
        timeline.samples[1].ram[0x3D9] = 0;
        assert_eq!(event_frames(&timeline)[0], Some(3));
        timeline.samples[3].ram[0x79A] = 1;
        assert_eq!(event_frames(&timeline)[0], Some(5));
    }

    #[test]
    fn credit_received_before_title_readiness_is_not_retimed_to_title() {
        let mut timeline = fixture(3);
        timeline.samples[0].ram[0x409] = 0;
        timeline.samples[1].ram[0x409] = 0;
        assert_eq!(event_frames(&timeline)[0], Some(3));
        assert_eq!(event_frames(&timeline)[1], None);
        assert_eq!(compare(&timeline, &timeline).outcome, ComparisonOutcome::Inconclusive);
    }

    #[test]
    fn incompatible_conditions_and_input_masks_are_inconclusive() {
        let reference = fixture(3);
        let mut native = reference.clone();
        native.metadata.rom_hash += 1;
        assert_eq!(compare(&native, &reference).outcome, ComparisonOutcome::Inconclusive);
        native.metadata = reference.metadata.clone();
        native.samples[7].buttons = 0;
        let report = compare(&native, &reference);
        assert_eq!(report.outcome, ComparisonOutcome::Inconclusive);
        assert!(report.diagnostics[0].contains("input masks differ at frame 8"));
    }

    #[test]
    fn absent_events_and_cleanly_truncated_capture_cannot_pass() {
        let reference = fixture(3);
        let mut native = reference.clone();
        native.samples.truncate(4);
        let parsed = parse(&encode(&native)).unwrap();
        assert!(!parsed.is_complete());
        let report = compare(&parsed, &reference);
        assert_eq!(report.outcome, ComparisonOutcome::Inconclusive);
        assert!(report.diagnostics.iter().any(|d| d.contains("incomplete")));
        native = reference.clone();
        for sample in &mut native.samples { sample.ram[0x529] = 0; }
        assert_eq!(compare(&native, &native).outcome, ComparisonOutcome::Inconclusive);
    }

    #[test]
    fn malformed_or_ambiguous_trace_is_rejected() {
        let encoded = encode(&fixture(3));
        for text in [
            encoded.replacen("dip_a=66", "dip_a=66 dip_a=66", 1),
            encoded.replacen("rom_hash=000004D2", "", 1),
            encoded.replacen("frames=8", "frames=10001", 1),
            encoded.replacen("1 00 ", "2 00 ", 1),
            encoded.replacen("1 00 ", "1 GG ", 1),
            encoded.replacen("1 00 ", "1 00 Z", 1),
            encoded.replacen("script_start=1", "script_start=4", 1),
            format!("{TRACE_HEADER}\n{TRACE_HEADER}\n"),
        ] {
            assert!(parse(&text).is_err());
        }
    }

    #[test]
    fn outcomes_have_distinct_diagnostic_exit_codes() {
        assert_eq!(ComparisonOutcome::Passed.exit_code(), 0);
        assert_eq!(ComparisonOutcome::Failed.exit_code(), 1);
        assert_eq!(ComparisonOutcome::Inconclusive.exit_code(), 2);
    }
}
