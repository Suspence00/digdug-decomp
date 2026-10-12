use std::collections::VecDeque;
#[cfg(windows)]
use std::mem::zeroed;
#[cfg(windows)]
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use windows_sys::Win32::Media::Audio::{
    waveOutClose, waveOutOpen, waveOutPrepareHeader, waveOutReset, waveOutWrite, HWAVEOUT,
    WAVEFORMATEX, WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};

pub const SAMPLE_RATE: u32 = 44_100;
#[cfg(windows)]
const SAMPLES_PER_120HZ_TICK: usize = (SAMPLE_RATE as usize) / 120; // 367 samples per 120Hz Sound CPU NMI tick (~8.33ms)
#[cfg(windows)]
const NUM_BUFFERS: usize = 6; // 6 * 8.33ms = ~50ms ring buffer lookahead
#[cfg(windows)]
const WSG_CLOCK: u64 = 96_000; // 18.432 MHz / 6 / 32

struct SharedWsgState {
    tick_queue: VecDeque<[u8; 32]>,
    last_regs: [u8; 32],
    wavetable: [[u8; 32]; 8],
    active: bool,
}

pub struct NamcoWsgAudio {
    state: Arc<Mutex<SharedWsgState>>,
    running: Arc<AtomicBool>,
}

impl NamcoWsgAudio {
    /// Spawns a dedicated 120 Hz Namco WSG hardware synthesizer thread.
    /// Consumes both 120 Hz Sound CPU NMI register ticks per 60 Hz video frame and keeps
    /// 6 `waveOut` buffers continuously queued so audio never underruns or drops half-frame notes.
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(SharedWsgState {
            tick_queue: VecDeque::with_capacity(12),
            last_regs: [0u8; 32],
            wavetable: [[8u8; 32]; 8],
            active: false,
        }));
        let running = Arc::new(AtomicBool::new(true));

        #[cfg(windows)]
        {
            let state_clone = Arc::clone(&state);
            let running_clone = Arc::clone(&running);

            thread::spawn(move || {
            let mut h_wave_out: HWAVEOUT = null_mut();
            let wfx = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };

            let opened = unsafe {
                waveOutOpen(&mut h_wave_out, WAVE_MAPPER, &wfx, 0, 0, 0) == 0
                    && !h_wave_out.is_null()
            };
            if !opened {
                return;
            }

            let mut buffers: Vec<Box<[i16; SAMPLES_PER_120HZ_TICK]>> =
                Vec::with_capacity(NUM_BUFFERS);
            let mut headers: Vec<Box<WAVEHDR>> = Vec::with_capacity(NUM_BUFFERS);

            for _ in 0..NUM_BUFFERS {
                let mut buf = Box::new([0i16; SAMPLES_PER_120HZ_TICK]);
                let mut hdr: Box<WAVEHDR> = Box::new(unsafe { zeroed() });
                hdr.lpData = buf.as_mut_ptr() as *mut u8;
                hdr.dwBufferLength =
                    (SAMPLES_PER_120HZ_TICK * std::mem::size_of::<i16>()) as u32;
                unsafe {
                    waveOutPrepareHeader(
                        h_wave_out,
                        hdr.as_mut() as *mut WAVEHDR,
                        std::mem::size_of::<WAVEHDR>() as u32,
                    );
                }
                hdr.dwFlags |= WHDR_DONE;
                buffers.push(buf);
                headers.push(hdr);
            }

            let mut phase = [0u32; 3];
            let mut current_buf = 0usize;

            while running_clone.load(Ordering::Relaxed) {
                let hdr = &mut headers[current_buf];
                if (hdr.dwFlags & WHDR_DONE) == 0 {
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }

                let (sound_regs, wavetable, active) = {
                    if let Ok(mut guard) = state_clone.lock() {
                        let regs = guard.tick_queue.pop_front().unwrap_or(guard.last_regs);
                        (regs, guard.wavetable, guard.active)
                    } else {
                        ([0u8; 32], [[8u8; 32]; 8], false)
                    }
                };

                let buf = &mut buffers[current_buf];
                if !active {
                    buf.fill(0);
                } else {
                    let waves: [usize; 3] = [
                        (sound_regs[0x05] & 0x07) as usize,
                        (sound_regs[0x0A] & 0x07) as usize,
                        (sound_regs[0x0F] & 0x07) as usize,
                    ];

                    let freqs: [u32; 3] = [
                        (sound_regs[0x10] as u32 & 0x0F)
                            | ((sound_regs[0x11] as u32 & 0x0F) << 4)
                            | ((sound_regs[0x12] as u32 & 0x0F) << 8)
                            | ((sound_regs[0x13] as u32 & 0x0F) << 12)
                            | ((sound_regs[0x14] as u32 & 0x0F) << 16),
                        ((sound_regs[0x16] as u32 & 0x0F) << 4)
                            | ((sound_regs[0x17] as u32 & 0x0F) << 8)
                            | ((sound_regs[0x18] as u32 & 0x0F) << 12)
                            | ((sound_regs[0x19] as u32 & 0x0F) << 16),
                        ((sound_regs[0x1B] as u32 & 0x0F) << 4)
                            | ((sound_regs[0x1C] as u32 & 0x0F) << 8)
                            | ((sound_regs[0x1D] as u32 & 0x0F) << 12)
                            | ((sound_regs[0x1E] as u32 & 0x0F) << 16),
                    ];

                    let vols: [i32; 3] = [
                        (sound_regs[0x15] & 0x0F) as i32,
                        (sound_regs[0x1A] & 0x0F) as i32,
                        (sound_regs[0x1F] & 0x0F) as i32,
                    ];

                    let steps_fp8: [u32; 3] = [
                        ((freqs[0] as u64 * WSG_CLOCK * 256) / SAMPLE_RATE as u64) as u32,
                        ((freqs[1] as u64 * WSG_CLOCK * 256) / SAMPLE_RATE as u64) as u32,
                        ((freqs[2] as u64 * WSG_CLOCK * 256) / SAMPLE_RATE as u64) as u32,
                    ];

                    for sample_out in buf.iter_mut() {
                        let mut mix: i32 = 0;
                        for ch in 0..3 {
                            if vols[ch] > 0 && freqs[ch] > 0 {
                                phase[ch] = phase[ch].wrapping_add(steps_fp8[ch]) & 0x0FFF_FFFF;
                                let sample_idx = ((phase[ch] >> 23) & 0x1F) as usize;
                                let wave_sample = wavetable[waves[ch]][sample_idx] as i32 - 8;
                                mix += wave_sample * vols[ch];
                            }
                        }
                        *sample_out = (mix * 45).clamp(-32767, 32767) as i16;
                    }
                }

                hdr.dwFlags &= !WHDR_DONE;
                unsafe {
                    waveOutWrite(
                        h_wave_out,
                        hdr.as_mut() as *mut WAVEHDR,
                        std::mem::size_of::<WAVEHDR>() as u32,
                    );
                }
                current_buf = (current_buf + 1) % NUM_BUFFERS;
            }

            unsafe {
                waveOutReset(h_wave_out);
                waveOutClose(h_wave_out);
            }
        });
        }

        Self { state, running }
    }

    #[inline(always)]
    pub fn push_frame(
        &mut self,
        sound_regs_tick0: &[u8; 32],
        sound_regs_tick1: &[u8; 32],
        wavetable: &[[u8; 32]; 8],
    ) {
        if let Ok(mut guard) = self.state.lock() {
            if !guard.active {
                guard.active = true;
                guard.wavetable = *wavetable;
            }
            if guard.tick_queue.len() >= 8 {
                guard.tick_queue.pop_front();
                guard.tick_queue.pop_front();
            }
            guard.tick_queue.push_back(*sound_regs_tick0);
            guard.tick_queue.push_back(*sound_regs_tick1);
            guard.last_regs = *sound_regs_tick1;
        }
    }
}

impl Drop for NamcoWsgAudio {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
