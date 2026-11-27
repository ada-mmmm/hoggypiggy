//! HogSpasm — an FL Studio extension that spasms random popups of hogs.
//!
//! Every sound that goes through the plugin comes back out as an oink, and
//! the plugin keeps a growing herd of permanent hog popups on screen.

use nih_plug::prelude::*;
use nih_plug_iced::IcedState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

mod editor;
mod hogs;
mod oink;
mod theme;

pub struct HogSpasm {
    params: Arc<HogSpasmParams>,
    shared: Arc<hogs::HogShared>,
    spawner_started: AtomicBool,
    oink: oink::OinkState,
}

#[derive(Params)]
pub struct HogSpasmParams {
    /// The editor state, so our lovely control panel's size is restored with the project.
    #[persist = "editor-state"]
    pub editor_state: Arc<IcedState>,

    /// How many hogs per second break loose on average. Automatable — paint an
    /// automation clip and conduct the stampede.
    #[id = "intensity"]
    pub intensity: FloatParam,

    /// How much of the audio gets replaced with oinks. 1.0 = everything is pig now.
    #[id = "oink"]
    pub oinkiness: FloatParam,
}

impl Default for HogSpasm {
    fn default() -> Self {
        Self {
            params: Arc::new(HogSpasmParams::default()),
            shared: hogs::HogShared::new(),
            spawner_started: AtomicBool::new(false),
            oink: oink::OinkState::new(),
        }
    }
}

impl Default for HogSpasmParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),
            intensity: FloatParam::new(
                "Hog Intensity",
                20.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 20.0,
                },
            )
            .with_unit(" hogs/s")
            .with_step_size(0.1)
            .with_value_to_string(formatters::v2s_f32_rounded(1))
            .with_string_to_value(Arc::new(|s: &str| s.trim().parse::<f32>().ok())),
            oinkiness: FloatParam::new(
                "Oinkiness",
                1.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1.0,
                },
            )
            .with_unit("%")
            .with_step_size(0.01)
            .with_value_to_string(formatters::v2s_f32_rounded(2))
            .with_string_to_value(Arc::new(|s: &str| s.trim().parse::<f32>().ok())),
        }
    }
}

impl Plugin for HogSpasm {
    const NAME: &'static str = "HogSpasm";
    const VENDOR: &'static str = "mchog";
    const URL: &'static str = "https://www.image-line.com";
    const EMAIL: &'static str = "oink@hogspasm.local";
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
        // Start the hog spawner exactly once per plugin instance. It lurks for
        // about half a minute, then the spasm begins. Also arm the secret
        // word listener so the user can always type their way out.
        if !self.spawner_started.swap(true, Ordering::SeqCst) {
            let params = Arc::clone(&self.params);
            let shared = Arc::clone(&self.shared);
            std::thread::spawn(move || hogs::spawn_loop(params, shared));
        }
        hogs::register_piggy_target(Arc::clone(&self.shared));
        self.oink.set_sample_rate(buffer_config.sample_rate as f32);
        self.oink.load_samples();
        true
    }

    fn reset(&mut self) {
        self.oink.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let oinkiness = self.params.oinkiness.value();
        if oinkiness < 0.001 {
            // The hog is napping. Pure passthrough.
            return ProcessStatus::Normal;
        }

        for mut channel_samples in buffer.iter_samples() {
            // Mono level of the input frame.
            let mut sum = 0.0f32;
            let mut count = 0usize;
            for sample in channel_samples.iter_mut() {
                sum += *sample;
                count += 1;
            }
            let input = if count > 0 {
                (sum / count as f32).abs()
            } else {
                0.0
            };

            let env = self.oink.update_env(input);
            self.oink.update_trigger(input);
            let oink = self.oink.next(env);

            for sample in channel_samples.iter_mut() {
                *sample = *sample * (1.0 - oinkiness) + oink * oinkiness;
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {
        // If the host deactivates us, round up the livestock.
        hogs::clear_hogs(&self.shared);
    }
}

impl Drop for HogSpasm {
    fn drop(&mut self) {
        // Teardown: popups and the machine go, but the hog decor stays until
        // the secret word is spoken.
        hogs::shutdown(&self.shared);
        hogs::unregister_piggy_target(&self.shared);
    }
}

impl Vst3Plugin for HogSpasm {
    const VST3_CLASS_ID: [u8; 16] = *b"HogSpasmPlugAAAA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_vst3!(HogSpasm);
