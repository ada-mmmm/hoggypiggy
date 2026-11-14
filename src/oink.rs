//! The hog vocal apparatus: whatever the host sends in comes back out as oinks.
//!
//! A fast envelope follower watches the input. When the input makes a sound,
//! a real pig recording (loaded from WAV files) is played back with randomized
//! pitch and loudness, so every kick, vocal, and bassline in the project is
//! faithfully translated into pig. Falls back to a crude formant synth if no
//! WAV files can be found.

use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// WAV loading
// ---------------------------------------------------------------------------

/// A decoded pig recording: mono f32 samples plus its native sample rate.
pub struct WavSample {
    pub sample_rate: f32,
    pub data: Vec<f32>,
}

fn read_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn read_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn read_i32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// A forgiving RIFF/WAVE parser: handles 16/24/32-bit PCM and 32-bit float,
/// mono or stereo. Returns mono f32 data, peak-normalized to 0.9.
fn load_wav(path: &Path) -> Option<WavSample> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut format_tag = 0u16;
    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits = 0u16;
    let mut data: Option<&[u8]> = None;

    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = read_u32(&bytes, pos + 4) as usize;
        let body_start = pos + 8;
        let body_end = (body_start + size).min(bytes.len());
        match id {
            b"fmt " => {
                let f = &bytes[body_start..body_end];
                if f.len() >= 16 {
                    format_tag = read_u16(f, 0);
                    channels = read_u16(f, 2);
                    sample_rate = read_u32(f, 4);
                    bits = read_u16(f, 14);
                    // WAVE_FORMAT_EXTENSIBLE: the real format tag lives in the
                    // first two bytes of the sub-format GUID.
                    if format_tag == 0xFFFE && f.len() >= 26 {
                        format_tag = read_u16(f, 24);
                    }
                }
            }
            b"data" => data = Some(&bytes[body_start..body_end]),
            _ => {}
        }
        // Chunks are word-aligned.
        pos = body_start + size + (size & 1);
    }

    let data = data?;
    if channels == 0 || sample_rate == 0 {
        return None;
    }

    let bytes_per_sample = match (format_tag, bits) {
        (1, 16) | (1, 24) | (1, 32) | (3, 32) => bits as usize / 8,
        _ => return None,
    };
    let frame_size = channels as usize * bytes_per_sample;
    if frame_size == 0 {
        return None;
    }

    let frames = data.len() / frame_size;
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let base = i * frame_size;
        let mut acc = 0.0f32;
        for c in 0..channels as usize {
            let o = base + c * bytes_per_sample;
            let v = match (format_tag, bits) {
                (1, 16) => i16::from_le_bytes([data[o], data[o + 1]]) as f32 / 32768.0,
                (1, 24) => {
                    let raw = (data[o] as i32) | ((data[o + 1] as i32) << 8) | ((data[o + 2] as i32) << 16);
                    // Sign-extend from 24 to 32 bits.
                    let raw = (raw << 8) >> 8;
                    raw as f32 / 8388608.0
                }
                (1, 32) => read_i32(data, o) as f32 / 2147483648.0,
                (3, 32) => {
                    f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
                }
                _ => 0.0,
            };
            acc += v;
        }
        mono.push(acc / channels as f32);
    }

    // Peak-normalize so quiet and loud recordings play at comparable levels.
    let peak = mono.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    if peak > 0.0001 {
        let k = 0.9 / peak;
        for s in &mut mono {
            *s *= k;
        }
    }

    Some(WavSample {
        sample_rate: sample_rate as f32,
        data: mono,
    })
}

/// Where pig recordings may live: an `oinks` folder next to the plugin, or
/// `%USERPROFILE%\Music\HogSpasm` for easy user drop-ins.
fn load_oink_samples() -> Vec<WavSample> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    unsafe {
        if let Some(dir) = crate::hogs::module_dir() {
            dirs.push(dir.join("oinks"));
        }
    }
    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        dirs.push(PathBuf::from(userprofile).join("Music").join("HogSpasm"));
    }

    for dir in dirs {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        let mut samples = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ext != "wav" {
                continue;
            }
            if let Some(sample) = load_wav(&path) {
                // Anything longer than 8 seconds is a speech, not an oink.
                if sample.data.len() > 16
                    && sample.data.len() as f32 / sample.sample_rate <= 8.0
                {
                    samples.push(sample);
                }
            }
        }
        if !samples.is_empty() {
            return samples;
        }
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// Filters (fallback synth voice only)
// ---------------------------------------------------------------------------

/// A minimal RBJ bandpass biquad filter.
#[derive(Default)]
struct Biquad {
    b0: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn set_bandpass(&mut self, sample_rate: f32, freq: f32, q: f32) {
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let cos_w0 = w0.cos();
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        self.b0 = alpha / a0;
        self.b2 = -alpha / a0;
        self.a1 = -2.0 * cos_w0 / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

// ---------------------------------------------------------------------------
// The oinker
// ---------------------------------------------------------------------------

/// One slot in the sample-playing voice pool.
struct SampleVoice {
    active: bool,
    sample_idx: usize,
    pos: f32,
    ratio: f32,
    gain: f32,
}

impl Default for SampleVoice {
    fn default() -> Self {
        Self {
            active: false,
            sample_idx: 0,
            pos: 0.0,
            ratio: 1.0,
            gain: 0.0,
        }
    }
}

const NUM_VOICES: usize = 4;

/// One persistent hog voice.
pub struct OinkState {
    sample_rate: f32,
    rng: StdRng,

    /// Real pig recordings (empty until `load_samples()` runs).
    samples: Vec<WavSample>,
    voices: [SampleVoice; NUM_VOICES],

    /// Fast-attack envelope follower of the input signal.
    env: f32,
    env_release: f32,

    retrigger_cooldown: i32,

    // Fallback synth voice (only used when no WAV files were found).
    synth_active: bool,
    phase: f32,
    oink_total: i32,
    oink_samples_left: i32,
    oink_base_freq: f32,
    noise_state: u64,
    formant1: Biquad,
    formant2: Biquad,
}

impl OinkState {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100.0,
            rng: StdRng::from_entropy(),
            samples: Vec::new(),
            voices: Default::default(),
            env: 0.0,
            env_release: 0.999,
            retrigger_cooldown: 0,
            synth_active: false,
            phase: 0.0,
            oink_total: 1,
            oink_samples_left: 0,
            oink_base_freq: 120.0,
            noise_state: 0x015A_4E35,
            formant1: Biquad::default(),
            formant2: Biquad::default(),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate.max(8000.0);
        // ~110 ms release on the envelope follower.
        self.env_release = (-1.0 / (0.11 * self.sample_rate)).exp();
    }

    /// Loads real pig recordings. Called from `initialize()`, never from the
    /// audio thread.
    pub fn load_samples(&mut self) {
        if self.samples.is_empty() {
            self.samples = load_oink_samples();
        }
    }

    pub fn reset(&mut self) {
        self.env = 0.0;
        self.retrigger_cooldown = 0;
        for voice in &mut self.voices {
            voice.active = false;
        }
        self.synth_active = false;
        self.formant1.reset();
        self.formant2.reset();
    }

    /// Updates the input envelope follower and returns the new envelope value.
    pub fn update_env(&mut self, input: f32) -> f32 {
        if input > self.env {
            self.env = input;
        } else {
            self.env *= self.env_release;
        }
        self.env
    }

    /// Called once per frame with the instantaneous input level. Triggers a new
    /// oink when the input wakes up — or keeps oinking while it stays loud, so
    /// pads and sustained notes turn into continuous oinking.
    pub fn update_trigger(&mut self, input_abs: f32) {
        if self.retrigger_cooldown > 0 {
            self.retrigger_cooldown -= 1;
            return;
        }

        const THRESHOLD: f32 = 0.015;
        if !self.samples.is_empty() {
            let any_active = self.voices.iter().any(|v| v.active);
            if !any_active && input_abs > THRESHOLD {
                self.start_sample_oink(input_abs);
            } else if any_active && input_abs > THRESHOLD && self.near_end() {
                // Sustained input: chain straight into the next oink as this
                // one peters out.
                self.start_sample_oink(input_abs);
            }
        } else {
            let synth_active = self.synth_active;
            let near_end = synth_active
                && self.oink_samples_left < (self.sample_rate * 0.02) as i32;
            if !synth_active && input_abs > THRESHOLD {
                self.start_synth_oink();
            } else if synth_active && near_end && input_abs > THRESHOLD {
                self.start_synth_oink();
            }
        }
    }

    fn near_end(&self) -> bool {
        self.voices.iter().any(|v| {
            if !v.active {
                return false;
            }
            match self.samples.get(v.sample_idx) {
                Some(s) => (s.data.len() as f32 - v.pos) < self.sample_rate * 0.04,
                None => true,
            }
        })
    }

    fn start_sample_oink(&mut self, env: f32) {
        if self.samples.is_empty() {
            return;
        }

        // Prefer a free voice; otherwise steal the one closest to finishing.
        let idx = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => {
                let mut best = 0usize;
                let mut best_progress = -1.0f32;
                for (i, v) in self.voices.iter().enumerate() {
                    let len = self
                        .samples
                        .get(v.sample_idx)
                        .map(|s| s.data.len())
                        .unwrap_or(1) as f32;
                    let progress = v.pos / len;
                    if progress > best_progress {
                        best_progress = progress;
                        best = i;
                    }
                }
                best
            }
        };

        let sample_idx = self.rng.gen_range(0..self.samples.len());
        let ratio = self.samples[sample_idx].sample_rate / self.sample_rate
            * self.rng.gen_range(0.8..1.3);
        let gain = (0.45 + env * 2.5).clamp(0.45, 1.1);

        let voice = &mut self.voices[idx];
        voice.active = true;
        voice.sample_idx = sample_idx;
        voice.pos = 0.0;
        voice.ratio = ratio;
        voice.gain = gain;

        self.retrigger_cooldown = (self.sample_rate * self.rng.gen_range(0.02..0.06)) as i32;
    }

    fn start_synth_oink(&mut self) {
        self.synth_active = true;
        self.phase = 0.0;
        self.oink_total = (self.sample_rate * self.rng.gen_range(0.14..0.38)) as i32;
        self.oink_samples_left = self.oink_total;
        self.oink_base_freq = self.rng.gen_range(70.0..220.0);
        let f1 = self.rng.gen_range(500.0..900.0);
        let f2 = self.rng.gen_range(1100.0..2000.0);
        self.formant1
            .set_bandpass(self.sample_rate, f1, self.rng.gen_range(3.0..7.0));
        self.formant2
            .set_bandpass(self.sample_rate, f2, self.rng.gen_range(4.0..9.0));
        self.formant1.reset();
        self.formant2.reset();
        self.retrigger_cooldown = (self.sample_rate * self.rng.gen_range(0.04..0.10)) as i32;
    }

    /// Generates the next frame of the current oinkage. Silence when the hog
    /// is between oinks (and the input is quiet).
    pub fn next(&mut self, env: f32) -> f32 {
        if !self.samples.is_empty() {
            return self.next_samples(env);
        }
        self.next_synth(env)
    }

    fn next_samples(&mut self, _env: f32) -> f32 {
        let mut out = 0.0f32;
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let Some(sample) = self.samples.get(voice.sample_idx) else {
                voice.active = false;
                continue;
            };
            let len = sample.data.len();
            let i = voice.pos as usize;
            if i + 1 >= len {
                voice.active = false;
                continue;
            }
            let frac = voice.pos - i as f32;
            let s = sample.data[i] + (sample.data[i + 1] - sample.data[i]) * frac;
            out += s * voice.gain;

            voice.pos += voice.ratio;
            if voice.pos as usize >= len - 1 {
                voice.active = false;
            }
        }

        out.clamp(-1.0, 1.0)
    }

    fn next_synth(&mut self, env: f32) -> f32 {
        if !self.synth_active {
            return 0.0;
        }

        let t = 1.0 - self.oink_samples_left as f32 / self.oink_total as f32;

        // Pitch contour: quick rise, then a droopy fall. "oi-ink."
        let bump = (-((t - 0.3) * (t - 0.3)) / 0.045).exp();
        let freq = self.oink_base_freq * (0.75 + 1.05 * bump);

        self.phase += freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let saw = self.phase * 2.0 - 1.0;

        // Cheap LCG noise for extra snort.
        self.noise_state = self
            .noise_state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let noise = (self.noise_state >> 56) as f32 / 255.0 * 2.0 - 1.0;

        // Fast attack, relaxed release.
        let amp = (t * 9.0).min(1.0).min((1.0 - t) * 2.2);

        // Louder input = more impassioned grunting.
        let drive = 0.4 + (env * 8.0).min(1.4);

        let exciter = saw * 0.8 + noise * 0.3;
        let grunt = self.formant1.process(exciter) * 1.25 + self.formant2.process(exciter) * 0.65;
        let out = (grunt * drive * amp).tanh();

        self.oink_samples_left -= 1;
        if self.oink_samples_left <= 0 {
            self.synth_active = false;
        }

        out
    }
}
