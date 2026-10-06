//! Touches and keys, from raw input events to what the reader does.
//!
//! A tap on the left third of the page goes back; anywhere else goes on.
//! Holding a finger down for over a second, or pressing the power button,
//! closes the reader.

/// One `struct input_event` as a 32-bit Kobo reads it: seconds and
/// microseconds as 32-bit words, then type, code and value.
pub const EVENT_SIZE: usize = 16;

const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_MT_POSITION_X: u16 = 53;
const ABS_MT_POSITION_Y: u16 = 54;
const ABS_MT_TRACKING_ID: u16 = 57;
const BTN_TOUCH: u16 = 330;
const KEY_POWER: u16 = 116;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Seconds, as a float: only differences matter.
    pub time: f64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

impl Event {
    pub fn parse(raw: &[u8; EVENT_SIZE]) -> Event {
        let word = |k: usize| u32::from_ne_bytes([raw[k], raw[k + 1], raw[k + 2], raw[k + 3]]);
        Event {
            time: word(0) as f64 + word(4) as f64 / 1e6,
            kind: u16::from_ne_bytes([raw[8], raw[9]]),
            code: u16::from_ne_bytes([raw[10], raw[11]]),
            value: i32::from_ne_bytes([raw[12], raw[13], raw[14], raw[15]]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Next,
    Previous,
    Close,
}

/// How the panel's axes sit on the screen, as FBInk knows them per model.
#[derive(Debug, Clone, Copy)]
pub struct Panel {
    pub width: u32,
    pub height: u32,
    pub swap: bool,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

impl Panel {
    /// A raw panel position on the screen.
    pub fn place(&self, raw_x: i32, raw_y: i32) -> (i32, i32) {
        let (mut x, mut y) = if self.swap {
            (raw_y, raw_x)
        } else {
            (raw_x, raw_y)
        };
        if self.mirror_x {
            x = self.width as i32 - 1 - x;
        }
        if self.mirror_y {
            y = self.height as i32 - 1 - y;
        }
        (x, y)
    }
}

/// Follows one finger from touch to lift.
#[derive(Debug, Default)]
pub struct Gestures {
    down: Option<f64>,
    raw: (i32, i32),
}

impl Gestures {
    /// Feeds an event from the touch screen; returns an action when a
    /// finger lifts.
    pub fn touch(&mut self, e: Event, panel: &Panel) -> Option<Action> {
        match (e.kind, e.code) {
            (EV_ABS, ABS_MT_POSITION_X | ABS_X) => {
                self.raw.0 = e.value;
                self.down.get_or_insert(e.time);
            }
            (EV_ABS, ABS_MT_POSITION_Y | ABS_Y) => {
                self.raw.1 = e.value;
                self.down.get_or_insert(e.time);
            }
            (EV_ABS, ABS_MT_TRACKING_ID) if e.value >= 0 => {
                self.down.get_or_insert(e.time);
            }
            (EV_KEY, BTN_TOUCH) if e.value == 1 => {
                self.down.get_or_insert(e.time);
            }
            (EV_ABS, ABS_MT_TRACKING_ID) | (EV_KEY, BTN_TOUCH) => {
                let start = self.down.take()?;
                if e.time - start > 1.0 {
                    return Some(Action::Close);
                }
                let (x, _) = panel.place(self.raw.0, self.raw.1);
                return Some(if x < panel.width as i32 / 3 {
                    Action::Previous
                } else {
                    Action::Next
                });
            }
            _ => {}
        }
        None
    }
}

/// A key press: the power button closes the reader.
pub fn key(e: Event) -> Option<Action> {
    (e.kind == EV_KEY && e.code == KEY_POWER && e.value == 1).then_some(Action::Close)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELIPSA: Panel = Panel {
        width: 1404,
        height: 1872,
        swap: true,
        mirror_x: false,
        mirror_y: true,
    };

    fn ev(time: f64, kind: u16, code: u16, value: i32) -> Event {
        Event {
            time,
            kind,
            code,
            value,
        }
    }

    fn tap(g: &mut Gestures, raw_x: i32, raw_y: i32, held: f64) -> Option<Action> {
        let mut out = None;
        for e in [
            ev(0.0, EV_ABS, ABS_MT_TRACKING_ID, 1),
            ev(0.0, EV_ABS, ABS_MT_POSITION_X, raw_x),
            ev(0.0, EV_ABS, ABS_MT_POSITION_Y, raw_y),
            ev(held, EV_ABS, ABS_MT_TRACKING_ID, -1),
        ] {
            out = g.touch(e, &ELIPSA).or(out);
        }
        out
    }

    #[test]
    fn a_raw_event_reads_as_a_32_bit_kobo_writes_it() {
        let mut raw = [0u8; EVENT_SIZE];
        raw[0..4].copy_from_slice(&10u32.to_ne_bytes());
        raw[4..8].copy_from_slice(&500_000u32.to_ne_bytes());
        raw[8..10].copy_from_slice(&EV_ABS.to_ne_bytes());
        raw[10..12].copy_from_slice(&ABS_MT_POSITION_X.to_ne_bytes());
        raw[12..16].copy_from_slice(&(-1i32).to_ne_bytes());
        assert_eq!(Event::parse(&raw), ev(10.5, EV_ABS, ABS_MT_POSITION_X, -1));
    }

    #[test]
    fn the_elipsas_panel_is_turned_a_quarter() {
        // Swapped, then mirrored in y: the panel's x runs up the screen.
        assert_eq!(ELIPSA.place(0, 0), (0, 1871));
        assert_eq!(ELIPSA.place(1871, 1403), (1403, 0));
    }

    #[test]
    fn taps_turn_pages_and_a_long_press_closes() {
        let mut g = Gestures::default();
        // Screen x comes from the panel's y.
        assert_eq!(tap(&mut g, 900, 100, 0.1), Some(Action::Previous));
        assert_eq!(tap(&mut g, 900, 1200, 0.1), Some(Action::Next));
        assert_eq!(tap(&mut g, 900, 1200, 1.5), Some(Action::Close));
    }

    #[test]
    fn the_power_button_closes() {
        assert_eq!(key(ev(0.0, EV_KEY, KEY_POWER, 1)), Some(Action::Close));
        assert_eq!(key(ev(0.0, EV_KEY, KEY_POWER, 0)), None);
    }
}
