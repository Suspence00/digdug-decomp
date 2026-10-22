//! Incremental pacing for translated work whose original cycle cost is known.
//!
//! Other translated blocks retain the existing one-slot scheduling behavior.
//! This is not a general cycle-accurate scheduler: callers must explicitly audit
//! a block before charging it, and must keep scheduling the other CPUs and I/O.

/// MAME 0.289 configures the CPU at master clock / 6 and the raster at master
/// clock / 3 with 384 * 264 pixels. Thus one video frame spans 384 * 264 / 2 CPU
/// cycles, independently of the host's frame pacing.
///
/// Source: <https://github.com/mamedev/mame/blob/mame0289/src/mame/namco/galaga.cpp#L1724>
pub const CPU_CYCLES_PER_FRAME: u64 = 50_688;

/// Four CPU steps per group, 1,250 groups per quarter, four quarters per frame.
pub const CPU_STEPS_PER_FRAME: u64 = 20_000;

/// Converts audited cycle costs into waits within the existing CPU-slot clock.
///
/// One debt unit is 1 / 20,000 of a CPU cycle; each scheduler slot advances time
/// by 50,688 units. A signed remainder preserves fractional slots across adjacent
/// audited blocks instead of rounding every instruction's duration upward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CyclePacer {
    // i128 can represent every u64 cycle cost multiplied by the slot denominator.
    debt_units: i128,
}

impl CyclePacer {
    /// Poll once for every CPU slot. A false result consumes that slot as elapsed
    /// time; a true result permits one translated step, followed by `charge`.
    /// Other CPUs, timers and interrupt requests must still advance on false.
    #[must_use]
    pub fn ready(&mut self) -> bool {
        if self.debt_units > 0 {
            self.debt_units -= i128::from(CPU_CYCLES_PER_FRAME);
            false
        } else {
            true
        }
    }

    /// Charge an admitted step's audited cycle cost, including its current slot.
    /// Pass zero for an unaudited legacy block; this restores normal one-slot
    /// scheduling and drops only the fractional credit left by completed work.
    /// Call after `ready` returns true, never to bypass outstanding work.
    pub fn charge(&mut self, audited_cycles: u64) {
        if audited_cycles == 0 {
            self.debt_units = 0;
        } else {
            self.debt_units += i128::from(audited_cycles) * i128::from(CPU_STEPS_PER_FRAME)
                - i128::from(CPU_CYCLES_PER_FRAME);
        }
    }

    /// Preserve interrupted work while accounting for any separately audited
    /// interrupt service cost. Zero preserves the debt unchanged. This does not
    /// consume a normal CPU slot: the caller services the interrupt separately
    /// and must not call `charge(0)` for it, which would erase the interrupted work.
    pub fn interrupt(&mut self, audited_cycles: u64) {
        self.debt_units += i128::from(audited_cycles) * i128::from(CPU_STEPS_PER_FRAME);
    }

    /// Clear all pending work and fractional credit when the owning CPU resets.
    pub fn reset(&mut self) {
        self.debt_units = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Return the first slot boundary at which all supplied work is complete.
    fn completion_slot(costs: impl IntoIterator<Item = u64>) -> u64 {
        let mut work = costs.into_iter();
        let mut pacer = CyclePacer::default();
        let mut slot = 0;
        loop {
            if pacer.ready() {
                match work.next() {
                    Some(cycles) => pacer.charge(cycles),
                    None => return slot,
                }
            }
            slot += 1;
        }
    }

    fn assert_within_one_slot(slots: u64, cycles: u64) {
        let elapsed = u128::from(slots) * u128::from(CPU_CYCLES_PER_FRAME);
        let required = u128::from(cycles) * u128::from(CPU_STEPS_PER_FRAME);
        assert!(elapsed >= required, "work completed before its cycle cost elapsed");
        assert!(elapsed - required < u128::from(CPU_CYCLES_PER_FRAME),
            "rounding drift exceeded one scheduler slot");
    }

    #[test]
    fn watchdog_delay_consumes_nineteen_board_frames() {
        // LD HL costs 10; 24,575 taken iterations cost 39 each; the last
        // iteration and RET cost 44. These are the original $005A loop costs.
        let costs = std::iter::once(10)
            .chain(std::iter::repeat(39).take(0x6000 - 1))
            .chain(std::iter::once(44));
        let slots = completion_slot(costs);
        assert_within_one_slot(slots, 958_479);
        assert!(slots > 18 * CPU_STEPS_PER_FRAME);
        assert!(slots < 19 * CPU_STEPS_PER_FRAME);
    }

    #[test]
    fn successive_iterations_preserve_fractional_time_without_rounding_drift() {
        let iterations = 10_000;
        let slots = completion_slot(std::iter::repeat(39).take(iterations));
        assert_within_one_slot(slots, iterations as u64 * 39);
    }

    #[test]
    fn outstanding_work_carries_across_a_video_frame() {
        let mut pacer = CyclePacer::default();
        assert!(pacer.ready());
        pacer.charge(CPU_CYCLES_PER_FRAME + 39);
        for _ in 1..CPU_STEPS_PER_FRAME {
            assert!(!pacer.ready());
        }
        // After the first frame, another 39 cycles remain. Sixteen slots
        // advance by just over that duration; the following slot is eligible.
        for _ in 0..16 {
            assert!(!pacer.ready());
        }
        assert!(pacer.ready());
    }

    #[test]
    fn legacy_block_restores_one_slot_execution_and_clears_fractional_credit() {
        let mut pacer = CyclePacer::default();
        assert!(pacer.ready());
        pacer.charge(1);
        assert!(pacer.debt_units < 0);
        assert!(pacer.ready());
        pacer.charge(0);
        assert_eq!(pacer, CyclePacer::default());
        for _ in 0..32 {
            assert!(pacer.ready());
            pacer.charge(0);
        }
    }

    #[test]
    fn interrupt_preserves_work_and_can_extend_it_by_an_audited_cost() {
        let mut pacer = CyclePacer::default();
        pacer.charge(39);
        let before = pacer.debt_units;
        pacer.interrupt(0);
        assert_eq!(pacer.debt_units, before);
        pacer.interrupt(11);
        assert_eq!(pacer.debt_units, before + 11 * i128::from(CPU_STEPS_PER_FRAME));
        assert!(!pacer.ready());
    }

    #[test]
    fn reset_discards_outstanding_work() {
        let mut pacer = CyclePacer::default();
        pacer.charge(2 * CPU_CYCLES_PER_FRAME);
        assert!(!pacer.ready());
        pacer.reset();
        assert_eq!(pacer, CyclePacer::default());
        assert!(pacer.ready());
    }
}
