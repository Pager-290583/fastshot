//! Scripted input for end-to-end tests of the capture screen.
//!
//! `FASTSHOT_SCRIPT=file fastshot gui -r > out.png` replays one command per
//! frame (coordinates in logical points):
//!
//! ```text
//! drag 100 100 400 300      # press, move in steps, release
//! down 10 10 [right]  /  up 10 10 [right]  /  move 10 10
//! key Enter [ctrl] [shift]
//! text Hello
//! wheel 3
//! wait 5
//! ```

use std::collections::VecDeque;

use egui::{Event, Key, Modifiers, PointerButton, RawInput, pos2, vec2};

pub struct Script {
    lines: VecDeque<String>,
    wait: u32,
    mods: Modifiers,
}

impl Script {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("FASTSHOT_SCRIPT")?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(Self {
            lines: text
                .lines()
                .map(|l| l.split('#').next().unwrap_or("").trim().to_owned())
                .filter(|l| !l.is_empty())
                .collect(),
            // Give the window time to map and go fullscreen.
            wait: 20,
            mods: Modifiers::default(),
        })
    }

    pub fn feed(&mut self, raw: &mut RawInput) {
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(line) = self.lines.pop_front() else { return };
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| w.get(i).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        let has = |s: &str| w.iter().skip(1).any(|x| *x == s);
        let button = if has("right") { PointerButton::Secondary } else { PointerButton::Primary };
        let m = Modifiers {
            shift: has("shift"),
            ctrl: has("ctrl"),
            command: has("ctrl"),
            ..Default::default()
        };
        if m != self.mods {
            self.mods = m;
            raw.events.push(Event::ModifiersChanged(m));
        }
        match w[0] {
            "move" => raw.events.push(Event::PointerMoved(pos2(num(1), num(2)))),
            "down" | "up" => {
                let p = pos2(num(1), num(2));
                raw.events.push(Event::PointerMoved(p));
                raw.events.push(Event::PointerButton {
                    pos: p,
                    button,
                    pressed: w[0] == "down",
                    modifiers: m,
                });
            }
            "drag" => {
                let (a, b) = (pos2(num(1), num(2)), pos2(num(3), num(4)));
                let suffix: String = w.iter().skip(5).map(|s| format!(" {s}")).collect();
                let steps = 8;
                let mut seq = vec![format!("down {} {}{suffix}", a.x, a.y)];
                for i in 1..=steps {
                    let p = a + (b - a) * (i as f32 / steps as f32);
                    seq.push(format!("move {} {}{suffix}", p.x, p.y));
                }
                seq.push(format!("up {} {}{suffix}", b.x, b.y));
                for l in seq.into_iter().rev() {
                    self.lines.push_front(l);
                }
            }
            "key" => {
                if let Some(key) = w.get(1).and_then(|k| Key::from_name(k)) {
                    for pressed in [true, false] {
                        raw.events.push(Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: m,
                        });
                    }
                }
            }
            "text" => raw.events.push(Event::Text(line[5..].to_owned())),
            "wheel" => raw.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: vec2(0.0, num(1)),
                phase: egui::TouchPhase::Move,
                modifiers: m,
            }),
            "wait" => self.wait = num(1) as u32,
            other => eprintln!("fastshot script: unknown command {other}"),
        }
    }

    pub fn active(&self) -> bool {
        !self.lines.is_empty() || self.wait > 0
    }
}
