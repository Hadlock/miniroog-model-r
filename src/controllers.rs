use std::collections::HashMap;

use macroquad::prelude::*;

const MIDI_MIN: i32 = 21;
const MIDI_MAX: i32 = 108;

#[derive(Clone)]
pub struct KeyBinding {
    pub label: &'static str,
    pub keycode: KeyCode,
    pub midi: i32,
    pub position_hint: f32,
}

pub struct ControllerMessage {
    pub gate: bool,
    pub voltage: f32,
    pub midi_note: i32,
}

pub struct KeyboardController {
    white_keys: Vec<KeyBinding>,
    black_keys: Vec<KeyBinding>,
    pressed: Vec<KeyCode>,
    lookup: HashMap<KeyCode, KeyBinding>,
    last_voltage: f32,
    octave_shift: i32,
    min_shift: i32,
    max_shift: i32,
    mouse_active: Option<KeyCode>,
}

impl KeyboardController {
    pub fn new() -> Self {
        let white_keys = vec![
            KeyBinding {
                label: "Z",
                keycode: KeyCode::Z,
                midi: 48,
                position_hint: 0.0,
            },
            KeyBinding {
                label: "X",
                keycode: KeyCode::X,
                midi: 50,
                position_hint: 1.0,
            },
            KeyBinding {
                label: "C",
                keycode: KeyCode::C,
                midi: 52,
                position_hint: 2.0,
            },
            KeyBinding {
                label: "V",
                keycode: KeyCode::V,
                midi: 53,
                position_hint: 3.0,
            },
            KeyBinding {
                label: "B",
                keycode: KeyCode::B,
                midi: 55,
                position_hint: 4.0,
            },
            KeyBinding {
                label: "N",
                keycode: KeyCode::N,
                midi: 57,
                position_hint: 5.0,
            },
            KeyBinding {
                label: "M",
                keycode: KeyCode::M,
                midi: 59,
                position_hint: 6.0,
            },
            KeyBinding {
                label: ",",
                keycode: KeyCode::Comma,
                midi: 60,
                position_hint: 7.0,
            },
            KeyBinding {
                label: ".",
                keycode: KeyCode::Period,
                midi: 62,
                position_hint: 8.0,
            },
            KeyBinding {
                label: "/",
                keycode: KeyCode::Slash,
                midi: 64,
                position_hint: 9.0,
            },
        ];

        let black_keys = vec![
            KeyBinding {
                label: "S",
                keycode: KeyCode::S,
                midi: 49,
                position_hint: 0.105,
            },
            KeyBinding {
                label: "D",
                keycode: KeyCode::D,
                midi: 51,
                position_hint: 0.205,
            },
            KeyBinding {
                label: "G",
                keycode: KeyCode::G,
                midi: 54,
                position_hint: 0.3888889,
            },
            KeyBinding {
                label: "H",
                keycode: KeyCode::H,
                midi: 56,
                position_hint: 0.5,
            },
            KeyBinding {
                label: "J",
                keycode: KeyCode::J,
                midi: 58,
                position_hint: 0.6111111,
            },
            KeyBinding {
                label: "L",
                keycode: KeyCode::L,
                midi: 61,
                position_hint: 0.79,
            },
            KeyBinding {
                label: ";",
                keycode: KeyCode::Semicolon,
                midi: 63,
                position_hint: 0.9,
            },
            KeyBinding {
                label: "'",
                keycode: KeyCode::Apostrophe,
                midi: 66,
                position_hint: 1.05,
            },
            KeyBinding {
                label: "]",
                keycode: KeyCode::RightBracket,
                midi: 68,
                position_hint: 1.2,
            },
            KeyBinding {
                label: "\\",
                keycode: KeyCode::Backslash,
                midi: 70,
                position_hint: 1.35,
            },
        ];

        let mut lookup = HashMap::new();
        for binding in white_keys.iter().chain(black_keys.iter()) {
            lookup.insert(binding.keycode, binding.clone());
        }

        let min_note = white_keys
            .iter()
            .chain(black_keys.iter())
            .map(|k| k.midi)
            .min()
            .unwrap_or(MIDI_MIN);
        let max_note = white_keys
            .iter()
            .chain(black_keys.iter())
            .map(|k| k.midi)
            .max()
            .unwrap_or(MIDI_MAX);

        let min_shift = ((MIDI_MIN - min_note) as f32 / 12.0).ceil() as i32;
        let max_shift = ((MIDI_MAX - max_note) as f32 / 12.0).floor() as i32;

        Self {
            white_keys,
            black_keys,
            pressed: Vec::new(),
            lookup,
            last_voltage: midi_to_voltage(48),
            octave_shift: 0,
            min_shift,
            max_shift,
            mouse_active: None,
        }
    }

    pub fn poll(&mut self, external_change: bool) -> Option<ControllerMessage> {
        let mut changed = external_change;

        if is_key_pressed(KeyCode::Minus) {
            self.adjust_octave(-1);
            changed = true;
        }
        if is_key_pressed(KeyCode::Equal) {
            self.adjust_octave(1);
            changed = true;
        }

        let keycodes: Vec<KeyCode> = self.lookup.keys().copied().collect();
        for keycode in keycodes {
            if is_key_pressed(keycode) {
                changed |= self.press_key(keycode);
            }
            if is_key_released(keycode) {
                changed |= self.release_key(keycode);
            }
        }
        if changed {
            Some(self.current_message())
        } else {
            None
        }
    }

    pub fn handle_mouse_keys(
        &mut self,
        hovered: Option<KeyCode>,
        mouse_pressed: bool,
        mouse_down: bool,
        mouse_released: bool,
    ) -> bool {
        let mut changed = false;
        if mouse_pressed {
            if let Some(code) = hovered {
                self.mouse_active = Some(code);
                changed |= self.press_key(code);
            }
        }
        if mouse_down {
            if let Some(active) = self.mouse_active {
                if let Some(hit) = hovered {
                    if hit != active {
                        changed |= self.release_key(active);
                        self.mouse_active = Some(hit);
                        changed |= self.press_key(hit);
                    }
                } else {
                    changed |= self.release_key(active);
                    self.mouse_active = None;
                }
            }
        }
        if mouse_released {
            if let Some(active) = self.mouse_active.take() {
                changed |= self.release_key(active);
            }
        }
        changed
    }

    fn current_message(&mut self) -> ControllerMessage {
        if let Some(last) = self.pressed.last() {
            if let Some(binding) = self.lookup.get(last) {
                let midi = (binding.midi + self.octave_shift * 12).clamp(MIDI_MIN, MIDI_MAX);
                let voltage = midi_to_voltage(midi);
                self.last_voltage = voltage;
                return ControllerMessage {
                    gate: true,
                    voltage,
                    midi_note: midi,
                };
            }
        }
        ControllerMessage {
            gate: false,
            voltage: self.last_voltage,
            midi_note: -1,
        }
    }

    fn adjust_octave(&mut self, delta: i32) {
        let new_shift = (self.octave_shift + delta).clamp(self.min_shift, self.max_shift);
        self.octave_shift = new_shift;
    }

    fn press_key(&mut self, keycode: KeyCode) -> bool {
        if self.pressed.contains(&keycode) {
            false
        } else {
            self.pressed.push(keycode);
            true
        }
    }

    fn release_key(&mut self, keycode: KeyCode) -> bool {
        if let Some(index) = self.pressed.iter().position(|code| *code == keycode) {
            self.pressed.remove(index);
            true
        } else {
            false
        }
    }

    pub fn is_pressed(&self, keycode: KeyCode) -> bool {
        self.pressed.contains(&keycode)
    }

    pub fn white_keys(&self) -> &[KeyBinding] {
        &self.white_keys
    }

    pub fn black_keys(&self) -> &[KeyBinding] {
        &self.black_keys
    }
}

pub fn midi_to_voltage(midi_note: i32) -> f32 {
    (midi_note as f32 - 33.0) / 12.0
}
