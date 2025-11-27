use std::collections::HashMap;

use macroquad::prelude::*;

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
                position_hint: 0.5,
            },
            KeyBinding {
                label: "D",
                keycode: KeyCode::D,
                midi: 51,
                position_hint: 1.5,
            },
            KeyBinding {
                label: "G",
                keycode: KeyCode::G,
                midi: 54,
                position_hint: 3.5,
            },
            KeyBinding {
                label: "H",
                keycode: KeyCode::H,
                midi: 56,
                position_hint: 4.5,
            },
            KeyBinding {
                label: "J",
                keycode: KeyCode::J,
                midi: 58,
                position_hint: 5.5,
            },
            KeyBinding {
                label: "L",
                keycode: KeyCode::L,
                midi: 61,
                position_hint: 6.5,
            },
            KeyBinding {
                label: ";",
                keycode: KeyCode::Semicolon,
                midi: 63,
                position_hint: 7.5,
            },
            KeyBinding {
                label: "'",
                keycode: KeyCode::Apostrophe,
                midi: 66,
                position_hint: 8.5,
            },
            KeyBinding {
                label: "]",
                keycode: KeyCode::RightBracket,
                midi: 68,
                position_hint: 9.5,
            },
            KeyBinding {
                label: "\\",
                keycode: KeyCode::Backslash,
                midi: 70,
                position_hint: 10.5,
            },
        ];

        let mut lookup = HashMap::new();
        for binding in white_keys.iter().chain(black_keys.iter()) {
            lookup.insert(binding.keycode, binding.clone());
        }

        Self {
            white_keys,
            black_keys,
            pressed: Vec::new(),
            lookup,
            last_voltage: midi_to_voltage(48),
        }
    }

    pub fn poll(&mut self) -> Option<ControllerMessage> {
        let mut changed = false;
        for binding in self.lookup.values() {
            if is_key_pressed(binding.keycode) {
                self.pressed.push(binding.keycode);
                changed = true;
            }
            if is_key_released(binding.keycode) {
                if let Some(index) = self
                    .pressed
                    .iter()
                    .position(|code| *code == binding.keycode)
                {
                    self.pressed.remove(index);
                    changed = true;
                }
            }
        }
        if changed {
            Some(self.current_message())
        } else {
            None
        }
    }

    fn current_message(&mut self) -> ControllerMessage {
        if let Some(last) = self.pressed.last() {
            if let Some(binding) = self.lookup.get(last) {
                let voltage = midi_to_voltage(binding.midi);
                self.last_voltage = voltage;
                return ControllerMessage {
                    gate: true,
                    voltage,
                    midi_note: binding.midi,
                };
            }
        }
        ControllerMessage {
            gate: false,
            voltage: self.last_voltage,
            midi_note: -1,
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
