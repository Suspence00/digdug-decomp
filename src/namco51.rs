//! The observed coin/start input qualification of the Namco 51XX.
//!
//! Inputs must agree on two successive frame samples before either edge is
//! accepted. Reads consume qualified press edges without advancing the filter.
//! This is a limited input model, not an emulation of the MCU or its output
//! register. Fire, directions, coinage ratios and autonomous coin processing
//! remain outside this component.

const COIN_START_MASK: u8 = 0xE0;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct CoinStartInputs {
    previous_raw: u8,
    qualified: u8,
    pending_presses: u8,
    coinage_arguments: u8,
    pub(crate) starts_enabled: bool,
}

impl CoinStartInputs {
    /// Sample physical coin/start lines once per emulated frame.
    /// A one-frame contrary sample leaves the qualified state unchanged.
    pub(crate) fn sample_frame(&mut self, buttons: u8) {
        let raw = buttons & COIN_START_MASK;
        let agreed = !(raw ^ self.previous_raw) & COIN_START_MASK;
        let next = (self.qualified & !agreed) | (raw & agreed);
        self.pending_presses |= next & !self.qualified;
        self.qualified = next;
        self.previous_raw = raw;
    }

    /// Consume edges for one host input packet. Repeated reads cannot qualify
    /// a short pulse or emit the same press again. Like the existing host-read
    /// model, the bitmask does not queue multiple presses of one line while the
    /// host is not polling.
    pub(crate) fn take_presses(&mut self) -> u8 {
        std::mem::take(&mut self.pending_presses)
    }

    /// Ignore pending presses while preserving the physical switch state.
    pub(crate) fn clear_presses(&mut self) {
        self.pending_presses = 0;
    }

    /// Handle the relevant low-three-bit 51XX data commands. These are distinct
    /// from the 06XX transfer-control register used to select the chip.
    pub(crate) fn write_data(&mut self, value: u8) {
        if self.coinage_arguments != 0 {
            self.coinage_arguments -= 1;
            return;
        }
        match value & 0x07 {
            1 => self.coinage_arguments = 4,
            2 => self.starts_enabled = true,
            5 => self.starts_enabled = false,
            _ => {}
        }
    }

    /// A successful start disarms further deductions until command 2 rearms it.
    pub(crate) fn disable_starts(&mut self) {
        self.starts_enabled = false;
    }

    /// Reset the chip-side qualification, pending edges and command state.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COIN: u8 = 0x40;
    const START1: u8 = 0x20;
    const START2: u8 = 0x80;

    #[test]
    fn one_frame_pulses_are_rejected_for_each_coin_and_start_line() {
        for line in [COIN, START1, START2] {
            let mut inputs = CoinStartInputs::default();
            for sample in [0, line, 0, 0, 0] {
                inputs.sample_frame(sample);
                assert_eq!(inputs.take_presses(), 0);
            }
        }
    }

    #[test]
    fn a_second_press_requires_both_a_qualified_release_and_a_qualified_press() {
        let mut inputs = CoinStartInputs::default();
        let samples = [COIN, COIN, COIN, 0, COIN, COIN, 0, 0, COIN, COIN];
        let mut events = Vec::new();
        for (frame, sample) in samples.into_iter().enumerate() {
            inputs.sample_frame(sample);
            if inputs.take_presses() != 0 { events.push(frame + 1); }
        }
        assert_eq!(events, [2, 10]);
    }

    #[test]
    fn independent_lines_can_qualify_while_another_line_remains_held() {
        let mut inputs = CoinStartInputs::default();
        inputs.sample_frame(COIN);
        inputs.sample_frame(COIN | START1);
        assert_eq!(inputs.take_presses(), COIN);
        inputs.sample_frame(COIN | START1 | START2);
        assert_eq!(inputs.take_presses(), START1);
        inputs.sample_frame(COIN | START1 | START2);
        assert_eq!(inputs.take_presses(), START2);
        inputs.sample_frame(0x1F | COIN | START1 | START2);
        assert_eq!(inputs.take_presses(), 0, "unrelated controls must not create coin/start edges");
    }

    #[test]
    fn host_reads_do_not_advance_the_sample_clock_or_repeat_an_edge() {
        let mut inputs = CoinStartInputs::default();
        inputs.sample_frame(COIN);
        for _ in 0..100 { assert_eq!(inputs.take_presses(), 0); }
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_presses(), COIN);
        for _ in 0..100 { assert_eq!(inputs.take_presses(), 0); }
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_presses(), 0);
    }

    #[test]
    fn pending_different_presses_accumulate_until_the_host_consumes_them() {
        let mut inputs = CoinStartInputs::default();
        for sample in [COIN, COIN, COIN | START1, COIN | START1] {
            inputs.sample_frame(sample);
        }
        assert_eq!(inputs.take_presses(), COIN | START1);
        assert_eq!(inputs.take_presses(), 0);
    }

    #[test]
    fn switch_and_credit_commands_control_start_deductions() {
        let mut inputs = CoinStartInputs::default();
        assert!(!inputs.starts_enabled);
        inputs.write_data(2);
        assert!(inputs.starts_enabled);
        inputs.disable_starts();
        assert!(!inputs.starts_enabled);
        inputs.write_data(2);
        assert!(inputs.starts_enabled);
        inputs.write_data(5);
        assert!(!inputs.starts_enabled);
    }

    #[test]
    fn coinage_arguments_are_not_interpreted_as_mode_commands() {
        let mut inputs = CoinStartInputs::default();
        inputs.write_data(1);
        for argument in [2, 5, 2, 5] {
            inputs.write_data(argument);
            assert!(!inputs.starts_enabled);
        }
        inputs.write_data(0xFA); // Only the low three command bits are connected.
        assert!(inputs.starts_enabled);
        inputs.write_data(1);
        for argument in [5, 2, 5, 2] {
            inputs.write_data(argument);
            assert!(inputs.starts_enabled);
        }
        inputs.write_data(5);
        assert!(!inputs.starts_enabled);
    }

    #[test]
    fn clearing_pending_raw_mode_presses_keeps_the_qualified_latch() {
        let mut inputs = CoinStartInputs::default();
        inputs.sample_frame(START1);
        inputs.sample_frame(START1);
        inputs.clear_presses();
        inputs.sample_frame(START1);
        assert_eq!(inputs.take_presses(), 0);
    }

    #[test]
    fn chip_reset_discards_all_filter_and_command_state() {
        let mut inputs = CoinStartInputs::default();
        inputs.write_data(2);
        inputs.sample_frame(COIN);
        inputs.sample_frame(COIN);
        inputs.write_data(1);
        inputs.reset();
        assert_eq!(inputs, CoinStartInputs::default());
        assert_eq!(inputs.take_presses(), 0);
        inputs.write_data(2);
        assert!(inputs.starts_enabled, "reset must discard the unfinished coinage packet");
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_presses(), 0);
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_presses(), COIN);
    }
}
