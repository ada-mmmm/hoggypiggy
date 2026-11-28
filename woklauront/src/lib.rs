//! WokLauront — hard autotune. T-Pain slide for your vocals.
//!
//! How it works:
//! 1. A downsampled autocorrelation pitch detector estimates the sung F0
//!    every 256 samples (~6 ms, fast enough for the robotic snap).
//! 2. The detected pitch is snapped to the nearest note of the selected
//!    scale (chromatic / major / minor around a chosen root).
//! 3. A two-tap granular pitch shifter (crossfading delay lines) resamples
//!    the voice toward the target note. With the retune speed set low, the
//!    correction is nearly instant — the classic slide.

use nih_plug::prelude::*;
use nih_plug_iced::IcedState;
use std::sync::Arc;

mod editor;

/// Analysis window at the host sample rate.
const ANA_LEN: usize = 1024;
/// Analyze every HOP samples (~6 ms at 44.1 kHz) for the fast snap.
const HOP: usize = 256;
/// Pitch detection runs on a downsampled copy of the window (cheap + fine
/// for F0 estimation).
const DS: usize = 4;
const DS_LEN: usize = ANA_LEN / DS;
const MIN_FREQ: f32 = 70.0;
const MAX_FREQ: f32 = 1000.0;
/// Pitch shifter ring buffer + grain length.
const BUF_LEN: usize = 8192;
const GRAIN: f32 = 2400.0;

#[derive(Enum, Clone, Copy, PartialEq)]
pub enum RootNote {
    #[name = "C"]
    C,
    #[name = "C#"]
    CSharp,
    #[name = "D"]
    D,
    #[name = "D#"]
    DSharp,
    #[name = "E"]
    E,
    #[name = "F"]
    F,
    #[name = "F#"]
    FSharp,
    #[name = "G"]
    G,
    #[name = "G#"]
    GSharp,
    #[name = "A"]
    A,
    #[name = "A#"]
    ASharp,
    #[name = "B"]
    B,
}

#[derive(Enum, Clone, Copy, PartialEq)]
pub enum ScaleKind {
    Chromatic,
    Major,
    Minor,
}

#[derive(Params)]
pub struct WokParams {
    /// The editor state, so the panel's size is restored with the project.
    #[persist = "editor-state"]
    pub editor_state: Arc<IcedState>,

    #[id = "root"]
    pub root: EnumParam<RootNote>,

    #[id = "scale"]
    pub scale: EnumParam<ScaleKind>,

    /// Retune speed in milliseconds. Low = hard robotic T-Pain snap.
    #[id = "retune"]
    pub retune: FloatParam,

    /// Dry/wet blend.
    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for WokParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),
            root: EnumParam::new("Root", RootNote::C),
            scale: EnumParam::new("Scale", ScaleKind::Chromatic),
            retune: FloatParam::new(
                "Retune Speed",
                3.0,
                FloatRange::Linear {
                    min: 1.0,
                    max: 100.0,
                },
            )
            .with_unit(" ms")
            .with_step_size(0.5),
            mix: FloatParam::new(
                "Mix",
                1.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1.0,
                },
            )
            .with_unit("%")
            .with_step_size(0.01),
        }
    }
}

pub struct WokLauront {
    params: Arc<WokParams>,
    sample_rate: f32,

    /// Mono analysis ring (last ANA_LEN samples).
    ana: Vec<f32>,
    ana_pos: usize,
    hop_counter: usize,
    /// Last confident detected F0, or 0 when unvoiced.
    detected_f0: f32,

    /// Normalized autocorrelation scratch space.
    acf: Vec<f32>,
    min_lag: usize,
    max_lag: usize,

    /// Pitch shifter ring + state.
    buf: Vec<f32>,
    w: usize,
    phase: f32,
    ratio: f32,

    /// DC blocker for the shifted signal.
    dc_x1: f32,
    dc_y1: f32,
}

impl Default for WokLauront {
    fn default() -> Self {
        Self {
            params: Arc::new(WokParams::default()),
            sample_rate: 44100.0,
            ana: vec![0.0; ANA_LEN],
            ana_pos: 0,
            hop_counter: 0,
            detected_f0: 0.0,
            acf: Vec::new(),
            min_lag: 4,
            max_lag: 64,
            buf: vec![0.0; BUF_LEN],
            w: 0,
            phase: 0.0,
            ratio: 1.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
        }
    }
}

impl WokLauront {
    /// Snaps a detected frequency to the nearest note of the selected scale.
    fn snap_to_scale(&self, f0: f32) -> f32 {
        let midi = 69.0 + 12.0 * (f0 / 440.0).log2();
        let m = midi.round() as i32;
        let pc = m.rem_euclid(12) as usize;
        let root_pc = self.params.root.value() as usize;
        let scale = self.params.scale.value();

        let allowed = |pc: usize| -> bool {
            let rel = (pc + 12 - root_pc) % 12;
            match scale {
                ScaleKind::Chromatic => true,
                ScaleKind::Major => matches!(rel, 0 | 2 | 4 | 5 | 7 | 9 | 11),
                ScaleKind::Minor => matches!(rel, 0 | 2 | 3 | 5 | 7 | 8 | 10),
            }
        };

        // Find the nearest allowed pitch class, searching outward.
        let mut delta = 0i32;
        'outer: for d in 0..=6i32 {
            if allowed((pc as i32 + d).rem_euclid(12) as usize) {
                delta = d;
                break;
            }
            if allowed((pc as i32 - d).rem_euclid(12) as usize) {
                delta = -d;
                break 'outer;
            }
        }

        let target_midi = m as f32 + delta as f32;
        440.0 * ((target_midi - 69.0) / 12.0).exp2()
    }

    /// Downsampled autocorrelation pitch detection over the analysis ring.
    fn analyze(&mut self) {
        let dsr = self.sample_rate / DS as f32;

        // Downsample by averaging (cheap anti-alias).
        let mut ds = [0.0f32; DS_LEN];
        for k in 0..DS_LEN {
            let base = (self.ana_pos + k * DS) % ANA_LEN;
            let mut acc = 0.0;
            for m in 0..DS {
                acc += self.ana[(base + m) % ANA_LEN];
            }
            ds[k] = acc / DS as f32;
        }

        // RMS gate — stay silent on breaths and consonants.
        let mut energy = 0.0;
        for i in 0..self.max_lag {
            energy += ds[i] * ds[i];
        }
        let rms = (energy / self.max_lag as f32).sqrt();
        if rms < 0.01 {
            self.detected_f0 = 0.0;
            return;
        }

        // Normalized autocorrelation over the allowed lag range.
        let window = DS_LEN - self.max_lag;
        let mut e0 = 0.0f32;
        for i in 0..window {
            e0 += ds[i] * ds[i];
        }
        if e0 <= 1e-9 {
            self.detected_f0 = 0.0;
            return;
        }

        for lag in self.min_lag..=self.max_lag {
            let mut s = 0.0f32;
            for i in 0..window {
                s += ds[i] * ds[i + lag];
            }
            // Normalized autocorrelation: mean product / mean square (~1.0 at
            // zero lag, peaks where the signal repeats).
            self.acf[lag] = (s / window as f32) / (e0 / window as f32);
        }

        // Best peak.
        let (mut best_lag, mut best_v) = (self.min_lag, 0.0f32);
        for lag in self.min_lag..=self.max_lag {
            if self.acf[lag] > best_v {
                best_v = self.acf[lag];
                best_lag = lag;
            }
        }
        if best_v < 0.35 {
            self.detected_f0 = 0.0;
            return;
        }

        // Octave guard: prefer the FIRST local peak that is close to the
        // best value, so sub-harmonics don't drag the pitch down an octave.
        for lag in (self.min_lag + 1)..self.max_lag {
            let is_local_max = self.acf[lag] > self.acf[lag - 1] && self.acf[lag] >= self.acf[lag + 1];
            if is_local_max && self.acf[lag] >= best_v * 0.8 {
                best_lag = lag;
                best_v = self.acf[lag];
                break;
            }
        }

        // Parabolic interpolation for sub-lag precision.
        let v_l = self.acf[best_lag.saturating_sub(1)];
        let v_c = self.acf[best_lag];
        let v_r = self.acf[(best_lag + 1).min(self.max_lag)];
        let denom = v_l - 2.0 * v_c + v_r;
        let offset = if denom.abs() > 1e-9 {
            (0.5 * (v_l - v_r) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        };

        let f0 = dsr / (best_lag as f32 + offset);
        if (MIN_FREQ..=MAX_FREQ).contains(&f0) {
            self.detected_f0 = f0;
        } else {
            self.detected_f0 = 0.0;
        }
    }

    /// Linear-interpolated read from the shifter ring, `delay` samples
    /// behind the write position.
    fn read_lerp(&self, delay: f32) -> f32 {
        let n = self.buf.len();
        let di = delay.floor() as usize;
        let fr = delay - di as f32;
        let i0 = (self.w + n - di) % n;
        let i1 = (i0 + n - 1) % n;
        self.buf[i0] + (self.buf[i1] - self.buf[i0]) * fr
    }
}

impl Plugin for WokLauront {
    const NAME: &'static str = "WokLauront";
    const VENDOR: &'static str = "mchog";
    const URL: &'static str = "https://www.image-line.com";
    const EMAIL: &'static str = "oink@woklauront.local";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f32;
        let dsr = self.sample_rate / DS as f32;
        self.min_lag = (dsr / MAX_FREQ).max(2.0) as usize;
        self.max_lag = ((dsr / MIN_FREQ) as usize).min(DS_LEN / 2);
        self.acf = vec![0.0; self.max_lag + 2];
        self.reset();
        true
    }

    fn reset(&mut self) {
        self.detected_f0 = 0.0;
        self.ratio = 1.0;
        self.phase = 0.0;
        self.buf.iter_mut().for_each(|s| *s = 0.0);
        self.dc_x1 = 0.0;
        self.dc_y1 = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let retune_s = self.params.retune.value().clamp(0.5, 500.0) / 1000.0;
        let mix = self.params.mix.value().clamp(0.0, 1.0);
        let alpha = 1.0 - (-1.0 / (retune_s * self.sample_rate)).exp();

        for mut channel_samples in buffer.iter_samples() {
            // Mono input.
            let mut input = 0.0f32;
            let mut count = 0usize;
            for sample in channel_samples.iter_mut() {
                input += *sample;
                count += 1;
            }
            input /= count.max(1) as f32;

            // Feed the rings.
            self.buf[self.w] = input;
            self.ana[self.ana_pos] = input;

            // Hop-driven pitch analysis.
            self.hop_counter += 1;
            if self.hop_counter >= HOP {
                self.hop_counter = 0;
                self.analyze();
            }

            // Target ratio: snap the detected pitch to the scale.
            let f0 = self.detected_f0;
            let r_target = if f0 > 0.0 {
                (self.snap_to_scale(f0) / f0).clamp(0.5, 2.0)
            } else {
                1.0
            };
            self.ratio += (r_target - self.ratio) * alpha;
            let ratio = self.ratio.clamp(0.25, 4.0);

            // Two-tap granular shifter: the delay for each tap sweeps, and
            // the sweep speed is what changes the pitch.
            self.phase += 1.0 - ratio;
            while self.phase < 0.0 {
                self.phase += GRAIN;
            }
            while self.phase >= GRAIN {
                self.phase -= GRAIN;
            }

            let u1 = self.phase / GRAIN;
            let u2 = (u1 + 0.5).fract();
            let d1 = u1 * GRAIN;
            let d2 = u2 * GRAIN;
            let g1 = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * u1).cos());
            let g2 = 1.0 - g1;

            let mut wet = g1 * self.read_lerp(d1) + g2 * self.read_lerp(d2);

            // DC blocker (interpolation drift).
            let hp = wet - self.dc_x1 + 0.995 * self.dc_y1;
            self.dc_x1 = wet;
            self.dc_y1 = hp;
            wet = hp.clamp(-1.0, 1.0);

            for sample in channel_samples.iter_mut() {
                *sample = input * (1.0 - mix) + wet * mix;
            }

            self.w = (self.w + 1) % self.buf.len();
            self.ana_pos = (self.ana_pos + 1) % self.ana.len();
        }

        ProcessStatus::Normal
    }
}

impl Vst3Plugin for WokLauront {
    const VST3_CLASS_ID: [u8; 16] = *b"WokLaurontPlugAA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nih_export_vst3!(WokLauront);
