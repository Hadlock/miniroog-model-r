mod controllers;
mod mixer;
mod modifiers;
mod oscillatorbank;
mod output;
mod vco;

use std::sync::{Arc, Mutex};

use controllers::{cycle_waveform, KeyboardController};
use macroquad::{prelude::*, text::measure_text};
use modifiers::compute_spectrum;
use oscillatorbank::OscillatorBank;
use output::{AudioEngine, DebugData, SynthPipeline};
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

    for (index, (_, tx)) in vcos.iter().enumerate() {
        let _ = tx.send(VcoCommand::SetWaveform(panel_state.waveforms[index]));
        let _ = tx.send(VcoCommand::SetDetune(panel_state.detune[index]));
    }

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
        handle_panel_interactions(&mut panel_state, &layout, &vcos, mouse);
        handle_debug_toggle(&mut debug_window, mouse);

        if let Ok(mut synth) = pipeline.lock() {
            for (index, level) in panel_state.mix_levels.iter().enumerate() {
                synth.set_mix_level(index, *level);
            }
            synth.set_master_level(panel_state.master);
            synth.set_cutoff(panel_state.cutoff);
        }

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
            &panel_state,
            &controller,
            &layout,
            &waveform_cache,
            &spectrum_cache,
            &debug_window,
        );

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
    wave_buttons: [Rect; 3],
    mix_sliders: [Rect; 3],
    master_slider: Rect,
    cutoff_slider: Rect,
}

fn compute_panel_layout() -> PanelLayout {
    let margin = 36.0;
    let gap = 18.0;
    let usable_width = SCREEN_WIDTH - margin * 2.0 - gap * 3.0;
    let section_width = usable_width / 4.0;
    let section_height = PANEL_HEIGHT - 80.0;
    let top = 40.0;

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

    let mut wave_buttons = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for index in 0..3 {
        wave_buttons[index] = Rect::new(
            oscillator_rect.x + 18.0,
            oscillator_rect.y + 28.0 + index as f32 * 76.0,
            oscillator_rect.w - 36.0,
            60.0,
        );
    }

    let mut mix_sliders = [Rect::new(0.0, 0.0, 0.0, 0.0); 3];
    for index in 0..3 {
        mix_sliders[index] = Rect::new(
            mixer_rect.x + 20.0 + index as f32 * 70.0,
            mixer_rect.y + 32.0,
            50.0,
            mixer_rect.h - 64.0,
        );
    }

    let master_slider = Rect::new(
        modifier_rect.x + modifier_rect.w - 50.0,
        modifier_rect.y + 32.0,
        30.0,
        modifier_rect.h - 64.0,
    );
    let cutoff_slider = Rect::new(
        modifier_rect.x + 20.0,
        modifier_rect.y + 32.0,
        30.0,
        modifier_rect.h - 64.0,
    );

    PanelLayout {
        controller_rect,
        oscillator_rect,
        mixer_rect,
        modifier_rect,
        wave_buttons,
        mix_sliders,
        master_slider,
        cutoff_slider,
    }
}

#[derive(Clone)]
struct PanelState {
    waveforms: [Waveform; 3],
    mix_levels: [f32; 3],
    detune: [f32; 3],
    master: f32,
    cutoff: f32,
    last_midi: i32,
    last_voltage: f32,
}

impl PanelState {
    fn new() -> Self {
        Self {
            waveforms: [Waveform::Saw, Waveform::Saw, Waveform::Saw],
            mix_levels: [0.85, 0.7, 0.55],
            detune: [0.0, 0.03, -0.02],
            master: 0.7,
            cutoff: 2200.0,
            last_midi: -1,
            last_voltage: 0.0,
        }
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

fn mouse_position_vec() -> Vec2 {
    let (x, y) = mouse_position();
    vec2(x, y)
}

fn handle_panel_interactions(
    panel_state: &mut PanelState,
    layout: &PanelLayout,
    vcos: &[VcoHandle],
    mouse: Vec2,
) {
    if is_mouse_button_pressed(MouseButton::Left) {
        for (index, rect) in layout.wave_buttons.iter().enumerate() {
            if rect.contains(mouse) {
                panel_state.waveforms[index] = cycle_waveform(panel_state.waveforms[index]);
                if let Some((_, tx)) = vcos.get(index) {
                    let _ = tx.send(VcoCommand::SetWaveform(panel_state.waveforms[index]));
                }
            }
        }
    }

    if is_mouse_button_down(MouseButton::Left) {
        for (index, rect) in layout.mix_sliders.iter().enumerate() {
            if rect.contains(mouse) {
                let value = 1.0 - ((mouse.y - rect.y) / rect.h);
                panel_state.mix_levels[index] = value.clamp(0.0, 1.0);
            }
        }
        if layout.master_slider.contains(mouse) {
            let value = 1.0 - ((mouse.y - layout.master_slider.y) / layout.master_slider.h);
            panel_state.master = value.clamp(0.0, 1.0);
        }
        if layout.cutoff_slider.contains(mouse) {
            let value = 1.0 - ((mouse.y - layout.cutoff_slider.y) / layout.cutoff_slider.h);
            panel_state.cutoff = 200.0 + value.clamp(0.0, 1.0) * 4800.0;
        }
    }
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
    panel_state: &PanelState,
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
    draw_section(&layout.modifier_rect, "MODIFIERS & OUTPUT");

    draw_controller_info(panel_state, &layout.controller_rect);
    draw_oscillators(panel_state, layout);
    draw_mixer(panel_state, layout);
    draw_modifiers(panel_state, layout);
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

fn draw_oscillators(panel_state: &PanelState, layout: &PanelLayout) {
    for (index, rect) in layout.wave_buttons.iter().enumerate() {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, PANEL_BROWN);
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
        draw_text_ex(
            &format!("VCO {} {}", index + 1, panel_state.waveforms[index].label()),
            rect.x + 12.0,
            rect.y + 36.0,
            TextParams {
                font_size: 20,
                color: AMBER,
                ..Default::default()
            },
        );
    }
}

fn draw_mixer(panel_state: &PanelState, layout: &PanelLayout) {
    for (index, rect) in layout.mix_sliders.iter().enumerate() {
        draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.04, 0.03, 0.02, 0.9));
        draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
        let knob_height = 18.0;
        let y = rect.y + rect.h - knob_height - panel_state.mix_levels[index] * (rect.h - knob_height);
        draw_rectangle(rect.x + 4.0, y, rect.w - 8.0, knob_height, AMBER);
        draw_centered_text(
            &format!("V{}", index + 1),
            Rect::new(rect.x, rect.y - 12.0, rect.w, 20.0),
            16,
        );
    }
}

fn draw_modifiers(panel_state: &PanelState, layout: &PanelLayout) {
    draw_slider(
        &layout.cutoff_slider,
        (panel_state.cutoff - 200.0) / 4800.0,
        "VCF",
    );
    draw_slider(
        &layout.master_slider,
        panel_state.master,
        "MASTER",
    );
    draw_text_block(
        layout.modifier_rect.x + 70.0,
        layout.modifier_rect.y + 60.0,
        &format!("CUTOFF {:.0} Hz\nMASTER {:.0}%",
        panel_state.cutoff,
        panel_state.master * 100.0),
    );
}

fn draw_slider(rect: &Rect, value: f32, label: &str) {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.04, 0.03, 0.02, 0.9));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, AMBER);
    let knob_height = 20.0;
    let y = rect.y + rect.h - knob_height - value.clamp(0.0, 1.0) * (rect.h - knob_height);
    draw_rectangle(rect.x + 4.0, y, rect.w - 8.0, knob_height, AMBER);
    draw_centered_text(label, Rect::new(rect.x - 20.0, rect.y - 14.0, rect.w + 40.0, 20.0), 16);
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
