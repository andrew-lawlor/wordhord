//! Touches and keys, from raw input events to gestures.
//!
//! A finger that lifts close to where it landed is a tap. One that moved
//! far sideways is a swipe; far up or down, a slide. One held still for a
//! second is pressed: then, while it's down, its moves are drags, and it
//! lifts as a release, or as a hold if it never moved. The power button
//! is a key.

/// One `struct input_event` as a 32-bit Kobo reads it: seconds and
/// microseconds as 32-bit words, then type, code and value.
pub const EVENT_SIZE: usize = 16;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_MT_POSITION_X: u16 = 53;
const ABS_MT_POSITION_Y: u16 = 54;
const ABS_MT_TRACKING_ID: u16 = 57;
const ABS_MT_TOOL_TYPE: u16 = 0x37;
const ABS_MT_PRESSURE: u16 = 0x3a;
const ABS_MT_DISTANCE: u16 = 0x3b;
const BTN_TOUCH: u16 = 330;
/// The stylus's other end, near the screen: an eraser.
const BTN_STYLUS: u16 = 0x14b;
/// The stylus's button, held as it touches: a highlighter.
const BTN_STYLUS2: u16 = 0x14c;
const KEY_POWER: u16 = 116;

/// Held this long, a touch is a hold, not a tap.
const HOLD_SECONDS: f64 = 1.0;

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
pub enum Gesture {
    /// Where on the screen.
    Tap(i32, i32),
    /// The finger moved left: on to the next page.
    SwipeLeft,
    /// The finger moved right: back to the previous page.
    SwipeRight,
    /// A finger held down a second, where it landed.
    Hold(i32, i32),
    /// A finger slid up or down: where across it was, and how far down it
    /// went (negative: up).
    Slide(i32, i32),
    /// A finger held still a second, and still down, where it landed.
    Press(i32, i32),
    /// A pressed finger, moved to here.
    Drag(i32, i32),
    /// A pressed finger that moved, lifted here.
    Release(i32, i32),
    /// The pen writing, at its latest point: the points since the last are
    /// the platform's, to take.
    Ink(i32, i32),
    /// The pen lifted from a stroke, here.
    InkUp(i32, i32),
    Power,
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

/// A pressed finger's moves are passed on once it has gone this far, in
/// pixels, since the last.
const DRAG_STEP: i32 = 10;

/// Follows one finger from touch to lift.
#[derive(Debug, Default)]
pub struct Gestures {
    down: Option<f64>,
    raw: (i32, i32),
    /// Where the finger landed, on screen, once its first position is in.
    start: Option<(i32, i32)>,
    /// When it landed, by the clock here, to tell a press while it's down.
    landed: Option<std::time::Instant>,
    /// It has moved too far from where it landed to be pressed.
    wandered: bool,
    /// Pressed: and where its last drag was, if it has moved since.
    pressed: Option<(i32, i32)>,
    dragged: bool,
    /// The tool the touch screen says is touching: 0 a finger, 1 the pen.
    tool: i32,
    pressure: i32,
    distance: i32,
    /// The pen on the screen: where it touched, and the stroke it's
    /// drawing, once it has moved far enough to be one.
    pen: Option<Pen>,
    /// Whether the pen writes here (in a book). Elsewhere it does nothing.
    pub ink: bool,
    /// When the pen was last near the screen: fingers are ignored while
    /// it is, and a little after, so a hand that writes can rest.
    pen_near: Option<std::time::Instant>,
    /// Points of ink not yet taken: screen position and pressure.
    pub points: Vec<(i32, i32, u16)>,
    /// The ends and button of the stylus, as last reported; and the tool
    /// the stroke being drawn was begun with.
    eraser: bool,
    button: bool,
    pub stroke_tool: Tool,
}

#[derive(Debug)]
struct Pen {
    /// Whether it writes: it touched in a book.
    stroke: bool,
}

/// What the pen does: writes, rubs out, or underlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Pen,
    Eraser,
    Highlighter,
}

/// Fingers are ignored for this long after the pen was near the screen.
const PALM_SECONDS: f64 = 1.0;

impl Gestures {
    /// Feeds an event from the touch screen; returns a gesture when a
    /// finger lifts.
    pub fn touch(&mut self, e: Event, panel: &Panel) -> Option<Gesture> {
        match (e.kind, e.code) {
            (EV_ABS, ABS_MT_TOOL_TYPE) => {
                // A pen now: whatever a finger had begun is forgotten.
                if e.value != 0 && self.tool == 0 {
                    self.down = None;
                    self.start = None;
                    self.landed = None;
                    self.pressed = None;
                    self.dragged = false;
                    self.wandered = false;
                }
                self.tool = e.value;
                return None;
            }
            (EV_ABS, ABS_MT_PRESSURE) => {
                self.pressure = e.value;
                return None;
            }
            (EV_ABS, ABS_MT_DISTANCE) => {
                self.distance = e.value;
                return None;
            }
            (EV_KEY, BTN_STYLUS) => {
                self.eraser = e.value != 0;
                return None;
            }
            (EV_KEY, BTN_STYLUS2) => {
                self.button = e.value != 0;
                return None;
            }
            _ => {}
        }
        if self.tool != 0 {
            self.pen_near = Some(std::time::Instant::now());
            return self.pen(e, panel);
        }
        // The pen near, or only just gone: this is the hand that holds it.
        if self
            .pen_near
            .is_some_and(|t| t.elapsed().as_secs_f64() < PALM_SECONDS)
        {
            self.down = None;
            self.start = None;
            self.landed = None;
            self.pressed = None;
            self.dragged = false;
            self.wandered = false;
            return None;
        }
        self.finger(e, panel)
    }

    /// The pen: touching when it's at no distance and pressing. Hovering is
    /// nothing. In a book, every touch is ink, a full stop as much as a
    /// line; elsewhere the pen does nothing.
    fn pen(&mut self, e: Event, panel: &Panel) -> Option<Gesture> {
        match (e.kind, e.code) {
            (EV_ABS, ABS_MT_POSITION_X | ABS_X) => self.raw.0 = e.value,
            (EV_ABS, ABS_MT_POSITION_Y | ABS_Y) => self.raw.1 = e.value,
            (EV_SYN, _) => {
                let at = panel.place(self.raw.0, self.raw.1);
                let touching = self.distance == 0 && self.pressure > 0;
                let point = (at.0, at.1, self.pressure.clamp(0, 4095) as u16);
                match (&self.pen, touching) {
                    (None, true) => {
                        self.pen = Some(Pen { stroke: self.ink });
                        self.stroke_tool = if self.eraser {
                            Tool::Eraser
                        } else if self.button {
                            Tool::Highlighter
                        } else {
                            Tool::Pen
                        };
                        if self.ink {
                            self.points.push(point);
                            return Some(Gesture::Ink(at.0, at.1));
                        }
                    }
                    (Some(p), true) if p.stroke => {
                        self.points.push(point);
                        return Some(Gesture::Ink(at.0, at.1));
                    }
                    (Some(_), false) => return self.pen_up(at),
                    _ => {}
                }
            }
            (EV_ABS, ABS_MT_TRACKING_ID) if e.value < 0 => {
                let at = panel.place(self.raw.0, self.raw.1);
                return self.pen_up(at);
            }
            _ => {}
        }
        None
    }

    fn pen_up(&mut self, at: (i32, i32)) -> Option<Gesture> {
        let p = self.pen.take()?;
        p.stroke.then_some(Gesture::InkUp(at.0, at.1))
    }

    fn finger(&mut self, e: Event, panel: &Panel) -> Option<Gesture> {
        match (e.kind, e.code) {
            (EV_ABS, ABS_MT_POSITION_X | ABS_X) => {
                self.raw.0 = e.value;
                self.down.get_or_insert(e.time);
            }
            (EV_ABS, ABS_MT_POSITION_Y | ABS_Y) => {
                self.raw.1 = e.value;
                self.down.get_or_insert(e.time);
            }
            (EV_SYN, _) if self.down.is_some() => {
                let at = panel.place(self.raw.0, self.raw.1);
                let Some(start) = self.start else {
                    self.start = Some(at);
                    self.landed = Some(std::time::Instant::now());
                    return None;
                };
                let far = panel.width as i32 / 40;
                if (at.0 - start.0).abs().max((at.1 - start.1).abs()) > far {
                    self.wandered = true;
                }
                if let Some(last) = self.pressed {
                    if (at.0 - last.0).abs().max((at.1 - last.1).abs()) >= DRAG_STEP {
                        self.pressed = Some(at);
                        self.dragged = true;
                        return Some(Gesture::Drag(at.0, at.1));
                    }
                }
            }
            (EV_ABS, ABS_MT_TRACKING_ID) if e.value >= 0 => {
                self.down.get_or_insert(e.time);
            }
            (EV_KEY, BTN_TOUCH) if e.value == 1 => {
                self.down.get_or_insert(e.time);
            }
            (EV_ABS, ABS_MT_TRACKING_ID) | (EV_KEY, BTN_TOUCH) => {
                let landed = self.down.take()?;
                let end = panel.place(self.raw.0, self.raw.1);
                let start = self.start.take().unwrap_or(end);
                self.landed = None;
                self.wandered = false;
                let dragged = std::mem::take(&mut self.dragged);
                if self.pressed.take().is_some() {
                    return Some(if dragged {
                        Gesture::Release(end.0, end.1)
                    } else {
                        Gesture::Hold(start.0, start.1)
                    });
                }
                let (dx, dy) = (end.0 - start.0, end.1 - start.1);
                // A swipe crosses an eighth of the screen, mostly sideways.
                if dx.abs() >= panel.width as i32 / 8 && dx.abs() > 2 * dy.abs() {
                    return Some(if dx < 0 {
                        Gesture::SwipeLeft
                    } else {
                        Gesture::SwipeRight
                    });
                }
                if let Some(g) = slide(start, end, panel.height) {
                    return Some(g);
                }
                if e.time - landed > HOLD_SECONDS {
                    return Some(Gesture::Hold(start.0, start.1));
                }
                return Some(Gesture::Tap(start.0, start.1));
            }
            _ => {}
        }
        None
    }
}

impl Gestures {
    /// How long to wait for a finger that's down to become pressed, if it
    /// still might.
    pub fn press_due(&self) -> Option<std::time::Duration> {
        if self.pressed.is_some() || self.wandered || self.start.is_none() {
            return None;
        }
        let held = std::time::Duration::from_secs_f64(HOLD_SECONDS);
        Some(held.saturating_sub(self.landed?.elapsed()))
    }

    /// Called when a finger has been down a while: pressed, if it's been
    /// held still long enough.
    pub fn tick(&mut self) -> Option<Gesture> {
        if self.press_due()? > std::time::Duration::ZERO {
            return None;
        }
        let start = self.start?;
        self.pressed = Some(start);
        Some(Gesture::Press(start.0, start.1))
    }
}

/// A slide crosses a sixteenth of the screen, mostly up or down.
pub fn slide(start: (i32, i32), end: (i32, i32), height: u32) -> Option<Gesture> {
    let (dx, dy) = (end.0 - start.0, end.1 - start.1);
    (dy.abs() >= height as i32 / 16 && dy.abs() > 2 * dx.abs())
        .then_some(Gesture::Slide(start.0, dy))
}

/// A key press: the power button.
pub fn key(e: Event) -> Option<Gesture> {
    (e.kind == EV_KEY && e.code == KEY_POWER && e.value == 1).then_some(Gesture::Power)
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

    /// A finger down at one raw position, up at another.
    fn touch(g: &mut Gestures, from: (i32, i32), to: (i32, i32), held: f64) -> Option<Gesture> {
        let mut out = None;
        for e in [
            ev(0.0, EV_ABS, ABS_MT_TRACKING_ID, 1),
            ev(0.0, EV_ABS, ABS_MT_POSITION_X, from.0),
            ev(0.0, EV_ABS, ABS_MT_POSITION_Y, from.1),
            ev(0.0, EV_SYN, 0, 0),
            ev(held, EV_ABS, ABS_MT_POSITION_X, to.0),
            ev(held, EV_ABS, ABS_MT_POSITION_Y, to.1),
            ev(held, EV_SYN, 0, 0),
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
    fn taps_swipes_and_holds() {
        let mut g = Gestures::default();
        // Screen x comes from the panel's y; screen y is the panel's x, upside down.
        assert_eq!(
            touch(&mut g, (900, 100), (905, 102), 0.1),
            Some(Gesture::Tap(100, 971))
        );
        assert_eq!(
            touch(&mut g, (900, 1200), (900, 400), 0.3),
            Some(Gesture::SwipeLeft)
        );
        assert_eq!(
            touch(&mut g, (900, 400), (950, 1200), 0.3),
            Some(Gesture::SwipeRight)
        );
        assert_eq!(
            touch(&mut g, (900, 700), (900, 700), 1.5),
            Some(Gesture::Hold(700, 971))
        );
    }

    #[test]
    fn a_pressed_finger_drags_and_releases() {
        let mut g = Gestures::default();
        let feed = |e: Event, g: &mut Gestures| g.touch(e, &ELIPSA);
        feed(ev(0.0, EV_ABS, ABS_MT_TRACKING_ID, 1), &mut g);
        feed(ev(0.0, EV_ABS, ABS_MT_POSITION_X, 900), &mut g);
        feed(ev(0.0, EV_ABS, ABS_MT_POSITION_Y, 700), &mut g);
        feed(ev(0.0, EV_SYN, 0, 0), &mut g);
        assert!(g.press_due().is_some());
        // As if a second had gone by.
        g.landed = Some(std::time::Instant::now() - std::time::Duration::from_secs(2));
        assert_eq!(g.tick(), Some(Gesture::Press(700, 971)));
        assert_eq!(g.press_due(), None);
        feed(ev(1.2, EV_ABS, ABS_MT_POSITION_Y, 900), &mut g);
        assert_eq!(
            feed(ev(1.2, EV_SYN, 0, 0), &mut g),
            Some(Gesture::Drag(900, 971))
        );
        assert_eq!(
            feed(ev(1.4, EV_ABS, ABS_MT_TRACKING_ID, -1), &mut g),
            Some(Gesture::Release(900, 971))
        );
        // The next touch starts afresh.
        assert_eq!(
            touch(&mut g, (900, 100), (905, 102), 0.1),
            Some(Gesture::Tap(100, 971))
        );
    }

    #[test]
    fn the_power_button_is_a_key() {
        assert_eq!(key(ev(0.0, EV_KEY, KEY_POWER, 1)), Some(Gesture::Power));
        assert_eq!(key(ev(0.0, EV_KEY, KEY_POWER, 0)), None);
    }
}
