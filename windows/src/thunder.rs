//! Thunder after the lightning: a crack and a long rumble, worked out once as sound rather than kept as a recording,
//! and played a moment after each stroke, later the farther off it seems. Off until someone turns it on.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::core::PCWSTR;

static ON: AtomicBool = AtomicBool::new(false);
/// Each stroke's thunder in turn, so a stroke right after another doesn't cut the first short.
static PENDING: AtomicU32 = AtomicU32::new(0);
const RATE: u32 = 22_050;

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Thunder for a stroke that just flashed, after a delay as if it were a few kilometres off.
pub fn after_flash(seed: u32) {
    if !ON.load(Ordering::Relaxed) || PENDING.load(Ordering::Relaxed) > 1 {
        return;
    }
    PENDING.fetch_add(1, Ordering::Relaxed);
    let delay = 900 + (seed.wrapping_mul(2_654_435_761) >> 20) % 3200;
    let near = delay < 2000;
    let _ = std::thread::Builder::new().name("thunder".into()).spawn(move || {
        std::thread::sleep(Duration::from_millis(delay as u64));
        let sound = if near { near_wave() } else { far_wave() };
        unsafe {
            let _ = PlaySoundW(PCWSTR(sound.as_ptr().cast()), None, SND_MEMORY | SND_ASYNC | SND_NODEFAULT);
        }
        PENDING.fetch_sub(1, Ordering::Relaxed);
    });
}

fn near_wave() -> &'static [u8] {
    static WAVE: OnceLock<Vec<u8>> = OnceLock::new();
    WAVE.get_or_init(|| wave(1, 0.55, 0.06, 4.2))
}

fn far_wave() -> &'static [u8] {
    static WAVE: OnceLock<Vec<u8>> = OnceLock::new();
    WAVE.get_or_init(|| wave(7, 0.32, 0.015, 4.8))
}

/// A thunderclap as a 16-bit mono WAV file: a short sharp crack, `crack` loud, then a deep rumble rolling a few
/// times as it dies away over `seconds`, `loudness` at its loudest.
fn wave(seed: u32, loudness: f32, crack: f32, seconds: f32) -> Vec<u8> {
    let count = (RATE as f32 * seconds) as usize;
    let mut state = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let mut white = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    // When the rumble rolls louder again, as the sound comes back from farther along the stroke.
    let rolls: Vec<(f32, f32)> = (0..4).map(|k| (0.35 + k as f32 * 0.7 + white().abs() * 0.4, 0.5 + white().abs() * 0.5)).collect();
    let (mut brown, mut low1, mut low2) = (0.0f32, 0.0f32, 0.0f32);
    let mut samples = Vec::with_capacity(count);
    for n in 0..count {
        let t = n as f32 / RATE as f32;
        let noise = white();
        brown = (brown + noise * 0.05) * 0.996;
        // Deep: low-passed twice.
        low1 += (brown - low1) * 0.05;
        low2 += (low1 - low2) * 0.05;
        let rolling: f32 = rolls.iter().map(|&(at, height)| height * (-((t - at) / 0.35).powi(2)).exp()).sum();
        let swell = (1.0 - (-t / 0.08).exp()) * (-t / (seconds * 0.38)).exp();
        let rumble = low2 * 9.0 * swell * (0.55 + rolling);
        let snap = noise * crack * (-t / 0.05).exp();
        samples.push(rumble + snap);
    }
    let peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs())).max(1e-6);
    let mut bytes = Vec::with_capacity(44 + count * 2);
    let data = (count * 2) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for (n, sample) in samples.iter().enumerate() {
        // Faded in and out over a few milliseconds, so it never clicks.
        let edge = ((n as f32 / 200.0).min(1.0)).min((count - n) as f32 / 2000.0).min(1.0);
        let value = (sample / peak * loudness * edge * 32767.0) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}
