//! Differential verification of the hand-translated Rust game code against the original ROM.
//!
//! For every CPU step (a "unit": one decompiled routine, loop, or instruction block) the
//! machine snapshots CPU + bus, runs the Rust implementation, then runs the independent
//! reference interpreter (`z80_ref.rs`) on the snapshot until it reaches the same `pc`/`sp`
//! the Rust code stopped at. Differences are classified:
//!
//! * **STATE** — non-stack RAM or compared bus state differs at unit exit.
//! * **REG-PROPAGATES** — only registers differ, but running the original ROM onward from both
//!   end states diverges within the reference model.
//! * **REG-CONVERGED** — sampled onward execution reaches equal compared registers and state.
//! * **REG-INCONCLUSIVE** — a limit, HALT, or unsupported instruction prevents convergence.
//!
//! Bytes just below the executing CPU's stack pointer are ignored: the ROM leaves pushed
//! return addresses there while the Rust hooks don't push. This is a heuristic, not a proof
//! of stack liveness. Timing, interrupts, other CPUs, and ordered I/O effects are not verified.

use std::collections::HashMap;

use crate::hardware::DigDugBus;
use crate::z80::Z80;
use crate::z80_ref::{ref_run_until, ref_step, RefRun};

/// Flag bits that are architecturally defined (excludes the undocumented X/Y copies).
pub const FLAG_MASK: u8 = 0xD7;
/// How far below SP residue is considered dead stack.
const DEAD_STACK_WINDOW: u16 = 0x60;
/// Reference steps allowed for a sampled continuation (not cycle or interrupt budgets).
const CONTINUATION_INSTR: u32 = 20_000;
/// Register-only mismatches sampled with the liveness test per unit.
const SAMPLES_PER_UNIT: u64 = 12;

#[derive(Default, Clone)]
pub struct Mismatch {
    pub hooked_name: &'static str,
    pub first_frame: u64,
    pub first_state_frame: u64,
    pub count: u64,
    pub state_count: u64,
    pub reg_propagates: u64,
    pub reg_converged: u64,
    pub reg_inconclusive: u64,
    pub reg_unsampled: u64,
    pub state_example: Vec<String>,
    pub propagate_example: Vec<String>,
    pub reg_example: Vec<String>,
    pub inconclusive_example: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerificationOutcome {
    Passed,
    Failed,
    Inconclusive,
}

impl VerificationOutcome {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Passed => 0,
            Self::Failed => 1,
            Self::Inconclusive => 2,
        }
    }
}

#[derive(Default)]
pub struct VerifyLog {
    pub max_instr: u64,
    pub checked: u64,
    pub skipped_halted: u64,
    pub timeouts: HashMap<(usize, u16), u64>,
    pub reference_halts: HashMap<(usize, u16), u64>,
    pub unsupported: HashMap<String, u64>,
    pub mismatches: HashMap<(usize, u16), Mismatch>,
}

struct Diff {
    regs: Vec<String>,
    state: Vec<String>,
    /// Addresses (first few) of differing, non-dead-stack RAM bytes.
    state_addrs: Vec<u16>,
}

impl VerifyLog {
    pub fn new(max_instr: u64) -> Self {
        Self {
            max_instr,
            ..Default::default()
        }
    }

    /// Strict register checking rejects every endpoint register mismatch, including sampled
    /// cases that later converge. All modes reject unresolved or unexamined observations.
    pub fn outcome(&self, strict_registers: bool) -> VerificationOutcome {
        if self.mismatches.values().any(|m| {
            m.state_count > 0 || m.reg_propagates > 0 || (strict_registers && m.count > 0)
        }) {
            return VerificationOutcome::Failed;
        }
        if self.checked == 0
            || !self.timeouts.is_empty()
            || !self.reference_halts.is_empty()
            || !self.unsupported.is_empty()
            || self.mismatches.values().any(|m| m.reg_inconclusive > 0 || m.reg_unsampled > 0)
        {
            return VerificationOutcome::Inconclusive;
        }
        VerificationOutcome::Passed
    }

    /// Verifies one step. `pre_*` are the snapshots taken before the Rust step; `post_*` are the
    /// states the Rust implementation produced.
    pub fn check(
        &mut self,
        frame: u64,
        hooked_name: &'static str,
        pre_cpu: &Z80,
        pre_bus: &DigDugBus,
        post_cpu: &Z80,
        post_bus: &DigDugBus,
    ) {
        if pre_cpu.halted {
            self.skipped_halted += 1;
            return;
        }
        let key = (pre_cpu.cpu_id, pre_cpu.pc);
        let mut rcpu = pre_cpu.clone();
        let mut rbus = pre_bus.clone();
        match ref_run_until(&mut rcpu, &mut rbus, post_cpu.pc, post_cpu.sp, self.max_instr) {
            RefRun::Reached(_) => {}
            RefRun::Timeout => {
                *self.timeouts.entry(key).or_insert(0) += 1;
                return;
            }
            RefRun::Halted => {
                *self.reference_halts.entry(key).or_insert(0) += 1;
                return;
            }
            RefRun::Unsupported(e) => {
                *self.unsupported.entry(e).or_insert(0) += 1;
                return;
            }
        }
        self.checked += 1;
        let diff = compare(post_cpu, post_bus, &rcpu, &rbus);
        if diff.regs.is_empty() && diff.state.is_empty() {
            return;
        }
        let m = self.mismatches.entry(key).or_insert_with(|| Mismatch {
            first_frame: frame,
            hooked_name,
            ..Default::default()
        });
        m.count += 1;
        if !diff.state.is_empty() {
            if m.first_state_frame == 0 {
                m.first_state_frame = frame;
            }
            m.state_count += 1;
            if m.state_example.is_empty() {
                m.state_example = diff.state.iter().chain(diff.regs.iter()).cloned().collect();
                m.state_example.extend(trace_ref_writes(
                    pre_cpu,
                    pre_bus,
                    post_cpu.pc,
                    post_cpu.sp,
                    &diff.state_addrs,
                    self.max_instr,
                ));
            }
            return;
        }
        if m.reg_example.is_empty() {
            m.reg_example = diff.regs.clone();
        }
        if m.reg_propagates + m.reg_converged + m.reg_inconclusive >= SAMPLES_PER_UNIT {
            m.reg_unsampled += 1;
            return;
        }
        match continuation(post_cpu, post_bus, &rcpu, &rbus) {
            Continuation::Converged => m.reg_converged += 1,
            Continuation::Inconclusive(reason) => {
                m.reg_inconclusive += 1;
                if m.inconclusive_example.is_empty() {
                    m.inconclusive_example = diff.regs.iter().cloned()
                        .chain(std::iter::once(reason)).collect();
                }
            }
            Continuation::Diverged(d) => {
                m.reg_propagates += 1;
                if m.propagate_example.is_empty() {
                    m.propagate_example = diff.regs.iter().cloned().chain(d).collect();
                }
            }
        }
    }
}

const REGISTER_NAMES: [&str; 25] = [
    "A", "F", "B", "C", "D", "E", "H", "L", "A'", "F'", "B'", "C'", "D'", "E'", "H'", "L'",
    "IX", "IY", "SP", "PC", "I", "IFF1", "IFF2", "IM", "HALT",
];

fn register_values(c: &Z80) -> [u16; 25] {
    [c.a as u16, (c.f & FLAG_MASK) as u16, c.b as u16, c.c as u16,
        c.d as u16, c.e as u16, c.h as u16, c.l as u16,
        c.a_alt as u16, (c.f_alt & FLAG_MASK) as u16, c.b_alt as u16, c.c_alt as u16,
        c.d_alt as u16, c.e_alt as u16, c.h_alt as u16, c.l_alt as u16,
        c.ix, c.iy, c.sp, c.pc, c.i as u16, c.iff1 as u16, c.iff2 as u16,
        c.im as u16, c.halted as u16]
}

fn in_dead_stack(addr: u16, sp: u16) -> bool {
    addr < sp && sp.wrapping_sub(addr) <= DEAD_STACK_WINDOW
}

fn bus_state_matches(a: &DigDugBus, b: &DigDugBus, sp: u16) -> bool {
    let ram_matches = a.shared_ram == b.shared_ram || a.shared_ram.iter()
        .zip(&b.shared_ram).enumerate().all(|(i, (a, b))| {
            a == b || in_dead_stack(0x8000 + i as u16, sp)
        });
    ram_matches
        && a.sound_regs == b.sound_regs && a.sound_regs_tick0 == b.sound_regs_tick0
        && a.earom == b.earom && a.earom_addr == b.earom_addr && a.earom_data == b.earom_data
        && a.irq_enable == b.irq_enable && a.sub_cpu_reset == b.sub_cpu_reset
        && a.video_latch == b.video_latch && a.game_started == b.game_started
        && a.namco_command == b.namco_command && a.namco_mode == b.namco_mode
        && a.namco_nmi_counter == b.namco_nmi_counter && a.namco_credit == b.namco_credit
        && a.buttons == b.buttons && a.keymask_d == b.keymask_d
        && a.coin_start_inputs == b.coin_start_inputs
}

fn compare(rust: &Z80, rbus: &DigDugBus, refc: &Z80, refbus: &DigDugBus) -> Diff {
    let mut regs: Vec<String> = Vec::new();
    let mut state: Vec<String> = Vec::new();
    for ((name, a), b) in REGISTER_NAMES.iter().zip(register_values(rust)).zip(register_values(refc)) {
        if a != b {
            regs.push(format!("{name}: rust={a:04X} ref={b:04X}"));
        }
    }

    let sp = rust.sp;
    let mut ram_diffs = 0usize;
    let mut state_addrs: Vec<u16> = Vec::new();
    // Idle steps usually leave identical RAM. An array comparison uses the platform's
    // fast equality operation; enumerate individual bytes only when there is a difference.
    if rbus.shared_ram != refbus.shared_ram {
        for i in 0..rbus.shared_ram.len() {
            if rbus.shared_ram[i] != refbus.shared_ram[i] {
                let addr = 0x8000u16 + i as u16;
                if in_dead_stack(addr, sp) {
                    continue;
                }
                if state_addrs.len() < 8 {
                    state_addrs.push(addr);
                }
                if ram_diffs < 10 {
                    state.push(format!(
                        "RAM ${:04X}: rust={:02X} ref={:02X}",
                        addr, rbus.shared_ram[i], refbus.shared_ram[i]
                    ));
                }
                ram_diffs += 1;
            }
        }
    }
    if ram_diffs > 10 {
        state.push(format!("... {} RAM bytes differ in total", ram_diffs));
    }
    macro_rules! busf {
        ($name:expr, $f:ident) => {
            if rbus.$f != refbus.$f {
                state.push(format!("bus.{}: rust={:?} ref={:?}", $name, rbus.$f, refbus.$f));
            }
        };
    }
    busf!("sound_regs", sound_regs);
    busf!("sound_regs_tick0", sound_regs_tick0);
    busf!("earom", earom);
    busf!("earom_addr", earom_addr);
    busf!("earom_data", earom_data);
    busf!("irq_enable", irq_enable);
    busf!("sub_cpu_reset", sub_cpu_reset);
    busf!("video_latch", video_latch);
    busf!("game_started", game_started);
    busf!("namco_command", namco_command);
    busf!("namco_mode", namco_mode);
    busf!("namco_nmi_counter", namco_nmi_counter);
    busf!("namco_credit", namco_credit);
    busf!("coin_start_inputs", coin_start_inputs);
    busf!("keymask_d", keymask_d);
    busf!("buttons", buttons);

    Diff { regs, state, state_addrs }
}

#[derive(Debug, PartialEq, Eq)]
enum Continuation {
    Converged,
    Diverged(Vec<String>),
    Inconclusive(String),
}

/// Bounded, single-CPU continuation. Equality must actually be reached; absence of an
/// observed divergence alone is inconclusive. Check state after every reference step so
/// an intermediate write cannot be hidden by a later overwrite.
fn continuation(
    rust_cpu: &Z80,
    rust_bus: &DigDugBus,
    ref_cpu: &Z80,
    ref_bus: &DigDugBus,
) -> Continuation {
    let (mut ac, mut ab) = (rust_cpu.clone(), rust_bus.clone());
    let (mut bc, mut bb) = (ref_cpu.clone(), ref_bus.clone());
    for n in 0..CONTINUATION_INSTR {
        if ac.halted || bc.halted {
            return Continuation::Inconclusive("continuation halted before register convergence".into());
        }
        if let Err(e) = ref_step(&mut ac, &mut ab) {
            return Continuation::Inconclusive(format!("Rust-state continuation: {e}"));
        }
        if let Err(e) = ref_step(&mut bc, &mut bb) {
            return Continuation::Inconclusive(format!("reference-state continuation: {e}"));
        }
        if ac.pc != bc.pc {
            return Continuation::Diverged(vec![format!(
                "control flow diverges after {} instr: rust-path pc={:04X} ref-path pc={:04X}",
                n + 1,
                ac.pc,
                bc.pc
            )]);
        }
        if !bus_state_matches(&ab, &bb, ac.sp) {
            let d = compare(&ac, &ab, &bc, &bb);
            return Continuation::Diverged(std::iter::once(format!(
                "state diverges after {} onward instructions:", n + 1)).chain(d.state).collect());
        }
        if register_values(&ac) == register_values(&bc) {
            return Continuation::Converged;
        }
    }
    Continuation::Inconclusive(format!("{CONTINUATION_INSTR}-instruction limit reached with register differences remaining"))
}

/// Replays the original ROM from the pre-step state and reports, for each watched address, the
/// instruction (and its step number) that wrote its final differing value.
fn trace_ref_writes(
    pre_cpu: &Z80,
    pre_bus: &DigDugBus,
    target_pc: u16,
    target_sp: u16,
    watch: &[u16],
    max_instr: u64,
) -> Vec<String> {
    let mut c = pre_cpu.clone();
    let mut b = pre_bus.clone();
    let mut last: Vec<Option<(u16, u8, u8, u64)>> = vec![None; watch.len()];
    let read = |b: &DigDugBus, a: u16| b.shared_ram[(a - 0x8000) as usize];
    for n in 1..=max_instr {
        let pc = c.pc;
        let before: Vec<u8> = watch.iter().map(|&a| read(&b, a)).collect();
        if ref_step(&mut c, &mut b).is_err() {
            break;
        }
        for (i, &a) in watch.iter().enumerate() {
            let now = read(&b, a);
            if now != before[i] {
                last[i] = Some((pc, before[i], now, n));
            }
        }
        if c.pc == target_pc && c.sp == target_sp {
            break;
        }
    }
    let mut out = vec!["-- ROM replay: last writer of each differing byte --".to_string()];
    for (i, &a) in watch.iter().enumerate() {
        match last[i] {
            Some((pc, was, now, n)) => out.push(format!(
                "ROM wrote ${:04X} = {:02X} (was {:02X}) at instruction ${:04X} (step {})",
                a, now, was, pc, n
            )),
            None => out.push(format!("ROM never wrote ${:04X} in this unit", a)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    fn states(program: &[u8]) -> (Z80, DigDugBus, Z80, DigDugBus) {
        let mut a = Z80::new(0);
        a.a = 0;
        a.sp = 0x9A00;
        let b = DigDugBus::new(test_support::roms(program));
        let mut other = a.clone();
        other.a = 1;
        (a, b.clone(), other, b)
    }

    #[test]
    fn register_difference_must_actually_converge() {
        let (a, ab, b, bb) = states(&[0xAF, 0x76]); // XOR A; HALT
        assert_eq!(continuation(&a, &ab, &b, &bb), Continuation::Converged);
    }

    #[test]
    fn instruction_limit_halt_and_unsupported_ops_are_inconclusive() {
        for program in [&[0xC3, 0, 0][..], &[0x76], &[0xED, 0]] {
            let (a, ab, b, bb) = states(program);
            assert!(matches!(continuation(&a, &ab, &b, &bb), Continuation::Inconclusive(_)));
        }
    }

    #[test]
    fn transient_memory_difference_is_detected_before_overwrite() {
        // LD ($8000),A; XOR A; LD ($8000),A. The second write would hide the first.
        let (a, ab, b, bb) = states(&[0x32, 0, 0x80, 0xAF, 0x32, 0, 0x80]);
        let Continuation::Diverged(differences) = continuation(&a, &ab, &b, &bb) else {
            panic!("missed an intermediate observable write");
        };
        assert!(differences.iter().any(|d| d.contains("RAM $8000")));
    }

    #[test]
    fn register_difference_that_changes_a_branch_is_a_failure() {
        let (a, ab, b, bb) = states(&[0xB7, 0xCA, 0x10, 0]); // OR A; JP Z,$0010
        assert!(matches!(continuation(&a, &ab, &b, &bb), Continuation::Diverged(_)));
    }

    #[test]
    fn empty_run_cannot_pass_and_outcomes_have_distinct_exit_codes() {
        assert_eq!(VerifyLog::new(10).outcome(false), VerificationOutcome::Inconclusive);
        assert_eq!(VerificationOutcome::Passed.exit_code(), 0);
        assert_eq!(VerificationOutcome::Failed.exit_code(), 1);
        assert_eq!(VerificationOutcome::Inconclusive.exit_code(), 2);
    }

    fn check_changed_a(program: &[u8]) -> VerifyLog {
        let (pre, bus, _, _) = states(program);
        let mut post = pre.clone();
        let mut post_bus = bus.clone();
        ref_step(&mut post, &mut post_bus).unwrap();
        post.a = 1;
        let mut log = VerifyLog::new(10);
        log.check(1, "synthetic", &pre, &bus, &post, &post_bus);
        log
    }

    #[test]
    fn strict_register_mode_rejects_even_sampled_convergence() {
        let log = check_changed_a(&[0, 0xAF, 0x76]);
        assert_eq!(log.mismatches[&(0, 0)].reg_converged, 1);
        assert_eq!(log.outcome(false), VerificationOutcome::Passed);
        assert_eq!(log.outcome(true), VerificationOutcome::Failed);
    }

    #[test]
    fn unresolved_and_unsampled_registers_prevent_passing() {
        let log = check_changed_a(&[0, 0x76]);
        assert_eq!(log.mismatches[&(0, 0)].reg_inconclusive, 1);
        assert_eq!(log.outcome(false), VerificationOutcome::Inconclusive);

        let program = [0, 0xAF, 0x76];
        let mut log = check_changed_a(&program);
        let (pre, bus, _, _) = states(&program);
        let mut post = pre.clone();
        let mut post_bus = bus.clone();
        ref_step(&mut post, &mut post_bus).unwrap();
        post.a = 1;
        for frame in 2..=13 {
            log.check(frame, "synthetic", &pre, &bus, &post, &post_bus);
        }
        assert_eq!(log.mismatches[&(0, 0)].reg_converged, 12);
        assert_eq!(log.mismatches[&(0, 0)].reg_unsampled, 1);
        assert_eq!(log.outcome(false), VerificationOutcome::Inconclusive);
    }

    #[test]
    fn timeout_reference_halt_and_unsupported_are_recorded_separately() {
        for (program, expected) in [(&[0][..], "timeout"), (&[0x76][..], "halt"), (&[0xED, 0][..], "unsupported")] {
            let (pre, bus, mut post, post_bus) = states(program);
            post.pc = 0x1234; // Unreachable in the one-instruction reference budget.
            let mut log = VerifyLog::new(1);
            log.check(1, "synthetic", &pre, &bus, &post, &post_bus);
            assert_eq!(log.timeouts.values().sum::<u64>(), u64::from(expected == "timeout"));
            assert_eq!(log.reference_halts.values().sum::<u64>(), u64::from(expected == "halt"));
            assert_eq!(log.unsupported.values().sum::<u64>(), u64::from(expected == "unsupported"));
            assert_eq!(log.outcome(false), VerificationOutcome::Inconclusive);
        }
    }

    #[test]
    fn interrupt_register_and_additional_bus_fields_are_compared() {
        let (a, ab, mut b, mut bb) = states(&[]);
        b.a = a.a;
        b.i = 1;
        assert!(!compare(&a, &ab, &b, &bb).regs.is_empty());
        for change in [
            (|b: &mut DigDugBus| b.sound_regs_tick0[0] = 1) as fn(&mut DigDugBus),
            |b: &mut DigDugBus| b.game_started = true,
            |b: &mut DigDugBus| b.buttons = 1,
            |b: &mut DigDugBus| b.coin_start_inputs.sample_frame(0x40),
            |b: &mut DigDugBus| b.coin_start_inputs.write_data(2),
        ] {
            bb = ab.clone();
            change(&mut bb);
            assert!(!bus_state_matches(&ab, &bb, a.sp));
            assert!(!compare(&a, &ab, &a, &bb).state.is_empty());
        }
    }
}
