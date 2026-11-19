//! Spike: the v7 shell (design system "ScriptFX Shell", skin Matte + Paper chat) in iced.
//! The plugin itself is a passthrough with a peak meter that drives the stage shader.

use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceSettings, application, create_iced_editor,
    iced::PollSubNotifier,
};
use std::sync::{Arc, atomic::Ordering};

mod shell;

const WINDOW: LogicalSize<f32> = LogicalSize::new(1040.0, 640.0);

#[derive(Params, Default)]
pub struct SpikeParams {}

pub struct ShellSpike {
    params: Arc<SpikeParams>,
    editor_state: Arc<IcedEditorState>,
    /// Output peak (linear), read by the stage shader every frame.
    level: Arc<AtomicF32>,
    notifier: PollSubNotifier,
}

impl Default for ShellSpike {
    fn default() -> Self {
        Self {
            params: Arc::new(SpikeParams::default()),
            editor_state: IcedEditorState::from_size(WINDOW, 1.0),
            level: Arc::new(AtomicF32::new(0.0)),
            notifier: PollSubNotifier::new(),
        }
    }
}

impl Plugin for ShellSpike {
    const NAME: &'static str = "ScriptFX Shell Spike";
    const VENDOR: &'static str = "ScriptFX";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    type Editor = IcedEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        let level = self.level.clone();
        create_iced_editor(
            self.editor_state.clone(),
            level,
            self.notifier.clone(),
            // always_redraw: the stage shader animates every frame (time is read on the render
            // side). Needs a throttling present mode, see stage.rs.
            IcedNiceSettings::new().with_always_redraw(true),
            |persistent, ctx| {
                let mut app = application(
                    persistent,
                    ctx,
                    shell::Shell::new,
                    shell::Shell::update,
                    shell::Shell::view,
                )
                .theme(shell::Shell::theme)
                .style(shell::Shell::style)
                .subscription(shell::Shell::subscription)
                .default_font(shell::theme::font::SANS);
                for font in shell::theme::font::FILES {
                    app = app.font(*font);
                }
                Ok(app.run())
            },
        )
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut peak = 0.0f32;
        for frame in buffer.iter_samples() {
            for s in frame {
                peak = peak.max(s.abs());
            }
        }
        if self.editor_state.is_open() {
            let old = self.level.load(Ordering::Relaxed);
            let new = if peak > old { peak } else { old * 0.92 + peak * 0.08 };
            self.level.store(new, Ordering::Relaxed);
            self.notifier.notify();
        }
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ShellSpike {
    const CLAP_ID: &'static str = "dev.scriptfx.shell-spike";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Shell UI spike (iced)");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::AudioEffect, ClapFeature::Stereo];
}

impl Vst3Plugin for ShellSpike {
    const VST3_CLASS_ID: [u8; 16] = *b"ScriptFXShellSpk";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[Vst3SubCategory::Fx];
}

nice_export_clap!(ShellSpike);
nice_export_vst3!(ShellSpike);
