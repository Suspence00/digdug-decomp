//! The observed coin/start and credit-mode fire qualification of the Namco 51XX.
//!
//! Inputs must agree on two successive frame samples before either edge is
//! accepted. Reads consume qualified press edges without advancing the filter.
//! This is a limited input model, not an emulation of the MCU or its output
//! register. Raw fire level, directions, coinage ratios and autonomous coin
//! processing remain outside this component.

const COIN_START_MASK: u8 = 0xE0;
const FIRE_MASK: u8 = 0x10;
const SAMPLED_MASK: u8 = COIN_START_MASK | FIRE_MASK;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Namco51Inputs {
    previous_raw: u8,
    qualified: u8,
    pending_presses: u8,
    packet_fire_pressed: bool,
    coinage_arguments: u8,
    pub(crate) starts_enabled: bool,
}

impl Namco51Inputs {
    /// Sample physical coin/start/fire lines once per emulated frame.
    /// A one-frame contrary sample leaves the qualified state unchanged.
    pub(crate) fn sample_frame(&mut self, buttons: u8) {
        let raw = buttons & SAMPLED_MASK;
        let agreed = !(raw ^ self.previous_raw) & SAMPLED_MASK;
        let next = (self.qualified & !agreed) | (raw & agreed);
        self.pending_presses |= next & !self.qualified;
        self.qualified = next;
        self.previous_raw = raw;
    }

    /// Consume edges for one host input packet. Repeated reads cannot qualify
    /// a short pulse or emit the same press again. Like the existing host-read
    /// model, the bitmask does not queue multiple presses of one line while the
    /// host is not polling.
    pub(crate) fn take_coin_start_presses(&mut self) -> u8 {
        let presses = self.pending_presses & COIN_START_MASK;
        self.pending_presses &= !COIN_START_MASK;
        presses
    }

    /// Ignore pending coin/start presses while preserving fire and switch state.
    pub(crate) fn clear_coin_start_presses(&mut self) {
        self.pending_presses &= !COIN_START_MASK;
    }

    /// Latch one qualified fire press for a new host packet. Repeated reads of
    /// its controller byte see the same edge; the next packet clears the edge
    /// unless another physical press has qualified in the meantime.
    pub(crate) fn begin_fire_packet(&mut self) {
        self.packet_fire_pressed = self.pending_presses & FIRE_MASK != 0;
        self.pending_presses &= !FIRE_MASK;
    }

    pub(crate) fn fire_pressed(&self) -> bool {
        self.packet_fire_pressed
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
    const FIRE: u8 = 0x10;

    #[test]
    fn one_frame_pulses_are_rejected_for_each_coin_and_start_line() {
        for line in [COIN, START1, START2] {
            let mut inputs = Namco51Inputs::default();
            for sample in [0, line, 0, 0, 0] {
                inputs.sample_frame(sample);
                assert_eq!(inputs.take_coin_start_presses(), 0);
            }
        }
    }

    #[test]
    fn a_second_press_requires_both_a_qualified_release_and_a_qualified_press() {
        let mut inputs = Namco51Inputs::default();
        let samples = [COIN, COIN, COIN, 0, COIN, COIN, 0, 0, COIN, COIN];
        let mut events = Vec::new();
        for (frame, sample) in samples.into_iter().enumerate() {
            inputs.sample_frame(sample);
            if inputs.take_coin_start_presses() != 0 { events.push(frame + 1); }
        }
        assert_eq!(events, [2, 10]);
    }

    #[test]
    fn independent_lines_can_qualify_while_another_line_remains_held() {
        let mut inputs = Namco51Inputs::default();
        inputs.sample_frame(COIN);
        inputs.sample_frame(COIN | START1);
        assert_eq!(inputs.take_coin_start_presses(), COIN);
        inputs.sample_frame(COIN | START1 | START2);
        assert_eq!(inputs.take_coin_start_presses(), START1);
        inputs.sample_frame(COIN | START1 | START2);
        assert_eq!(inputs.take_coin_start_presses(), START2);
        inputs.sample_frame(0x1F | COIN | START1 | START2);
        assert_eq!(inputs.take_coin_start_presses(), 0, "unrelated controls must not create coin/start edges");
    }

    #[test]
    fn host_reads_do_not_advance_the_sample_clock_or_repeat_an_edge() {
        let mut inputs = Namco51Inputs::default();
        inputs.sample_frame(COIN);
        for _ in 0..100 { assert_eq!(inputs.take_coin_start_presses(), 0); }
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_coin_start_presses(), COIN);
        for _ in 0..100 { assert_eq!(inputs.take_coin_start_presses(), 0); }
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_coin_start_presses(), 0);
    }

    #[test]
    fn pending_different_presses_accumulate_until_the_host_consumes_them() {
        let mut inputs = Namco51Inputs::default();
        for sample in [COIN, COIN, COIN | START1, COIN | START1] {
            inputs.sample_frame(sample);
        }
        assert_eq!(inputs.take_coin_start_presses(), COIN | START1);
        assert_eq!(inputs.take_coin_start_presses(), 0);
    }

    #[test]
    fn fire_rejects_a_single_frame_pulse_and_emits_one_press_for_a_hold() {
        let mut inputs = Namco51Inputs::default();
        for sample in [0, FIRE, 0, 0] {
            inputs.sample_frame(sample);
            inputs.begin_fire_packet();
            assert!(!inputs.fire_pressed());
        }
        inputs.sample_frame(FIRE);
        inputs.begin_fire_packet();
        assert!(!inputs.fire_pressed());
        inputs.sample_frame(FIRE);
        inputs.begin_fire_packet();
        assert!(inputs.fire_pressed());
        for _ in 0..30 {
            inputs.sample_frame(FIRE);
            inputs.begin_fire_packet();
            assert!(!inputs.fire_pressed(), "holding fire must not create another launch press");
        }
    }

    #[test]
    fn fire_repress_requires_two_release_samples_then_two_press_samples() {
        let mut inputs = Namco51Inputs::default();
        let samples = [FIRE, FIRE, FIRE, 0, FIRE, FIRE, 0, 0, FIRE, FIRE];
        let mut presses = Vec::new();
        for (frame, sample) in samples.into_iter().enumerate() {
            inputs.sample_frame(sample);
            inputs.begin_fire_packet();
            if inputs.fire_pressed() { presses.push(frame + 1); }
        }
        assert_eq!(presses, [2, 10]);
    }

    #[test]
    fn fire_reads_are_stable_for_one_packet_and_cannot_advance_qualification() {
        let mut inputs = Namco51Inputs::default();
        inputs.sample_frame(FIRE);
        for _ in 0..100 {
            inputs.begin_fire_packet();
            assert!(!inputs.fire_pressed(), "host polls cannot manufacture a second frame sample");
        }
        inputs.sample_frame(FIRE);
        inputs.begin_fire_packet();
        for _ in 0..100 { assert!(inputs.fire_pressed()); }
        inputs.begin_fire_packet();
        for _ in 0..100 { assert!(!inputs.fire_pressed()); }
        inputs.sample_frame(FIRE);
        inputs.begin_fire_packet();
        assert!(!inputs.fire_pressed(), "another packet cannot repeat the held press");
    }

    #[test]
    fn fire_packets_and_coin_start_reads_consume_only_their_own_presses() {
        for fire_first in [false, true] {
            let mut inputs = Namco51Inputs::default();
            inputs.sample_frame(FIRE | COIN | START1);
            inputs.sample_frame(FIRE | COIN | START1);
            if fire_first { inputs.begin_fire_packet(); }
            assert_eq!(inputs.take_coin_start_presses(), COIN | START1);
            if !fire_first { inputs.begin_fire_packet(); }
            assert!(inputs.fire_pressed());
            assert_eq!(inputs.take_coin_start_presses(), 0);
            inputs.begin_fire_packet();
            assert!(!inputs.fire_pressed());
        }
    }

    #[test]
    fn switch_and_credit_commands_control_start_deductions() {
        let mut inputs = Namco51Inputs::default();
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
        let mut inputs = Namco51Inputs::default();
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
        let mut inputs = Namco51Inputs::default();
        inputs.sample_frame(START1 | FIRE);
        inputs.sample_frame(START1 | FIRE);
        inputs.clear_coin_start_presses();
        assert_eq!(inputs.take_coin_start_presses(), 0);
        inputs.begin_fire_packet();
        assert!(inputs.fire_pressed(), "raw-mode coin/start cleanup must preserve pending fire");
        inputs.clear_coin_start_presses();
        assert!(inputs.fire_pressed(), "cleanup must preserve the current fire packet too");
        inputs.sample_frame(START1 | FIRE);
        assert_eq!(inputs.take_coin_start_presses(), 0);
        inputs.begin_fire_packet();
        assert!(!inputs.fire_pressed(), "cleanup must keep the physical fire latch qualified");
    }

    #[test]
    fn chip_reset_discards_all_filter_and_command_state() {
        let mut inputs = Namco51Inputs::default();
        inputs.write_data(2);
        inputs.sample_frame(COIN | FIRE);
        inputs.sample_frame(COIN | FIRE);
        inputs.begin_fire_packet();
        assert!(inputs.fire_pressed());
        // Leave another fire press pending as well as the current packet edge.
        for sample in [0, 0, FIRE, FIRE] { inputs.sample_frame(sample); }
        inputs.write_data(1);
        inputs.reset();
        assert_eq!(inputs, Namco51Inputs::default());
        assert_eq!(inputs.take_coin_start_presses(), 0);
        assert!(!inputs.fire_pressed());
        inputs.begin_fire_packet();
        assert!(!inputs.fire_pressed());
        inputs.write_data(2);
        assert!(inputs.starts_enabled, "reset must discard the unfinished coinage packet");
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_coin_start_presses(), 0);
        inputs.sample_frame(COIN);
        assert_eq!(inputs.take_coin_start_presses(), COIN);
    }
}
