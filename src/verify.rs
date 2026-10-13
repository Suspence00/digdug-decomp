//! Differential verification of the hand-translated Rust game code against the original ROM.
//!
//! For every CPU step (a "unit": one decompiled routine, loop, or instruction block) the
//! machine snapshots CPU + bus, runs the Rust implementation, then runs the independent
//! reference interpreter (`z80_ref.rs`) on the snapshot until it reaches the same `pc`/`sp`
//! the Rust code stopped at. Differences are classified:
//!
//! * **STATE** — non-stack RAM or bus state differs at unit exit. Always a real divergence.
//! * **REG-PROPAGATES** — only registers differ, but running the original ROM onward from both
//!   end states diverges (the stale register value is actually consumed). A real bug.
//! * **REG-BENIGN** — only registers differ and onward execution converges (dead value).
//!
//! Bytes just below the executing CPU's stack pointer are ignored: the ROM leaves pushed
//! return addresses there while the Rust hooks don't push, and nothing reads them back.

use std::collections::HashMap;

use crate::hardware::DigDugBus;
use crate::z80::Z80;
use crate::z80_ref::{ref_run_until, ref_step, RefRun};

/// Flag bits that are architecturally defined (excludes the undocumented X/Y copies).
pub const FLAG_MASK: u8 = 0xD7;
/// How far below SP residue is considered dead stack.
const DEAD_STACK_WINDOW: u16 = 0x60;
/// Instructions of onward ROM execution used by the liveness test.
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
    pub reg_benign: u64,
    pub reg_unsampled: u64,
    pub state_example: Vec<String>,
    pub propagate_example: Vec<String>,
    pub reg_example: Vec<String>,
}

#[derive(Default)]
pub struct VerifyLog {
    pub max_instr: u64,
    pub checked: u64,
    pub skipped_halted: u64,
    pub timeouts: HashMap<(usize, u16), u64>,
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
            RefRun::Timeout | RefRun::Halted => {
                *self.timeouts.entry(key).or_insert(0) += 1;
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
        if m.reg_propagates + m.reg_benign >= SAMPLES_PER_UNIT {
            m.reg_unsampled += 1;
            return;
        }
        match continuation(post_cpu, post_bus, &rcpu, &rbus) {
            None => m.reg_benign += 1,
            Some(d) => {
                m.reg_propagates += 1;
                if m.propagate_example.is_empty() {
                    m.propagate_example = diff.regs.iter().cloned().chain(d).collect();
                }
            }
        }
    }
}

fn compare(rust: &Z80, rbus: &DigDugBus, refc: &Z80, refbus: &DigDugBus) -> Diff {
    let mut regs: Vec<String> = Vec::new();
    let mut state: Vec<String> = Vec::new();
    macro_rules! reg {
        ($name:expr, $a:expr, $b:expr) => {
            if $a != $b {
                regs.push(format!("{}: rust={:02X?} ref={:02X?}", $name, $a, $b));
            }
        };
    }
    reg!("A", rust.a, refc.a);
    if (rust.f & FLAG_MASK) != (refc.f & FLAG_MASK) {
        regs.push(format!(
            "F: rust={:02X} ref={:02X} (SZ-H-PNC mask)",
            rust.f & FLAG_MASK,
            refc.f & FLAG_MASK
        ));
    }
    reg!("B", rust.b, refc.b);
    reg!("C", rust.c, refc.c);
    reg!("D", rust.d, refc.d);
    reg!("E", rust.e, refc.e);
    reg!("H", rust.h, refc.h);
    reg!("L", rust.l, refc.l);
    reg!("A'", rust.a_alt, refc.a_alt);
    reg!("B'", rust.b_alt, refc.b_alt);
    reg!("C'", rust.c_alt, refc.c_alt);
    reg!("D'", rust.d_alt, refc.d_alt);
    reg!("E'", rust.e_alt, refc.e_alt);
    reg!("H'", rust.h_alt, refc.h_alt);
    reg!("L'", rust.l_alt, refc.l_alt);
    if (rust.f_alt & FLAG_MASK) != (refc.f_alt & FLAG_MASK) {
        regs.push(format!(
            "F': rust={:02X} ref={:02X}",
            rust.f_alt & FLAG_MASK,
            refc.f_alt & FLAG_MASK
        ));
    }
    reg!("IX", rust.ix, refc.ix);
    reg!("IY", rust.iy, refc.iy);
    reg!("SP", rust.sp, refc.sp);
    reg!("PC", rust.pc, refc.pc);
    reg!("IFF1", rust.iff1, refc.iff1);
    reg!("IFF2", rust.iff2, refc.iff2);
    reg!("IM", rust.im, refc.im);
    reg!("HALT", rust.halted, refc.halted);

    let sp = rust.sp;
    let in_dead_stack = |addr: u16| addr < sp && sp.wrapping_sub(addr) <= DEAD_STACK_WINDOW;
    let mut ram_diffs = 0usize;
    let mut state_addrs: Vec<u16> = Vec::new();
    for i in 0..rbus.shared_ram.len() {
        if rbus.shared_ram[i] != refbus.shared_ram[i] {
            let addr = 0x8000u16 + i as u16;
            if in_dead_stack(addr) {
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
    busf!("earom", earom);
    busf!("earom_addr", earom_addr);
    busf!("earom_data", earom_data);
    busf!("irq_enable", irq_enable);
    busf!("sub_cpu_reset", sub_cpu_reset);
    busf!("video_latch", video_latch);
    busf!("namco_command", namco_command);
    busf!("namco_mode", namco_mode);
    busf!("namco_nmi_counter", namco_nmi_counter);
    busf!("namco_credit", namco_credit);
    busf!("keymask_d", keymask_d);

    Diff { regs, state, state_addrs }
}

/// Liveness test: run the original ROM onward from both end states. If the register difference
/// is dead the two runs converge; otherwise memory/bus/control flow differs. Returns the
/// divergence description, or `None` if they stay equivalent.
fn continuation(
    rust_cpu: &Z80,
    rust_bus: &DigDugBus,
    ref_cpu: &Z80,
    ref_bus: &DigDugBus,
) -> Option<Vec<String>> {
    let (mut ac, mut ab) = (rust_cpu.clone(), rust_bus.clone());
    let (mut bc, mut bb) = (ref_cpu.clone(), ref_bus.clone());
    for n in 0..CONTINUATION_INSTR {
        if ac.halted || bc.halted {
            break;
        }
        if ref_step(&mut ac, &mut ab).is_err() || ref_step(&mut bc, &mut bb).is_err() {
            break;
        }
        if ac.pc != bc.pc {
            return Some(vec![format!(
                "control flow diverges after {} instr: rust-path pc={:04X} ref-path pc={:04X}",
                n + 1,
                ac.pc,
                bc.pc
            )]);
        }
    }
    let d = compare(&ac, &ab, &bc, &bb);
    if d.state.is_empty() {
        None
    } else {
        Some(
            std::iter::once("state diverges during onward ROM execution:".to_string())
                .chain(d.state)
                .collect(),
        )
    }
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