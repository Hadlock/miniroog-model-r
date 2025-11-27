mod controllers;
mod mixer;
mod modifiers;
mod noise;
mod oscillatorbank;
mod output;
mod vco;

use std::sync::{Arc, Mutex};

use controllers::KeyboardController;
use macroquad::{prelude::*, text::measure_text};
use modifiers::compute_spectrum;
use noise::{NoiseColor, NoiseGenerator};
use oscillatorbank::OscillatorBank;
use output::{AudioEngine, DebugData, SharedPipeline, SynthPipeline};
use tokio::runtime::Runtime;
use vco::{VcoCommand, VcoHandle, Waveform, spawn_vco, voltage_to_frequency};

const SCREEN_WIDTH: f32 = 1280.0;
const SCREEN_HEIGHT: f32 = 720.0;
const PANEL_HEIGHT: f32 = 360.0;
const KEY_FONT_SIZE: u16 = 35;
const MAX_ANALYZER_FREQ: f32 = 25_000.0;
const MIN_ANALYZER_DB: f32 = -20.0;
const MAX_ANALYZER_DB: f32 = 20.0;
const TUNE_RANGE_OCT: f32 = 1.0;
const GLIDE_MIN_SEC: f32 = 0.0;
const GLIDE_MAX_SEC: f32 = 0.6;
const MOD_LFO_FREQ: f32 = 4.5;
const MOD_DEPTH: f32 = 0.3;
const CONTROLLER_KNOB_SPACING: f32 = 1.2;

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
const DETUNE_RANGE: f32 = 0.5;
const FILTER_MIN_HZ: f32 = 200.0;
const FILTER_MAX_HZ: f32 = 5_000.0;
const WAVEFORMS: [Waveform; 4] = [
    Waveform::Saw,
    Waveform::Pulse,
    Waveform::Triangle,
    Waveform::Sine,
];
const NOISE_COLOR_COUNT: usize = NoiseColor::COUNT;

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
    let _audio =
        AudioEngine::start(pipeline.clone(), debug_data.clone()).expect("audio output stream");

    let mut controller = KeyboardController::new();
    let mut panel_state = PanelState::new();
    let mut knob_drag = KnobDragState::default();
    let mut debug_window = DebugWindowState::new();
    sync_audio_from_panel(&panel_state, &vcos, &pipeline);
    panel_state.refresh_pitch_target();
    panel_state.apply_pitch(0.0, &vcos);

    let panel_texture = load_texture("assets/synth-ui-style.png")
        .await
        .expect("synth texture");
    panel_texture.set_filter(FilterMode::Linear);

    let mut waveform_cache = Vec::new();
    let mut spectrum_cache = Vec::new();

    if let Ok(synth) = pipeline.lock() {
        debug_window.set_sample_rate(synth.sample_rate());
    }

    loop {
        let dt = get_frame_time();
        let layout = compute_panel_layout();
        let keyboard_layout = build_keyboard_layout(&controller);
        let mouse_pos = mouse_position_vec();
        let mouse_changed = controller.handle_mouse_keys(
            keyboard_layout.hit_test(mouse_pos),
            is_mouse_button_pressed(MouseButton::Left),
            is_mouse_button_down(MouseButton::Left),
            is_mouse_button_released(MouseButton::Left),
        );
        if is_key_pressed(KeyCode::Tab) {
            panel_state.mod_noise_color = panel_state.mod_noise_color.next();
        }
        if let Some(message) = controller.poll(mouse_changed) {
            panel_state.last_midi = message.midi_note;
            panel_state.last_voltage = message.voltage;
            if let Ok(mut synth) = pipeline.lock() {
                synth.set_gate(message.gate);
            }
        }

        handle_debug_toggle(&mut debug_window, mouse_pos);
        handle_mixer_switches(&mut panel_state, &layout);
        panel_state.refresh_pitch_target();
        panel_state.update_modulation(dt);
        panel_state.apply_pitch(dt, &vcos);

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
            &mut knob_drag,
            &controller,
            &layout,
            &keyboard_layout,
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
    modifier_loudness_split: f32,
    controller_knobs: [Rect; 3],
    osc_range_knobs: [Rect; 3],
    osc_freq_knobs: [Rect; 3],
    osc_wave_knobs: [Rect; 3],
    mixer_osc_knobs: [Rect; 3],
    mixer_extra_knobs: [Rect; 2],
    mixer_toggle_rects: [Rect; 5],
    noise_selector_rects: [Rect; NOISE_COLOR_COUNT],
    overload_rect: Rect,
    filter_knobs: [Rect; 3],
    filter_env_knobs: [Rect; 3],
    loudness_knobs: [Rect; 3],
    output_knobs: [Rect; 2],
}

fn compute_panel_layout() -> PanelLayout {
    let margin = 36.0;
    let gap = 18.0;
    let usable_width = SCREEN_WIDTH - margin * 2.0 - gap * 4.0;
    let section_height = PANEL_HEIGHT - 80.0;
    let top = 40.0;
    let knob_size = 70.0;

    let width_factors = [0.14, 0.27, 0.22, 0.27, 0.1];
    let mut sections = [0.0; 5];
    for (i, factor) in width_factors.iter().enumerate() {
        sections[i] = usable_width * factor;
    }

    let controller_rect = Rect::new(margin, top, sections[0], section_height);
    let oscillator_rect = Rect::new(
        controller_rect.x + controller_rect.w + gap,
        top,
        sections[1],
        section_height,
    );
    let mixer_rect = Rect::new(
        oscillator_rect.x + oscillator_rect.w + gap,
        top,
        sections[2],
        section_height,
    );
    let modifier_rect = Rect::new(
        mixer_rect.x + mixer_rect.w + gap,
        top,
        sections[3],
        section_height,
    );
    let output_rect = Rect::new(
        modifier_rect.x + modifier_rect.w + gap,
        top,
        sections[4],
        section_height,
    );

    let center_x = controller_rect.x + controller_rect.w * 0.5;
    let center_spacing = knob_size * CONTROLLER_KNOB_SPACING.max(1.1);
    let half_spacing = center_spacing * 0.5;
    let left_center = (center_x - half_spacing).max(controller_rect.x + knob_size * 0.5 + 8.0);
    let right_center = (center_x + half_spacing)
        .min(controller_rect.x + controller_rect.w - knob_size * 0.5 - 8.0);
    let bottom_y = controller_rect.y + controller_rect.h - knob_size - 20.0;
    let controller_knobs = [
        Rect::new(
            center_x - knob_size * 0.5,
            controller_rect.y + 20.0,
            knob_size,
            knob_size,
        ),
        Rect::new(
            left_center - knob_size * 0.5,
            bottom_y,
            knob_size,
            knob_size,
        ),
        Rect::new(
            right_center - knob_size * 0.5,
            bottom_y,
            knob_size,
            knob_size,
        ),
    ];

    let mut osc_range_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut osc_freq_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut osc_wave_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for index in 0..3 {
        let y = oscillator_rect.y + 30.0 + index as f32 * 110.0;
        let row_y = y;
        let spacing = (oscillator_rect.w - knob_size * 3.0) / 2.0;
        let x0 = oscillator_rect.x;
        osc_range_knobs[index] = Rect::new(x0, row_y, knob_size, knob_size);
        osc_freq_knobs[index] = Rect::new(x0 + knob_size + spacing, row_y, knob_size, knob_size);
        osc_wave_knobs[index] = Rect::new(
            x0 + 2.0 * (knob_size + spacing),
            row_y,
            knob_size,
            knob_size,
        );
    }

    let mut mixer_osc_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut mixer_extra_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 2];
    let mut mixer_toggle_rects = [Rect::new(0.0, 0.0, 0.0, 0.0); 5];
    let mut noise_selector_rects = [Rect::new(0.0, 0.0, 0.0, 0.0); NOISE_COLOR_COUNT];
    let row_spacing = knob_size + 25.0;
    let osc_x = mixer_rect.x + 20.0;
    let extra_x = mixer_rect.x + mixer_rect.w * 0.55;
    let toggle_size = vec2(30.0, 20.0);
    for index in 0..3 {
        let y = mixer_rect.y + 20.0 + index as f32 * row_spacing;
        mixer_osc_knobs[index] = Rect::new(osc_x, y, knob_size, knob_size);
        mixer_toggle_rects[index] = Rect::new(
            osc_x + knob_size + 14.0,
            y + knob_size * 0.5 - toggle_size.y * 0.5,
            toggle_size.x,
            toggle_size.y,
        );
    }
    for index in 0..2 {
        let y = mixer_rect.y + 20.0 + index as f32 * row_spacing;
        mixer_extra_knobs[index] = Rect::new(extra_x, y, knob_size, knob_size);
        let toggle_index = 3 + index;
        mixer_toggle_rects[toggle_index] = Rect::new(
            extra_x + knob_size + 14.0,
            y + knob_size * 0.5 - toggle_size.y * 0.5,
            toggle_size.x,
            toggle_size.y,
        );
    }
    let noise_button = vec2(64.0, 24.0);
    let noise_per_row = 3;
    let noise_start_x = mixer_extra_knobs[1].x;
    let noise_start_y = mixer_extra_knobs[1].y + knob_size + 36.0;
    for index in 0..NOISE_COLOR_COUNT {
        let row = index / noise_per_row;
        let col = index % noise_per_row;
        noise_selector_rects[index] = Rect::new(
            noise_start_x + col as f32 * (noise_button.x + 10.0),
            noise_start_y + row as f32 * (noise_button.y + 8.0),
            noise_button.x,
            noise_button.y,
        );
    }
    let overload_rect = Rect::new(
        mixer_extra_knobs[0].x + knob_size * 0.5 - 12.0,
        mixer_rect.y + 2.0,
        24.0,
        24.0,
    );

    let mut filter_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut filter_env_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let mut loudness_knobs = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    let column_spacing = (modifier_rect.w - knob_size * 3.0) / 2.0;
    for index in 0..3 {
        let x = modifier_rect.x + index as f32 * (knob_size + column_spacing);
        filter_knobs[index] = Rect::new(x, modifier_rect.y + 20.0, knob_size, knob_size);
        filter_env_knobs[index] = Rect::new(x, modifier_rect.y + 120.0, knob_size, knob_size);
    }
    let loudness_split = modifier_rect.y + modifier_rect.h * 0.58;
    for index in 0..3 {
        let x = modifier_rect.x + index as f32 * (knob_size + column_spacing);
        loudness_knobs[index] = Rect::new(x, loudness_split + 30.0, knob_size, knob_size);
    }

    let output_knobs = [
        Rect::new(
            output_rect.x + output_rect.w * 0.5 - knob_size * 0.5,
            output_rect.y + 60.0,
            knob_size,
            knob_size,
        ),
        Rect::new(
            output_rect.x + output_rect.w * 0.5 - knob_size * 0.5,
            output_rect.y + output_rect.h - knob_size - 40.0,
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
        modifier_loudness_split: loudness_split,
        controller_knobs,
        osc_range_knobs,
        osc_freq_knobs,
        osc_wave_knobs,
        mixer_osc_knobs,
        mixer_extra_knobs,
        mixer_toggle_rects,
        noise_selector_rects,
        overload_rect,
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
    pitch_target: f32,
    pitch_current: f32,
    mod_phase: f32,
    mod_signal: f32,
    mod_noise_color: NoiseColor,
    mod_noise: NoiseGenerator,
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
            pitch_target: 0.0,
            pitch_current: 0.0,
            mod_phase: 0.0,
            mod_signal: 0.0,
            mod_noise_color: NoiseColor::White,
            mod_noise: NoiseGenerator::new(),
        }
    }

    fn oscillator_mix_levels(&self) -> [f32; 3] {
        [
            if self.mixer_panel.osc_enabled[0] {
                self.mixer_panel.osc[0].value
            } else {
                0.0
            },
            if self.mixer_panel.osc_enabled[1] {
                self.mixer_panel.osc[1].value
            } else {
                0.0
            },
            if self.mixer_panel.osc_enabled[2] {
                self.mixer_panel.osc[2].value
            } else {
                0.0
            },
        ]
    }

    fn cutoff_hz(&self) -> f32 {
        let base =
            FILTER_MIN_HZ + self.modifiers_panel.filter[0].value * (FILTER_MAX_HZ - FILTER_MIN_HZ);
        let modulated = base * (1.0 + self.mod_signal * MOD_DEPTH);
        modulated.clamp(FILTER_MIN_HZ, FILTER_MAX_HZ)
    }

    fn master_level(&self) -> f32 {
        self.output_panel.main_volume.value
    }

    fn osc_detune(&self, index: usize) -> f32 {
        let value = self.oscillator.freq[index].value;
        (value * 2.0 - 1.0) * DETUNE_RANGE
    }

    fn tune_offset(&self) -> f32 {
        (self.controllers.tune.value - 0.5) * TUNE_RANGE_OCT
    }

    fn refresh_pitch_target(&mut self) {
        self.pitch_target = self.last_voltage + self.tune_offset();
    }

    fn glide_time(&self) -> f32 {
        GLIDE_MIN_SEC + self.controllers.glide.value * (GLIDE_MAX_SEC - GLIDE_MIN_SEC)
    }

    fn apply_pitch(&mut self, dt: f32, vcos: &[VcoHandle]) {
        let previous = self.pitch_current;
        if dt <= 0.0 || self.glide_time() <= 0.0001 {
            self.pitch_current = self.pitch_target;
        } else {
            let glide = self.glide_time().max(0.0001);
            let step = (dt / glide).clamp(0.0, 1.0);
            self.pitch_current += (self.pitch_target - self.pitch_current) * step;
        }
        if (self.pitch_current - previous).abs() > 0.0001 {
            for (_, tx) in vcos.iter() {
                let _ = tx.send(VcoCommand::SetVoltage(self.pitch_current));
            }
        }
    }

    fn update_modulation(&mut self, dt: f32) {
        self.mod_phase = (self.mod_phase + dt * MOD_LFO_FREQ).fract();
        let sine = (self.mod_phase * std::f32::consts::TAU).sin();
        let noise = self.mod_noise.sample(self.mod_noise_color);
        let mix = self.controllers.modulation_mix.value;
        self.mod_signal = sine * (1.0 - mix) + noise * mix;
    }
}

struct DebugWindowState {
    open: bool,
    rect: Rect,
    sample_rate: f32,
}

impl DebugWindowState {
    fn new() -> Self {
        Self {
            open: true,
            rect: Rect::new(20.0, 20.0, 400.0, 400.0),
            sample_rate: 44_100.0,
        }
    }

    fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
    }
}

#[derive(Default)]
struct KnobDragState {
    active_knob: Option<KnobId>,
    origin_value: f32,
    origin_y: f32,
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
            modulation_mix: KnobValue::implemented(0.5),
        }
    }
}

#[derive(Clone)]
struct OscillatorKnobs {
    range: [KnobValue; 3],
    freq: [KnobValue; 3],
    waveform: [KnobValue; 3],
}

impl OscillatorKnobs {
    fn new() -> Self {
        Self {
            range: std::array::from_fn(|_| KnobValue::stub(0.5)),
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
    osc_enabled: [bool; 3],
    ext_enabled: bool,
    noise_enabled: bool,
    noise_color: NoiseColor,
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
            noise: KnobValue::implemented(0.0),
            osc_enabled: [true; 3],
            ext_enabled: true,
            noise_enabled: true,
            noise_color: NoiseColor::White,
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
    OscRange1,
    OscRange2,
    OscRange3,
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

fn handle_mixer_switches(panel_state: &mut PanelState, layout: &PanelLayout) {
    if !is_mouse_button_pressed(MouseButton::Left) {
        return;
    }
    let mouse = mouse_position_vec();
    for (index, rect) in layout.mixer_toggle_rects.iter().enumerate() {
        if rect.contains(mouse) {
            match index {
                0..=2 => {
                    let flag = &mut panel_state.mixer_panel.osc_enabled[index];
                    *flag = !*flag;
                }
                3 => {
                    panel_state.mixer_panel.ext_enabled = !panel_state.mixer_panel.ext_enabled;
                }
                4 => {
                    panel_state.mixer_panel.noise_enabled = !panel_state.mixer_panel.noise_enabled;
                }
                _ => {}
            }
        }
    }
    for (index, rect) in layout.noise_selector_rects.iter().enumerate() {
        if rect.contains(mouse) {
            if let Some(color) = NoiseColor::VALUES.get(index).copied() {
                panel_state.mixer_panel.noise_color = color;
            }
        }
    }
}

fn draw_scene(
    texture: &Texture2D,
    panel_state: &mut PanelState,
    knob_drag: &mut KnobDragState,
    controller: &KeyboardController,
    layout: &PanelLayout,
    keyboard_layout: &KeyboardLayout,
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

    draw_controllers_panel(panel_state, knob_drag, layout);
    draw_oscillators(panel_state, knob_drag, layout);
    draw_mixer(panel_state, knob_drag, layout);
    draw_modifiers(panel_state, knob_drag, layout);
    draw_output_panel(panel_state, knob_drag, layout);
    draw_keyboard(controller, keyboard_layout);
    draw_debug_button(debug_window);
    if debug_window.open {
        draw_debug_window(debug_window, waveform, spectrum);
    }
}

fn draw_section(rect: &Rect, label: &str) {
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.05, 0.03, 0.02, 0.65),
    );
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

fn draw_controllers_panel(
    panel_state: &mut PanelState,
    knob_drag: &mut KnobDragState,
    layout: &PanelLayout,
) {
    draw_knob_widget(
        knob_drag,
        KnobId::ControllersTune,
        layout.controller_knobs[0],
        &mut panel_state.controllers.tune,
        "TUNE",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::ControllersGlide,
        layout.controller_knobs[1],
        &mut panel_state.controllers.glide,
        "GLIDE",
        None,
    );
    draw_knob_widget(
        knob_drag,
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
            if panel_state.last_midi >= 0 {
                "OPEN"
            } else {
                "IDLE"
            },
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
        &format!(
            "TUNE {:+.2} OCT\nGLIDE {:.2} s\nMOD NOISE {}",
            panel_state.tune_offset(),
            panel_state.glide_time(),
            panel_state.mod_noise_color.label()
        ),
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

fn draw_oscillators(
    panel_state: &mut PanelState,
    knob_drag: &mut KnobDragState,
    layout: &PanelLayout,
) {
    for index in 0..3 {
        let range_label = format!("OSC {} RANGE", index + 1);
        draw_knob_widget(
            knob_drag,
            match index {
                0 => KnobId::OscRange1,
                1 => KnobId::OscRange2,
                _ => KnobId::OscRange3,
            },
            layout.osc_range_knobs[index],
            &mut panel_state.oscillator.range[index],
            &range_label,
            None,
        );
        let freq_rect = layout.osc_freq_knobs[index];
        let wave_rect = layout.osc_wave_knobs[index];
        let detune = panel_state.osc_detune(index);
        let freq_label = format!("OSC {} FREQ", index + 1);
        let detune_label = format!("{:+.2} OCT", detune);
        draw_knob_widget(
            knob_drag,
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
            knob_drag,
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

fn draw_mixer(panel_state: &mut PanelState, knob_drag: &mut KnobDragState, layout: &PanelLayout) {
    draw_text_ex(
        "VOLUME",
        layout.mixer_rect.x + 10.0,
        layout.mixer_rect.y + 16.0,
        TextParams {
            font_size: 18,
            color: AMBER,
            ..Default::default()
        },
    );
    let osc_labels = ["OSC 1", "OSC 2", "OSC 3"];
    for index in 0..3 {
        let value_text = format!("{:.1}", panel_state.mixer_panel.osc[index].value * 10.0);
        draw_knob_widget(
            knob_drag,
            match index {
                0 => KnobId::MixerOsc1,
                1 => KnobId::MixerOsc2,
                _ => KnobId::MixerOsc3,
            },
            layout.mixer_osc_knobs[index],
            &mut panel_state.mixer_panel.osc[index],
            osc_labels[index],
            Some(&format!("{value_text}/10")),
        );
        draw_knob_scale(layout.mixer_osc_knobs[index]);
        draw_toggle_switch(
            layout.mixer_toggle_rects[index],
            panel_state.mixer_panel.osc_enabled[index],
            "ON",
        );
    }
    let extra_labels = ["EXT INPUT", "NOISE"];
    let mut extra_knobs = [
        &mut panel_state.mixer_panel.external_input,
        &mut panel_state.mixer_panel.noise,
    ];
    for index in 0..2 {
        let knob = &mut extra_knobs[index];
        let label = extra_labels[index];
        draw_knob_widget(
            knob_drag,
            if index == 0 {
                KnobId::MixerExternal
            } else {
                KnobId::MixerNoise
            },
            layout.mixer_extra_knobs[index],
            knob,
            label,
            Some(&format!("{:.1}/10", knob.value * 10.0)),
        );
        draw_knob_scale(layout.mixer_extra_knobs[index]);
        let toggle_index = 3 + index;
        let enabled = if index == 0 {
            panel_state.mixer_panel.ext_enabled
        } else {
            panel_state.mixer_panel.noise_enabled
        };
        draw_toggle_switch(layout.mixer_toggle_rects[toggle_index], enabled, "ON");
    }
    draw_noise_selector(
        &layout.noise_selector_rects,
        panel_state.mixer_panel.noise_color,
    );
    let overload_active = panel_state.oscillator_mix_levels().iter().sum::<f32>() > 2.5;
    draw_overload_lamp(layout.overload_rect, overload_active);
}

fn draw_knob_scale(rect: Rect) {
    draw_text_ex(
        "10",
        rect.x + rect.w + 8.0,
        rect.y + 14.0,
        TextParams {
            font_size: 12,
            color: AMBER_DIM,
            ..Default::default()
        },
    );
    draw_text_ex(
        "0",
        rect.x + rect.w + 14.0,
        rect.y + rect.h - 4.0,
        TextParams {
            font_size: 12,
            color: AMBER_DIM,
            ..Default::default()
        },
    );
}

fn draw_toggle_switch(rect: Rect, on: bool, label: &str) {
    let color = if on {
        AMBER
    } else {
        Color::new(0.1, 0.08, 0.05, 1.0)
    };
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.02, 0.02, 0.02, 1.0),
    );
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
    draw_rectangle(
        rect.x + 2.0,
        rect.y + 2.0,
        rect.w - 4.0,
        rect.h - 4.0,
        color,
    );
    draw_text_ex(
        label,
        rect.x + 4.0,
        rect.y + rect.h - 4.0,
        TextParams {
            font_size: 12,
            color: BACKGROUND,
            ..Default::default()
        },
    );
}

fn draw_noise_selector(rects: &[Rect; NOISE_COLOR_COUNT], selection: NoiseColor) {
    for (index, rect) in rects.iter().enumerate() {
        let color = NoiseColor::VALUES
            .get(index)
            .copied()
            .unwrap_or(NoiseColor::White);
        let active = selection == color;
        let fill = if active {
            AMBER
        } else {
            Color::new(0.08, 0.05, 0.03, 1.0)
        };
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, fill);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
        draw_text_ex(
            color.label(),
            rect.x + 4.0,
            rect.y + rect.h - 6.0,
            TextParams {
                font_size: 12,
                color: if active { BACKGROUND } else { AMBER },
                ..Default::default()
            },
        );
    }

    draw_text_ex(
        "NOISE COLOR",
        rects[0].x,
        rects[0].y - 6.0,
        TextParams {
            font_size: 12,
            color: AMBER_DIM,
            ..Default::default()
        },
    );
}

fn draw_overload_lamp(rect: Rect, active: bool) {
    let color = if active {
        AMBER
    } else {
        Color::new(0.1, 0.08, 0.05, 1.0)
    };
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.02, 0.02, 0.02, 1.0),
    );
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
    draw_circle(
        rect.x + rect.w * 0.5,
        rect.y + rect.h * 0.5,
        rect.w.min(rect.h) * 0.3,
        color,
    );
    draw_text_ex(
        "OVERLOAD",
        rect.x - 10.0,
        rect.y - 4.0,
        TextParams {
            font_size: 12,
            color: AMBER,
            ..Default::default()
        },
    );
}

fn draw_modifiers(
    panel_state: &mut PanelState,
    knob_drag: &mut KnobDragState,
    layout: &PanelLayout,
) {
    let line_y = layout.modifier_loudness_split + 10.0;
    draw_line(
        layout.modifier_rect.x + 8.0,
        line_y,
        layout.modifier_rect.x + layout.modifier_rect.w - 8.0,
        line_y,
        1.0,
        AMBER_DIM,
    );
    let cutoff_text = format!("{:.0} Hz", panel_state.cutoff_hz());
    draw_knob_widget(
        knob_drag,
        KnobId::FilterCutoff,
        layout.filter_knobs[0],
        &mut panel_state.modifiers_panel.filter[0],
        "CUTOFF",
        Some(&cutoff_text),
    );
    draw_knob_widget(
        knob_drag,
        KnobId::FilterEmphasis,
        layout.filter_knobs[1],
        &mut panel_state.modifiers_panel.filter[1],
        "EMPHASIS",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::FilterContour,
        layout.filter_knobs[2],
        &mut panel_state.modifiers_panel.filter[2],
        "AMT CONTOUR",
        None,
    );

    draw_knob_widget(
        knob_drag,
        KnobId::FilterAttack,
        layout.filter_env_knobs[0],
        &mut panel_state.modifiers_panel.filter_env[0],
        "ATTACK",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::FilterDecay,
        layout.filter_env_knobs[1],
        &mut panel_state.modifiers_panel.filter_env[1],
        "DECAY",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::FilterSustain,
        layout.filter_env_knobs[2],
        &mut panel_state.modifiers_panel.filter_env[2],
        "SUSTAIN",
        None,
    );

    draw_knob_widget(
        knob_drag,
        KnobId::LoudnessAttack,
        layout.loudness_knobs[0],
        &mut panel_state.modifiers_panel.loudness_env[0],
        "LOUD ATTACK",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::LoudnessDecay,
        layout.loudness_knobs[1],
        &mut panel_state.modifiers_panel.loudness_env[1],
        "LOUD DECAY",
        None,
    );
    draw_knob_widget(
        knob_drag,
        KnobId::LoudnessSustain,
        layout.loudness_knobs[2],
        &mut panel_state.modifiers_panel.loudness_env[2],
        "LOUD SUSTAIN",
        None,
    );
}

fn draw_output_panel(
    panel_state: &mut PanelState,
    knob_drag: &mut KnobDragState,
    layout: &PanelLayout,
) {
    let master = format!("{:.0}%", panel_state.master_level() * 100.0);
    draw_knob_widget(
        knob_drag,
        KnobId::OutputVolume,
        layout.output_knobs[0],
        &mut panel_state.output_panel.main_volume,
        "MAIN VOL",
        Some(&master),
    );
    let phones = format!(
        "{:.0}%",
        panel_state.output_panel.phones_volume.value * 100.0
    );
    draw_knob_widget(
        knob_drag,
        KnobId::OutputPhones,
        layout.output_knobs[1],
        &mut panel_state.output_panel.phones_volume,
        "PHONES",
        Some(&phones),
    );
}

fn draw_knob_widget(
    knob_drag: &mut KnobDragState,
    knob_id: KnobId,
    rect: Rect,
    knob: &mut KnobValue,
    label: &str,
    display: Option<&str>,
) {
    handle_knob_drag(knob_drag, knob_id, rect, knob);
    let center = vec2(rect.x + rect.w * 0.5, rect.y + rect.h * 0.5);
    let radius = rect.w.min(rect.h) * 0.35;
    draw_circle(
        center.x,
        center.y,
        radius + 6.0,
        Color::new(0.05, 0.03, 0.02, 1.0),
    );
    draw_circle(
        center.x,
        center.y,
        radius,
        Color::new(0.12, 0.12, 0.12, 1.0),
    );
    draw_circle(
        center.x,
        center.y,
        radius * 0.65,
        Color::new(0.2, 0.2, 0.2, 1.0),
    );
    draw_circle_lines(center.x, center.y, radius + 6.0, 1.0, AMBER_DIM);
    draw_circle_lines(
        center.x,
        center.y,
        radius,
        1.0,
        Color::new(0.4, 0.4, 0.4, 0.3),
    );
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
    knob_drag: &mut KnobDragState,
    knob_id: KnobId,
    rect: Rect,
    knob: &mut KnobValue,
) {
    let mouse = mouse_position_vec();
    if is_mouse_button_pressed(MouseButton::Left) && rect.contains(mouse) {
        knob_drag.active_knob = Some(knob_id);
        knob_drag.origin_value = knob.value;
        knob_drag.origin_y = mouse.y;
    }
    if let Some(active) = knob_drag.active_knob {
        if active == knob_id {
            if is_mouse_button_down(MouseButton::Left) {
                let delta = (knob_drag.origin_y - mouse.y) * 0.005;
                knob.value = (knob_drag.origin_value + delta).clamp(0.0, 1.0);
            } else {
                knob_drag.active_knob = None;
            }
        }
    }
    if is_mouse_button_released(MouseButton::Left) && knob_drag.active_knob == Some(knob_id) {
        knob_drag.active_knob = None;
    }
    let (_x, wheel) = mouse_wheel();
    if rect.contains(mouse) && wheel.abs() > f32::EPSILON {
        knob.value = (knob.value + wheel * 0.03).clamp(0.0, 1.0);
    }
}

struct KeyVisual {
    rect: Rect,
    keycode: KeyCode,
    label: &'static str,
}

struct KeyboardLayout {
    white: Vec<KeyVisual>,
    black: Vec<KeyVisual>,
}

impl KeyboardLayout {
    fn hit_test(&self, point: Vec2) -> Option<KeyCode> {
        for key in &self.black {
            if key.rect.contains(point) {
                return Some(key.keycode);
            }
        }
        for key in &self.white {
            if key.rect.contains(point) {
                return Some(key.keycode);
            }
        }
        None
    }
}

fn build_keyboard_layout(controller: &KeyboardController) -> KeyboardLayout {
    let area = Rect::new(
        40.0,
        PANEL_HEIGHT + 40.0,
        SCREEN_WIDTH - 80.0,
        SCREEN_HEIGHT - PANEL_HEIGHT - 80.0,
    );
    let spacing = 18.0;
    let white_count = controller.white_keys().len() as f32;
    let max_size_width = (area.w - spacing * (white_count - 1.0)) / white_count;
    let max_size_height = (area.h - spacing * 3.0) / 2.0;
    let key_size = max_size_width.min(max_size_height).max(40.0);
    let total_width = white_count * key_size + (white_count - 1.0) * spacing;
    let start_x = area.x + (area.w - total_width) * 0.5;
    let white_y = area.y + area.h - key_size;
    let black_y = white_y - key_size - spacing * 0.7;

    let mut white = Vec::new();
    for (index, binding) in controller.white_keys().iter().enumerate() {
        let x = start_x + index as f32 * (key_size + spacing);
        let rect = Rect::new(x, white_y, key_size, key_size);
        white.push(KeyVisual {
            rect,
            keycode: binding.keycode,
            label: binding.label,
        });
    }

    let mut black = Vec::new();
    for binding in controller.black_keys() {
        let center = start_x + binding.position_hint * total_width;
        let rect = Rect::new(center - key_size * 0.5, black_y, key_size, key_size);
        if rect.x + rect.w >= area.x && rect.x <= area.x + area.w {
            black.push(KeyVisual {
                rect,
                keycode: binding.keycode,
                label: binding.label,
            });
        }
    }

    KeyboardLayout { white, black }
}

fn draw_keyboard(controller: &KeyboardController, layout: &KeyboardLayout) {
    for key in &layout.white {
        let active = controller.is_pressed(key.keycode);
        draw_key(key.rect, active, false, key.label);
    }
    for key in &layout.black {
        let active = controller.is_pressed(key.keycode);
        draw_key(key.rect, active, true, key.label);
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
    draw_rounded_rect(rect, 10.0, fill_color);
    draw_rounded_rect_lines(rect, 10.0, AMBER);
    draw_centered_text(label, rect, KEY_FONT_SIZE);
}

fn draw_rounded_rect(rect: Rect, radius: f32, color: Color) {
    draw_rectangle(
        rect.x + radius,
        rect.y,
        rect.w - 2.0 * radius,
        rect.h,
        color,
    );
    draw_rectangle(
        rect.x,
        rect.y + radius,
        rect.w,
        rect.h - 2.0 * radius,
        color,
    );
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
    let top = rect.y;
    let bottom = rect.y + rect.h;
    let left = rect.x;
    let right = rect.x + rect.w;
    let left_x = left + radius;
    let right_x = right - radius;
    let top_y = top + radius;
    let bottom_y = bottom - radius;

    draw_line(left_x, top, right_x, top, 1.0, color);
    draw_line(left_x, bottom, right_x, bottom, 1.0, color);
    draw_line(left, top_y, left, bottom_y, 1.0, color);
    draw_line(right, top_y, right, bottom_y, 1.0, color);

    draw_corner_arc(
        vec2(left_x, top_y),
        std::f32::consts::PI,
        1.5 * std::f32::consts::PI,
        radius,
        color,
    );
    draw_corner_arc(
        vec2(right_x, top_y),
        1.5 * std::f32::consts::PI,
        0.0,
        radius,
        color,
    );
    draw_corner_arc(
        vec2(right_x, bottom_y),
        0.0,
        0.5 * std::f32::consts::PI,
        radius,
        color,
    );
    draw_corner_arc(
        vec2(left_x, bottom_y),
        0.5 * std::f32::consts::PI,
        std::f32::consts::PI,
        radius,
        color,
    );
}

fn draw_corner_arc(center: Vec2, start: f32, end: f32, radius: f32, color: Color) {
    let tau = std::f32::consts::TAU;
    let normalized_start = start.rem_euclid(tau);
    let normalized_end = end.rem_euclid(tau);
    let mut sweep = normalized_end - normalized_start;
    if sweep <= 0.0 {
        sweep += tau;
    }
    let steps = 10;
    let mut prev = center
        + vec2(
            normalized_start.cos() * radius,
            normalized_start.sin() * radius,
        );
    for idx in 1..=steps {
        let angle = normalized_start + sweep * (idx as f32 / steps as f32);
        let norm_angle = angle.rem_euclid(tau);
        let next = center + vec2(norm_angle.cos() * radius, norm_angle.sin() * radius);
        draw_line(prev.x, prev.y, next.x, next.y, 1.0, color);
        prev = next;
    }
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
        draw_rectangle(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Color::new(0.05, 0.03, 0.02, 1.0),
        );
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
        draw_centered_text("DEBUGGER", rect, 18);
    }
}

fn draw_debug_window(state: &DebugWindowState, waveform: &[f32], spectrum: &[f32]) {
    let rect = state.rect;
    draw_rectangle(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Color::new(0.02, 0.02, 0.02, 0.95),
    );
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
    draw_centered_text(
        "X",
        Rect::new(rect.x + rect.w - 32.0, rect.y + 8.0, 24.0, 24.0),
        20,
    );

    let scope_rect = Rect::new(rect.x + 16.0, rect.y + 52.0, rect.w - 32.0, 110.0);
    draw_rectangle_lines(
        scope_rect.x,
        scope_rect.y,
        scope_rect.w,
        scope_rect.h,
        1.0,
        AMBER,
    );
    draw_waveform(scope_rect, waveform);

    let freq_rect = Rect::new(
        rect.x + 16.0,
        scope_rect.y + scope_rect.h + 24.0,
        rect.w - 32.0,
        rect.h - scope_rect.h - 90.0,
    );
    draw_rectangle_lines(
        freq_rect.x,
        freq_rect.y,
        freq_rect.w,
        freq_rect.h,
        1.0,
        AMBER,
    );
    draw_frequency(freq_rect, spectrum, state.sample_rate);
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

fn draw_frequency(rect: Rect, spectrum: &[f32], sample_rate: f32) {
    if spectrum.is_empty() {
        return;
    }
    let nyquist = sample_rate * 0.5;
    let max_freq = MAX_ANALYZER_FREQ.min(nyquist);
    let freq_ratio = max_freq / nyquist;
    let usable_bins = ((spectrum.len() as f32) * freq_ratio).max(1.0) as usize;
    let mut prev = None;
    for i in 0..usable_bins {
        let freq = nyquist * (i as f32 / spectrum.len() as f32);
        if freq > MAX_ANALYZER_FREQ {
            break;
        }
        let x = rect.x + (freq / MAX_ANALYZER_FREQ) * rect.w;
        let magnitude = spectrum[i].max(1e-6);
        let db = 20.0 * magnitude.log10();
        let normalized =
            ((db - MIN_ANALYZER_DB) / (MAX_ANALYZER_DB - MIN_ANALYZER_DB)).clamp(0.0, 1.0);
        let y = rect.y + rect.h - normalized * rect.h;
        if let Some((px, py)) = prev {
            draw_line(px, py, x, y, 2.0, AMBER_DIM);
        }
        prev = Some((x, y));
    }

    // axis lines
    let zero = (0.0 - MIN_ANALYZER_DB) / (MAX_ANALYZER_DB - MIN_ANALYZER_DB);
    let zero_y = rect.y + rect.h - zero * rect.h;
    draw_line(rect.x, zero_y, rect.x + rect.w, zero_y, 1.0, AMBER_DIM);

    for db in [MIN_ANALYZER_DB, 0.0, MAX_ANALYZER_DB] {
        let ratio = (db - MIN_ANALYZER_DB) / (MAX_ANALYZER_DB - MIN_ANALYZER_DB);
        let y = rect.y + rect.h - ratio * rect.h;
        draw_line(
            rect.x,
            y,
            rect.x + rect.w,
            y,
            0.5,
            Color::new(0.2, 0.1, 0.03, 0.4),
        );
        draw_text_ex(
            &format!("{db:.0} dB"),
            rect.x - 60.0,
            y + 4.0,
            TextParams {
                font_size: 14,
                color: AMBER,
                ..Default::default()
            },
        );
    }

    let freq_labels = [0.0, 5_000.0, 10_000.0, 15_000.0, 20_000.0, 25_000.0];
    for freq in freq_labels {
        let ratio = (freq / MAX_ANALYZER_FREQ).clamp(0.0, 1.0);
        let x = rect.x + ratio * rect.w;
        draw_line(
            x,
            rect.y,
            x,
            rect.y + rect.h,
            0.3,
            Color::new(0.2, 0.1, 0.03, 0.3),
        );
        draw_text_ex(
            &format!("{:.0}k", freq / 1000.0),
            x - 12.0,
            rect.y + rect.h + 16.0,
            TextParams {
                font_size: 14,
                color: AMBER,
                ..Default::default()
            },
        );
    }

    draw_text_ex(
        "FREQUENCY (kHz)",
        rect.x + rect.w * 0.5 - 70.0,
        rect.y + rect.h + 34.0,
        TextParams {
            font_size: 16,
            color: AMBER,
            ..Default::default()
        },
    );
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
        for (index, enabled) in panel_state.mixer_panel.osc_enabled.iter().enumerate() {
            synth.set_osc_enabled(index, *enabled);
        }
        synth.set_noise_level(panel_state.mixer_panel.noise.value);
        synth.set_noise_enabled(panel_state.mixer_panel.noise_enabled);
        synth.set_noise_color(panel_state.mixer_panel.noise_color);
        synth.set_master_level(panel_state.master_level());
        synth.set_cutoff(panel_state.cutoff_hz());
    }
}

fn feed_stub_knobs(panel_state: &PanelState) {
    for rage in &panel_state.oscillator.range {
        stub_oscillator_range(rage.value);
    }
    stub_external_input_volume(panel_state.mixer_panel.external_input.value);
    stub_mixer_external_toggle(panel_state.mixer_panel.ext_enabled);
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

fn stub_oscillator_range(_value: f32) {
    // TODO: Switch oscillator range to follow MiniMoog foot settings.
}

fn stub_external_input_volume(_value: f32) {
    // TODO: Mix external input audio stream.
}

fn stub_mixer_external_toggle(_on: bool) {
    // TODO: Implement external input enable switch.
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
