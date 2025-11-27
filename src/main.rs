mod controllers;
mod mixer;
mod modifiers;
mod oscillatorbank;
mod output;
mod vco;

use std::sync::{Arc, Mutex};

use controllers::KeyboardController;
use macroquad::{prelude::*, text::measure_text};
use modifiers::compute_spectrum;
use oscillatorbank::OscillatorBank;
use output::{AudioEngine, DebugData, SharedPipeline, SynthPipeline};
use tokio::runtime::Runtime;
use vco::{spawn_vco, VcoCommand, Waveform, VcoHandle, voltage_to_frequency};

const SCREEN_WIDTH: f32 = 1280.0;
const SCREEN_HEIGHT: f32 = 720.0;
const PANEL_HEIGHT: f32 = 360.0;

const AMBER: Color = Color {
    r: 0.98,
    g: 0.66,
    b: 0.12,
    a: 1.0,
};
const AMBER_DIM: Color = Color {
    r: 0.78,
    g: 0.52,
    b: 0.08,
    a: 0.4,
};
const BACKGROUND: Color = Color {
    r: 0.02,
    g: 0.02,
    b: 0.02,
    a: 1.0,
};
const PANEL_BROWN: Color = Color {
    r: 0.12,
    g: 0.08,
    b: 0.05,
    a: 1.0,
};
const DETUNE_RANGE: f32 = 0.5;
const FILTER_MIN_HZ: f32 = 200.0;
const FILTER_MAX_HZ: f32 = 5_000.0;
const WAVEFORMS: [Waveform; 4] = [
    Waveform::Saw,
    Waveform::Pulse,
    Waveform::Triangle,
    Waveform::Sine,
];

#[macroquad::main(window_conf)]
async fn main() {
    let runtime = Runtime::new().expect("tokio runtime");
    let vcos: Vec<VcoHandle> = (0..3).map(|_| spawn_vco(&runtime)).collect();

    let states = vcos.iter().map(|(state, _)| state.clone()).collect();
    let bank = OscillatorBank::new(states);
    let mixer = mixer::Mixer::new();
    let modifiers = modifiers::Modifiers::new();
    let pipeline = Arc::new(Mutex::new(SynthPipeline::new(bank, mixer, modifiers)));
    let debug_data = Arc::new(Mutex::new(DebugData::new(1024)));
    let _audio = AudioEngine::start(pipeline.clone(), debug_data.clone())
        .expect("audio output stream");

    let mut controller = KeyboardController::new();
    let mut panel_state = PanelState::new();
    let mut debug_window = DebugWindowState::new();
    sync_audio_from_panel(&panel_state, &vcos, &pipeline);

    let panel_texture = load_texture("assets/synth-ui-style.png")
        .await
        .expect("synth texture");
    panel_texture.set_filter(FilterMode::Linear);

    let mut waveform_cache = Vec::new();
    let mut spectrum_cache = Vec::new();

    loop {
        let layout = compute_panel_layout();
        if let Some(message) = controller.poll() {
            panel_state.last_midi = message.midi_note;
            panel_state.last_voltage = message.voltage;
            for (_, tx) in vcos.iter() {
                let _ = tx.send(VcoCommand::SetVoltage(message.voltage));
            }
            if let Ok(mut synth) = pipeline.lock() {
                synth.set_gate(message.gate);
            }
        }

        let mouse = mouse_position_vec();
        handle_debug_toggle(&mut debug_window, mouse);

        {
            let snapshot = {
                let guard = debug_data.lock().expect("debug lock");
                guard.snapshot()
            };
            if !snapshot.is_empty() {
                waveform_cache = snapshot;
                spectrum_cache = compute_spectrum(&waveform_cache);
            }
        }

        draw_scene(
            &panel_texture,
            &mut panel_state,
            &controller,
            &layout,
            &waveform_cache,
            &spectrum_cache,
            &debug_window,
        );

        sync_audio_from_panel(&panel_state, &vcos, &pipeline);
        feed_stub_knobs(&panel_state);

        next_frame().await;
    }
}

fn window_conf() -> Conf {
    Conf {
        window_title: "MiniRoog Model R".into(),
        fullscreen: false,
        sample_count: 1,
        window_width: SCREEN_WIDTH as i32,
        window_height: SCREEN_HEIGHT as i32,
        high_dpi: false,
        ..Default::default()
    }
}

#[derive(Clone)]
struct PanelLayout {
    controller_rect: Rect,
    oscillator_rect: Rect,
    mixer_rect: Rect,
    modifier_rect: Rect,
    output_rect: Rect,
    controller_knobs: [Rect; 3],
    osc_range_knob: Rect,
    osc_freq_knobs: [Rect; 3],
    osc_wave_knobs: [Rect; 3],
    mixer_knobs: [Rect; 5],
    filter_knobs: [Rect; 3],
    filter_env_knobs: [Rect; 3],
    loudness_knobs: [Rect; 3],
    output_knobs: [Rect; 2],
}

fn compute_panel_layout() -> PanelLayout {
    let margin = 36.0;
    let gap = 18.0;
    let usable_width = SCREEN_WIDTH - margin * 2.0 - gap * 4.0;
    let section_width = usable_width / 5.0;
    let section_height = PANEL_HEIGHT - 80.0;
    let top = 40.0;
    let knob_size = 70.0;

    let controller_rect = Rect::new(margin, top, section_width, section_height);
    let oscillator_rect =
        Rect::new(controller_rect.x + section_width + gap, top, section_width, section_height);
    let mixer_rect = Rect::new(
        oscillator_rect.x + section_width + gap,
        top,
        section_width,
        section_height,
    );
    let modifier_rect = Rect::new(
        mixer_rect.x + section_width + gap,
        top,
        section_width,
        section_height,
    );
    let output_rect = Rect::new(
        modifier_rect.x + section_width + gap,
        top,
        section_width,
        section_height,
    );

    let controller_knobs = [
        Rect::new(
            controller_rect.x + 20.0,
            controller_rect.y + 40.0,
            knob_size,
            knob_size,
        ),
        Rect::new(
            controller_rect.x + controller_rect.w - knob_size - 20.0,
            controller_rect.y + 40.0,
            knob_size,
            knob_size,
        ),
        Rect::new(
            controller_rect.x + controller_rect.w * 0.5 - knob_size * 0.5,
            controller_rect.y + controller_rect.h - knob_size - 30.0,
            knob_size,
            knob_size,
        ),
    ];

    let osc_range_knob =
        Rect::new(oscillator_rect.x + 20.0, oscillator_rect.y + 20.0, knob_size, knob_size);

    let mut osc_freq_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut osc_wave_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for index in 0..3 {
        let y = oscillator_rect.y + 120.0 + index as f32 * 90.0;
        osc_freq_knobs[index] = Rect::new(oscillator_rect.x + 20.0, y, knob_size, knob_size);
        osc_wave_knobs[index] = Rect::new(
            oscillator_rect.x + oscillator_rect.w - knob_size - 20.0,
            y,
            knob_size,
            knob_size,
        );
    }

    let mut mixer_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 5];
    for index in 0..5 {
        let y = mixer_rect.y + 30.0 + index as f32 * 60.0;
        mixer_knobs[index] = Rect::new(
            mixer_rect.x + mixer_rect.w * 0.5 - knob_size * 0.5,
            y,
            knob_size,
            knob_size,
        );
    }

    let mut filter_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut filter_env_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut loudness_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for index in 0..3 {
        let x = modifier_rect.x + 20.0 + index as f32 * (knob_size + 18.0);
        filter_knobs[index] = Rect::new(x, modifier_rect.y + 20.0, knob_size, knob_size);
        filter_env_knobs[index] =
            Rect::new(x, modifier_rect.y + 120.0, knob_size, knob_size);
        loudness_knobs[index] =
            Rect::new(x, modifier_rect.y + 220.0, knob_size, knob_size);
    }

    let output_knobs = [
        Rect::new(output_rect.x + output_rect.w * 0.5 - knob_size * 0.5, output_rect.y + 40.0, knob_size, knob_size),
        Rect::new(
            output_rect.x + output_rect.w * 0.5 - knob_size * 0.5,
            output_rect.y + 180.0,
            knob_size,
            knob_size,
        ),
    ];

    PanelLayout {
        controller_rect,
        oscillator_rect,
        mixer_rect,
        modifier_rect,
        output_rect,
        controller_knobs,
        osc_range_knob,
        osc_freq_knobs,
        osc_wave_knobs,
        mixer_knobs,
        filter_knobs,
        filter_env_knobs,
        loudness_knobs,
        output_knobs,
    }
}

#[derive(Clone)]
struct PanelState {
    controllers: ControllerKnobs,
    oscillator: OscillatorKnobs,
    mixer_panel: MixerKnobs,
    modifiers_panel: ModifierKnobs,
    output_panel: OutputKnobs,
    last_midi: i32,
    last_voltage: f32,
    active_knob: Option<KnobId>,
    knob_origin_value: f32,
    knob_origin_y: f32,
}

impl PanelState {
    fn new() -> Self {
        Self {
            controllers: ControllerKnobs::new(),
            oscillator: OscillatorKnobs::new(),
            mixer_panel: MixerKnobs::new(),
            modifiers_panel: ModifierKnobs::new(),
            output_panel: OutputKnobs::new(),
            last_midi: -1,
            last_voltage: 0.0,
            active_knob: None,
            knob_origin_value: 0.0,
            knob_origin_y: 0.0,
        }
    }

    fn oscillator_mix_levels(&self) -> [f32; 3] {
        [
            self.mixer_panel.osc[0].value,
            self.mixer_panel.osc[1].value,
            self.mixer_panel.osc[2].value,
        ]
    }

    fn cutoff_hz(&self) -> f32 {
        FILTER_MIN_HZ
            + self.modifiers_panel.filter[0].value * (FILTER_MAX_HZ - FILTER_MIN_HZ)
    }

    fn master_level(&self) -> f32 {
        self.output_panel.main_volume.value
    }

    fn osc_detune(&self, index: usize) -> f32 {
        let value = self.oscillator.freq[index].value;
        (value * 2.0 - 1.0) * DETUNE_RANGE
    }
}

struct DebugWindowState {
    open: bool,
    rect: Rect,
}

impl DebugWindowState {
    fn new() -> Self {
        Self {
            open: true,
            rect: Rect::new(20.0, 20.0, 400.0, 400.0),
        }
    }
}

#[derive(Clone)]
struct KnobValue {
    value: f32,
    implemented: bool,
}

impl KnobValue {
    fn implemented(value: f32) -> Self {
        Self {
            value,
            implemented: true,
        }
    }

    fn stub(value: f32) -> Self {
        Self {
            value,
            implemented: false,
        }
    }
}

#[derive(Clone)]
struct ControllerKnobs {
    tune: KnobValue,
    glide: KnobValue,
    modulation_mix: KnobValue,
}

impl ControllerKnobs {
    fn new() -> Self {
        Self {
            tune: KnobValue::stub(0.5),
            glide: KnobValue::stub(0.3),
            modulation_mix: KnobValue::stub(0.5),
        }
    }
}

#[derive(Clone)]
struct OscillatorKnobs {
    range: KnobValue,
    freq: [KnobValue; 3],
    waveform: [KnobValue; 3],
}

impl OscillatorKnobs {
    fn new() -> Self {
        Self {
            range: KnobValue::stub(0.5),
            freq: [
                KnobValue::implemented(0.5),
                KnobValue::implemented(detune_to_value(0.03)),
                KnobValue::implemented(detune_to_value(-0.02)),
            ],
            waveform: [
                KnobValue::implemented(waveform_to_value(Waveform::Saw)),
                KnobValue::implemented(waveform_to_value(Waveform::Saw)),
                KnobValue::implemented(waveform_to_value(Waveform::Saw)),
            ],
        }
    }
}

#[derive(Clone)]
struct MixerKnobs {
    external_input: KnobValue,
    osc: [KnobValue; 3],
    noise: KnobValue,
}

impl MixerKnobs {
    fn new() -> Self {
        Self {
            external_input: KnobValue::stub(0.0),
            osc: [
                KnobValue::implemented(0.85),
                KnobValue::implemented(0.7),
                KnobValue::implemented(0.55),
            ],
            noise: KnobValue::stub(0.0),
        }
    }
}

#[derive(Clone)]
struct ModifierKnobs {
    filter: [KnobValue; 3],
    filter_env: [KnobValue; 3],
    loudness_env: [KnobValue; 3],
}

impl ModifierKnobs {
    fn new() -> Self {
        Self {
            filter: [
                KnobValue::implemented((2200.0 - FILTER_MIN_HZ) / (FILTER_MAX_HZ - FILTER_MIN_HZ)),
                KnobValue::stub(0.4),
                KnobValue::stub(0.5),
            ],
            filter_env: [
                KnobValue::stub(0.2),
                KnobValue::stub(0.5),
                KnobValue::stub(0.5),
            ],
            loudness_env: [
                KnobValue::stub(0.2),
                KnobValue::stub(0.5),
                KnobValue::stub(0.5),
            ],
        }
    }
}

#[derive(Clone)]
struct OutputKnobs {
    main_volume: KnobValue,
    phones_volume: KnobValue,
}

impl OutputKnobs {
    fn new() -> Self {
        Self {
            main_volume: KnobValue::implemented(0.7),
            phones_volume: KnobValue::stub(0.7),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
enum KnobId {
    ControllersTune,
    ControllersGlide,
    ControllersModMix,
    OscRange,
    OscFreq1,
    OscFreq2,
    OscFreq3,
    OscWave1,
    OscWave2,
    OscWave3,
    MixerExternal,
    MixerOsc1,
    MixerOsc2,
    MixerOsc3,
    MixerNoise,
    FilterCutoff,
    FilterEmphasis,
    FilterContour,
    FilterAttack,
    FilterDecay,
    FilterSustain,
    LoudnessAttack,
    LoudnessDecay,
    LoudnessSustain,
    OutputVolume,
    OutputPhones,
}

fn detune_to_value(detune: f32) -> f32 {
    ((detune / DETUNE_RANGE) + 1.0) * 0.5
}

fn mouse_position_vec() -> Vec2 {
    let (x, y) = mouse_position();
    vec2(x, y)
}

fn handle_debug_toggle(state: &mut DebugWindowState, mouse: Vec2) {
    let button_rect = Rect::new(SCREEN_WIDTH - 170.0, PANEL_HEIGHT + 25.0, 140.0, 36.0);
    if state.open {
        let close_rect = Rect::new(
            state.rect.x + state.rect.w - 32.0,
            state.rect.y + 8.0,
            24.0,
            24.0,
        );
        if close_rect.contains(mouse) && is_mouse_button_pressed(MouseButton::Left) {
            state.open = false;
        }
    } else if button_rect.contains(mouse) && is_mouse_button_pressed(MouseButton::Left) {
        state.open = true;
    }
}

fn draw_scene(
    texture: &Texture2D,
    panel_state: &mut PanelState,
    controller: &KeyboardController,
    layout: &PanelLayout,
    waveform: &[f32],
    spectrum: &[f32],
    debug_window: &DebugWindowState,
) {
    clear_background(BACKGROUND);
    draw_texture_ex(
        texture,
        0.0,
        0.0,
        Color::new(1.0, 1.0, 1.0, 0.6),
        DrawTextureParams {
            dest_size: Some(vec2(SCREEN_WIDTH, PANEL_HEIGHT)),
            source: Some(Rect::new(0.0, 0.0, texture.width(), texture.height())),
            ..Default::default()
        },
    );

    draw_section(&layout.controller_rect, "CONTROLLERS");
    draw_section(&layout.oscillator_rect, "OSCILLATOR BANK");
    draw_section(&layout.mixer_rect, "MIXER");
    draw_section(&layout.modifier_rect, "MODIFIERS");
    draw_section(&layout.output_rect, "OUTPUT");

    draw_controllers_panel(panel_state, layout);
    draw_oscillators(panel_state, layout);
    draw_mixer(panel_state, layout);
    draw_modifiers(panel_state, layout);
    draw_output_panel(panel_state, layout);
    draw_keyboard(controller);
    draw_debug_button(debug_window);
    if debug_window.open {
        draw_debug_window(debug_window, waveform, spectrum);
    }
}

fn draw_section(rect: &Rect, label: &str) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.05, 0.03, 0.02, 0.65));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
    let text = label.to_string();
    draw_text_ex(
        &text,
        rect.x + 6.0,
        rect.y - 6.0,
        TextParams {
            font_size: 18,
            color: AMBER,
            ..Default::default()
        },
    );
}

fn draw_controllers_panel(panel_state: &mut PanelState, layout: &PanelLayout) {
    draw_knob_widget(
        panel_state,
        KnobId::ControllersTune,
        layout.controller_knobs[0],
        &mut panel_state.controllers.tune,
        "TUNE",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::ControllersGlide,
        layout.controller_knobs[1],
        &mut panel_state.controllers.glide,
        "GLIDE",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::ControllersModMix,
        layout.controller_knobs[2],
        &mut panel_state.controllers.modulation_mix,
        "MOD MIX",
        None,
    );
    draw_controller_info(panel_state, &layout.controller_rect);
}

fn draw_controller_info(panel_state: &PanelState, rect: &Rect) {
    draw_text_block(
        rect.x + 16.0,
        rect.y + 40.0,
        &format!(
            "GATE {}\nLAST NOTE {}\nVOLTAGE {:.2} V\nFREQUENCY {:.1} Hz",
            if panel_state.last_midi >= 0 { "OPEN" } else { "IDLE" },
            if panel_state.last_midi >= 0 {
                panel_state.last_midi.to_string()
            } else {
                "-".into()
            },
            panel_state.last_voltage,
            voltage_to_frequency(panel_state.last_voltage)
        ),
    );
    draw_text_block(
        rect.x + 16.0,
        rect.y + rect.h - 60.0,
        "MONOPHONIC\nPOLY READY BUS",
    );
}

fn draw_text_block(x: f32, mut y: f32, text: &str) {
    for line in text.lines() {
        draw_text_ex(
            line,
            x,
            y,
            TextParams {
                font_size: 18,
                color: AMBER,
                ..Default::default()
            },
        );
        y += 22.0;
    }
}

fn draw_oscillators(panel_state: &mut PanelState, layout: &PanelLayout) {
    draw_knob_widget(
        panel_state,
        KnobId::OscRange,
        layout.osc_range_knob,
        &mut panel_state.oscillator.range,
        "RANGE",
        None,
    );
    for index in 0..3 {
        let freq_rect = layout.osc_freq_knobs[index];
        let wave_rect = layout.osc_wave_knobs[index];
        let detune = panel_state.osc_detune(index);
        let freq_label = format!("OSC {} FREQ", index + 1);
        let detune_label = format!("{:+.2} OCT", detune);
        draw_knob_widget(
            panel_state,
            match index {
                0 => KnobId::OscFreq1,
                1 => KnobId::OscFreq2,
                _ => KnobId::OscFreq3,
            },
            freq_rect,
            &mut panel_state.oscillator.freq[index],
            &freq_label,
            Some(&detune_label),
        );
        let waveform = value_to_waveform(panel_state.oscillator.waveform[index].value);
        let wave_label = format!("OSC {} WAVE", index + 1);
        draw_knob_widget(
            panel_state,
            match index {
                0 => KnobId::OscWave1,
                1 => KnobId::OscWave2,
                _ => KnobId::OscWave3,
            },
            wave_rect,
            &mut panel_state.oscillator.waveform[index],
            &wave_label,
            Some(waveform.label()),
        );
    }
}

fn draw_mixer(panel_state: &mut PanelState, layout: &PanelLayout) {
    let external = format!("{:.0}%", panel_state.mixer_panel.external_input.value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::MixerExternal,
        layout.mixer_knobs[0],
        &mut panel_state.mixer_panel.external_input,
        "EXT INPUT",
        Some(&external),
    );
    let osc1 = format!("{:.0}%", panel_state.mixer_panel.osc[0].value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::MixerOsc1,
        layout.mixer_knobs[1],
        &mut panel_state.mixer_panel.osc[0],
        "OSC 1",
        Some(&osc1),
    );
    let osc2 = format!("{:.0}%", panel_state.mixer_panel.osc[1].value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::MixerOsc2,
        layout.mixer_knobs[2],
        &mut panel_state.mixer_panel.osc[1],
        "OSC 2",
        Some(&osc2),
    );
    let osc3 = format!("{:.0}%", panel_state.mixer_panel.osc[2].value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::MixerOsc3,
        layout.mixer_knobs[3],
        &mut panel_state.mixer_panel.osc[2],
        "OSC 3",
        Some(&osc3),
    );
    let noise = format!("{:.0}%", panel_state.mixer_panel.noise.value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::MixerNoise,
        layout.mixer_knobs[4],
        &mut panel_state.mixer_panel.noise,
        "NOISE",
        Some(&noise),
    );
}

fn draw_modifiers(panel_state: &mut PanelState, layout: &PanelLayout) {
    let cutoff_text = format!("{:.0} Hz", panel_state.cutoff_hz());
    draw_knob_widget(
        panel_state,
        KnobId::FilterCutoff,
        layout.filter_knobs[0],
        &mut panel_state.modifiers_panel.filter[0],
        "CUTOFF",
        Some(&cutoff_text),
    );
    draw_knob_widget(
        panel_state,
        KnobId::FilterEmphasis,
        layout.filter_knobs[1],
        &mut panel_state.modifiers_panel.filter[1],
        "EMPHASIS",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::FilterContour,
        layout.filter_knobs[2],
        &mut panel_state.modifiers_panel.filter[2],
        "AMT CONTOUR",
        None,
    );

    draw_knob_widget(
        panel_state,
        KnobId::FilterAttack,
        layout.filter_env_knobs[0],
        &mut panel_state.modifiers_panel.filter_env[0],
        "ATTACK",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::FilterDecay,
        layout.filter_env_knobs[1],
        &mut panel_state.modifiers_panel.filter_env[1],
        "DECAY",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::FilterSustain,
        layout.filter_env_knobs[2],
        &mut panel_state.modifiers_panel.filter_env[2],
        "SUSTAIN",
        None,
    );

    draw_knob_widget(
        panel_state,
        KnobId::LoudnessAttack,
        layout.loudness_knobs[0],
        &mut panel_state.modifiers_panel.loudness_env[0],
        "LOUD ATTACK",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::LoudnessDecay,
        layout.loudness_knobs[1],
        &mut panel_state.modifiers_panel.loudness_env[1],
        "LOUD DECAY",
        None,
    );
    draw_knob_widget(
        panel_state,
        KnobId::LoudnessSustain,
        layout.loudness_knobs[2],
        &mut panel_state.modifiers_panel.loudness_env[2],
        "LOUD SUSTAIN",
        None,
    );
}

fn draw_output_panel(panel_state: &mut PanelState, layout: &PanelLayout) {
    let master = format!("{:.0}%", panel_state.master_level() * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::OutputVolume,
        layout.output_knobs[0],
        &mut panel_state.output_panel.main_volume,
        "MAIN VOL",
        Some(&master),
    );
    let phones = format!("{:.0}%", panel_state.output_panel.phones_volume.value * 100.0);
    draw_knob_widget(
        panel_state,
        KnobId::OutputPhones,
        layout.output_knobs[1],
        &mut panel_state.output_panel.phones_volume,
        "PHONES",
        Some(&phones),
    );
}

fn draw_knob_widget(
    panel_state: &mut PanelState,
    knob_id: KnobId,
    rect: Rect,
    knob: &mut KnobValue,
    label: &str,
    display: Option<&str>,
) {
    handle_knob_drag(panel_state, knob_id, rect, knob);
    let center = vec2(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    let radius = rect.w.min(rect.h) * 0.35;
    draw_circle(
        center.x,
        center.y,
        radius + 6.0,
        Color::new(0.05, 0.03, 0.02, 1.0),
    );
    draw_circle(center.x, center.y, radius, Color::new(0.12, 0.12, 0.12, 1.0));
    draw_circle(center.x, center.y, radius * 0.65, Color::new(0.2, 0.2, 0.2, 1.0));
    draw_circle_lines(center.x, center.y, radius + 6.0, 1.0, AMBER_DIM);
    draw_circle_lines(center.x, center.y, radius, 1.0, Color::new(0.4, 0.4, 0.4, 0.3));
    let start_angle = -150.0f32.to_radians();
    let angle_range = 300.0f32.to_radians();
    let theta = start_angle + knob.value.clamp(0.0, 1.0) * angle_range;
    let pointer = vec2(theta.cos(), theta.sin()) * radius * 0.8;
    draw_line(
        center.x,
        center.y,
        center.x + pointer.x,
        center.y + pointer.y,
        3.0,
        AMBER,
    );
    if !knob.implemented {
        draw_centered_text(
            "!",
            Rect::new(rect.x, rect.y + rect.h * 0.5 - 12.0, rect.w, 24.0),
            30,
        );
    }
    if let Some(text) = display {
        draw_centered_text(text, Rect::new(rect.x, rect.y - 12.0, rect.w, 20.0), 14);
    }
    draw_centered_text(
        label,
        Rect::new(rect.x, rect.y + rect.h + 4.0, rect.w, 18.0),
        16,
    );
}

fn handle_knob_drag(
    panel_state: &mut PanelState,
    knob_id: KnobId,
    rect: Rect,
    knob: &mut KnobValue,
) {
    let mouse = mouse_position_vec();
    if is_mouse_button_pressed(MouseButton::Left) && rect.contains(mouse) {
        panel_state.active_knob = Some(knob_id);
        panel_state.knob_origin_value = knob.value;
        panel_state.knob_origin_y = mouse.y;
    }
    if let Some(active) = panel_state.active_knob {
        if active == knob_id {
            if is_mouse_button_down(MouseButton::Left) {
                let delta = (panel_state.knob_origin_y - mouse.y) * 0.005;
                knob.value = (panel_state.knob_origin_value + delta).clamp(0.0, 1.0);
            } else {
                panel_state.active_knob = None;
            }
        }
    }
    if is_mouse_button_released(MouseButton::Left) && panel_state.active_knob == Some(knob_id) {
        panel_state.active_knob = None;
    }
    let (_x, wheel) = mouse_wheel();
    if rect.contains(mouse) && wheel.abs() > f32::EPSILON {
        knob.value = (knob.value + wheel * 0.03).clamp(0.0, 1.0);
    }
}

fn draw_keyboard(controller: &KeyboardController) {
    let base_rect = Rect::new(40.0, PANEL_HEIGHT + 60.0, SCREEN_WIDTH - 80.0, SCREEN_HEIGHT - PANEL_HEIGHT - 100.0);
    draw_rectangle(base_rect.x, base_rect.y, base_rect.w, base_rect.h, Color::new(0.01, 0.01, 0.01, 0.9));
    draw_rectangle_lines(base_rect.x, base_rect.y, base_rect.w, base_rect.h, 1.0, AMBER);
    let white_width = base_rect.w / controller.white_keys().len() as f32;
    let white_size = vec2(white_width - 10.0, base_rect.h - 20.0);
    for (index, binding) in controller.white_keys().iter().enumerate() {
        let rect = Rect::new(
            base_rect.x + index as f32 * white_width + 5.0,
            base_rect.y + base_rect.h - white_size.y - 5.0,
            white_size.x,
            white_size.y,
        );
        let active = controller.is_pressed(binding.keycode);
        draw_key(rect, active, false, binding.label);
    }
    let black_size = vec2(white_width * 0.8, (base_rect.h - 30.0) * 0.55);
    for binding in controller.black_keys() {
        let x = base_rect.x + binding.position_hint * base_rect.w - black_size.x * 0.5;
        let rect = Rect::new(
            x,
            base_rect.y + 10.0,
            black_size.x,
            black_size.y,
        );
        let active = controller.is_pressed(binding.keycode);
        draw_key(rect, active, true, binding.label);
    }
}

fn draw_key(rect: Rect, active: bool, filled: bool, label: &str) {
    let fill_color = if active {
        Color::new(0.3, 0.2, 0.07, 0.9)
    } else if filled {
        Color::new(0.08, 0.05, 0.03, 0.95)
    } else {
        Color::new(0.02, 0.02, 0.02, 0.95)
    };
    draw_rounded_rect(rect, 6.0, fill_color);
    draw_rounded_rect_lines(rect, 6.0, AMBER);
    draw_centered_text(label, rect, 22);
}

fn draw_rounded_rect(rect: Rect, radius: f32, color: Color) {
    draw_rectangle(rect.x + radius, rect.y, rect.w - 2.0 * radius, rect.h, color);
    draw_rectangle(rect.x, rect.y + radius, rect.w, rect.h - 2.0 * radius, color);
    draw_circle(rect.x + radius, rect.y + radius, radius, color);
    draw_circle(rect.x + rect.w - radius, rect.y + radius, radius, color);
    draw_circle(rect.x + radius, rect.y + rect.h - radius, radius, color);
    draw_circle(
        rect.x + rect.w - radius,
        rect.y + rect.h - radius,
        radius,
        color,
    );
}

fn draw_rounded_rect_lines(rect: Rect, radius: f32, color: Color) {
    draw_line(rect.x + radius, rect.y, rect.x + rect.w - radius, rect.y, 1.0, color);
    draw_line(
        rect.x + radius,
        rect.y + rect.h,
        rect.x + rect.w - radius,
        rect.y + rect.h,
        1.0,
        color,
    );
    draw_line(rect.x, rect.y + radius, rect.x, rect.y + rect.h - radius, 1.0, color);
    draw_line(
        rect.x + rect.w,
        rect.y + radius,
        rect.x + rect.w,
        rect.y + rect.h - radius,
        1.0,
        color,
    );
}

fn draw_centered_text(text: &str, rect: Rect, size: u16) {
    let measure = measure_text(text, None, size, 1.0);
    let x = rect.x + rect.w * 0.5 - measure.width * 0.5;
    let y = rect.y + rect.h * 0.5 + measure.height * 0.5;
    draw_text_ex(
        text,
        x,
        y,
        TextParams {
            font_size: size,
            color: AMBER,
            ..Default::default()
        },
    );
}

fn draw_debug_button(state: &DebugWindowState) {
    let rect = Rect::new(SCREEN_WIDTH - 170.0, PANEL_HEIGHT + 25.0, 140.0, 36.0);
    if state.open {
        draw_text_ex(
            "DEBUG OPEN",
            rect.x,
            rect.y - 6.0,
            TextParams {
                font_size: 18,
                color: AMBER,
                ..Default::default()
            },
        );
    } else {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.05, 0.03, 0.02, 1.0));
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
        draw_centered_text("DEBUGGER", rect, 18);
    }
}

fn draw_debug_window(state: &DebugWindowState, waveform: &[f32], spectrum: &[f32]) {
    let rect = state.rect;
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.02, 0.02, 0.02, 0.95));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
    draw_text_ex(
        "DEBUG SCOPE",
        rect.x + 12.0,
        rect.y + 26.0,
        TextParams {
            font_size: 20,
            color: AMBER,
            ..Default::default()
        },
    );
    draw_rectangle_lines(rect.x + rect.w - 32.0, rect.y + 8.0, 24.0, 24.0, 1.0, AMBER);
    draw_centered_text("X", Rect::new(rect.x + rect.w - 32.0, rect.y + 8.0, 24.0, 24.0), 20);

    let scope_rect = Rect::new(rect.x + 16.0, rect.y + 52.0, rect.w - 32.0, 150.0);
    draw_rectangle_lines(scope_rect.x, scope_rect.y, scope_rect.w, scope_rect.h, 1.0, AMBER);
    draw_waveform(scope_rect, waveform);

    let freq_rect = Rect::new(
        rect.x + 16.0,
        scope_rect.y + scope_rect.h + 20.0,
        rect.w - 32.0,
        rect.h - scope_rect.h - 76.0,
    );
    draw_rectangle_lines(freq_rect.x, freq_rect.y, freq_rect.w, freq_rect.h, 1.0, AMBER);
    draw_frequency(freq_rect, spectrum);
}

fn draw_waveform(rect: Rect, samples: &[f32]) {
    if samples.len() < 2 {
        return;
    }
    for i in 1..samples.len() {
        let x0 = rect.x + (i as f32 - 1.0) / samples.len() as f32 * rect.w;
        let x1 = rect.x + (i as f32) / samples.len() as f32 * rect.w;
        let y0 = rect.y + rect.h * 0.5 - samples[i - 1] * rect.h * 0.45;
        let y1 = rect.y + rect.h * 0.5 - samples[i] * rect.h * 0.45;
        draw_line(x0, y0, x1, y1, 1.0, AMBER);
    }
}

fn draw_frequency(rect: Rect, spectrum: &[f32]) {
    if spectrum.is_empty() {
        return;
    }
    let bins = spectrum.len().min(128);
    for i in 0..bins {
        let value = spectrum[i];
        let x = rect.x + i as f32 / bins as f32 * rect.w;
        let height = value.min(1.0) * rect.h;
        draw_line(
            x,
            rect.y + rect.h,
            x,
            rect.y + rect.h - height,
            2.0,
            AMBER_DIM,
        );
    }
}

fn value_to_waveform(value: f32) -> Waveform {
    let mut index = (value.clamp(0.0, 0.999) * WAVEFORMS.len() as f32) as usize;
    if index >= WAVEFORMS.len() {
        index = WAVEFORMS.len() - 1;
    }
    WAVEFORMS[index]
}

fn waveform_to_value(waveform: Waveform) -> f32 {
    if let Some(index) = WAVEFORMS.iter().position(|w| *w == waveform) {
        (index as f32 + 0.5) / WAVEFORMS.len() as f32
    } else {
        0.5
    }
}

fn sync_audio_from_panel(panel_state: &PanelState, vcos: &[VcoHandle], pipeline: &SharedPipeline) {
    for (index, (_, tx)) in vcos.iter().enumerate() {
        let detune = panel_state.osc_detune(index);
        let waveform = value_to_waveform(panel_state.oscillator.waveform[index].value);
        let _ = tx.send(VcoCommand::SetDetune(detune));
        let _ = tx.send(VcoCommand::SetWaveform(waveform));
    }
    if let Ok(mut synth) = pipeline.lock() {
        for (index, level) in panel_state.oscillator_mix_levels().iter().enumerate() {
            synth.set_mix_level(index, *level);
        }
        synth.set_master_level(panel_state.master_level());
        synth.set_cutoff(panel_state.cutoff_hz());
    }
}

fn feed_stub_knobs(panel_state: &PanelState) {
    stub_controllers_tune(panel_state.controllers.tune.value);
    stub_controllers_glide(panel_state.controllers.glide.value);
    stub_controllers_mod_mix(panel_state.controllers.modulation_mix.value);
    stub_oscillator_range(panel_state.oscillator.range.value);
    stub_external_input_volume(panel_state.mixer_panel.external_input.value);
    stub_noise_volume(panel_state.mixer_panel.noise.value);
    stub_filter_emphasis(panel_state.modifiers_panel.filter[1].value);
    stub_filter_contour_amount(panel_state.modifiers_panel.filter[2].value);
    stub_filter_attack(panel_state.modifiers_panel.filter_env[0].value);
    stub_filter_decay(panel_state.modifiers_panel.filter_env[1].value);
    stub_filter_sustain(panel_state.modifiers_panel.filter_env[2].value);
    stub_loudness_attack(panel_state.modifiers_panel.loudness_env[0].value);
    stub_loudness_decay(panel_state.modifiers_panel.loudness_env[1].value);
    stub_loudness_sustain(panel_state.modifiers_panel.loudness_env[2].value);
    stub_phones_volume(panel_state.output_panel.phones_volume.value);
}

fn stub_controllers_tune(_value: f32) {
    // TODO: Map controller tune knob into base pitch offset.
}

fn stub_controllers_glide(_value: f32) {
    // TODO: Implement glide/portamento smoothing for pitch CV.
}

fn stub_controllers_mod_mix(_value: f32) {
    // TODO: Blend modulation bus and noise when modulation mix knob is ready.
}

fn stub_oscillator_range(_value: f32) {
    // TODO: Switch oscillator range to follow MiniMoog foot settings.
}

fn stub_external_input_volume(_value: f32) {
    // TODO: Mix external input audio stream.
}

fn stub_noise_volume(_value: f32) {
    // TODO: Route noise generator into the mixer.
}

fn stub_filter_emphasis(_value: f32) {
    // TODO: Apply resonance to the filter core.
}

fn stub_filter_contour_amount(_value: f32) {
    // TODO: Apply contour envelope modulation depth.
}

fn stub_filter_attack(_value: f32) {
    // TODO: Add filter envelope attack time handling.
}

fn stub_filter_decay(_value: f32) {
    // TODO: Add filter envelope decay segment.
}

fn stub_filter_sustain(_value: f32) {
    // TODO: Tie filter sustain knob into envelope sustain.
}

fn stub_loudness_attack(_value: f32) {
    // TODO: Extend loudness contour attack handling.
}

fn stub_loudness_decay(_value: f32) {
    // TODO: Extend loudness contour decay handling.
}

fn stub_loudness_sustain(_value: f32) {
    // TODO: Extend loudness contour sustain handling.
}

fn stub_phones_volume(_value: f32) {
    // TODO: Apply dedicated headphones gain stage.
}
