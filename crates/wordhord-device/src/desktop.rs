//! The reader on the desktop: the device's screen in a window, drawn at
//! half size by default. A click is a tap, a drag a swipe, and a press held
//! over a second a hold. The arrow keys turn pages; Escape closes.
//!
//! ```text
//! wordhord-reader <library> <app folder> [--scale N] [--screen 1404x1872]
//! wordhord-reader <library> <app folder> --script "tap 700 300, swipe left, hold" <frames>
//! ```
//!
//! With `--script`, there's no window: the gestures are played in order and
//! each screen drawn is written to `<frames>` as a PNG. That's for tests.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use minifb::{Key, KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};

use crate::app;
use crate::platform::Platform;
use crate::touch::Gesture;

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `--scan <root> <app folder>`: the library, as the reader would see it.
    if let [flag, root, app] = args.as_slice() {
        if flag == "--scan" {
            let library = crate::library::Library::scan(
                Path::new(root),
                &Path::new(app).join("library.json"),
            );
            for (name, books) in library.by_series() {
                println!(
                    "series {name}: {}",
                    books
                        .iter()
                        .map(|b| format!("{} {}", b.series_index.unwrap_or(0.0), b.title))
                        .collect::<Vec<_>>()
                        .join(" · ")
                );
            }
            let authors = library.by_author();
            println!(
                "{} authors; first: {:?}",
                authors.len(),
                authors
                    .iter()
                    .take(6)
                    .map(|(a, b)| (a, b.len()))
                    .collect::<Vec<_>>()
            );
            let tags = library.by_tag();
            println!(
                "{} tags; e.g. {:?}",
                tags.len(),
                tags.iter()
                    .take(8)
                    .map(|(t, b)| (t, b.len()))
                    .collect::<Vec<_>>()
            );
            println!(
                "titles: {:?}",
                library
                    .books
                    .iter()
                    .take(6)
                    .map(|b| &b.title)
                    .collect::<Vec<_>>()
            );
            println!(
                "covers: {} of {}",
                library.books.iter().filter(|b| b.cover.is_some()).count(),
                library.books.len()
            );
            return ExitCode::SUCCESS;
        }
    }
    if args.len() < 2 {
        eprintln!("usage: wordhord-reader <library> <app folder> [--scale N] [--screen WxH] [--script GESTURES FRAMES]");
        return ExitCode::from(2);
    }
    let (library, app) = (PathBuf::from(&args[0]), PathBuf::from(&args[1]));
    let mut scale = 2;
    let mut size = (1404, 1872);
    let mut script = None;
    let mut rest = args[2..].iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--scale", Some(n)) => scale = n.parse().unwrap_or(2).max(1),
            ("--screen", Some(s)) => {
                if let Some((w, h)) = s
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                {
                    size = (w, h);
                }
            }
            ("--script", Some(g)) => match rest.next() {
                Some(frames) => script = Some((g.clone(), PathBuf::from(frames))),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let result = match script {
        Some((gestures, frames)) => match Script::new(size, &gestures, &frames) {
            Ok(mut s) => app::run(&mut s, &library, &app),
            Err(e) => Err(e),
        },
        None => match Desktop::new(size, scale) {
            Ok(mut d) => app::run(&mut d, &library, &app),
            Err(e) => Err(e),
        },
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: wordhord-reader <library> <app folder> [--scale N] [--screen WxH] [--script GESTURES FRAMES]");
    ExitCode::from(2)
}

/// A window showing the device's screen, `scale` times smaller.
struct Desktop {
    window: Window,
    size: (u32, u32),
    scale: usize,
    /// The device's screen, in grey.
    screen: Vec<u8>,
    /// Where the button went down, on the device's screen, and when.
    down: Option<(Instant, (i32, i32))>,
    /// Held still a second: pressed, and where it last dragged to; and
    /// whether it has.
    pressed: Option<(i32, i32)>,
    dragged: bool,
}

impl Desktop {
    fn new(size: (u32, u32), scale: usize) -> Result<Desktop, String> {
        let window = Window::new(
            "Wordhord",
            size.0 as usize / scale,
            size.1 as usize / scale,
            WindowOptions::default(),
        )
        .map_err(|e| format!("can't open a window: {e}"))?;
        Ok(Desktop {
            window,
            size,
            scale,
            screen: vec![255; (size.0 * size.1) as usize],
            down: None,
            pressed: None,
            dragged: false,
        })
    }

    /// The device's screen, shrunk by averaging, onto the window.
    fn show(&mut self, invert: Option<(u32, u32, u32, u32)>) {
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        let s = self.scale;
        let (ow, oh) = (w / s, h / s);
        let mut out = vec![0u32; ow * oh];
        for y in 0..oh {
            for x in 0..ow {
                let mut sum = 0u32;
                for dy in 0..s {
                    for dx in 0..s {
                        sum += self.screen[(y * s + dy) * w + x * s + dx] as u32;
                    }
                }
                let mut g = sum / (s * s) as u32;
                if let Some((x0, y0, rw, rh)) = invert {
                    let (px, py) = ((x * s) as u32, (y * s) as u32);
                    if px >= x0 && px < x0 + rw && py >= y0 && py < y0 + rh {
                        g = 255 - g;
                    }
                }
                out[y * ow + x] = (g << 16) | (g << 8) | g;
            }
        }
        let _ = self.window.update_with_buffer(&out, ow, oh);
    }
}

impl Platform for Desktop {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn draw(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, flash: bool) {
        blit(&mut self.screen, self.size.0, grey, w, h, x, y);
        // A flash, as e-ink does it: the region goes dark, then clears.
        if flash {
            self.show(Some((x, y, w, h)));
            std::thread::sleep(Duration::from_millis(120));
        }
        self.show(None);
    }

    fn next(&mut self) -> Gesture {
        loop {
            if !self.window.is_open() {
                return Gesture::Power;
            }
            // One press, one page; and no keys at all without focus, where
            // a key can look held down forever.
            let keys = if self.window.is_active() {
                self.window.get_keys_pressed(KeyRepeat::No)
            } else {
                Vec::new()
            };
            for key in keys {
                match key {
                    Key::Right | Key::PageDown | Key::Space => return Gesture::SwipeLeft,
                    Key::Left | Key::PageUp => return Gesture::SwipeRight,
                    Key::Escape | Key::Q => return Gesture::Power,
                    _ => {}
                }
            }
            let at = self.window.get_mouse_pos(MouseMode::Clamp).map(|(x, y)| {
                let s = self.scale as f32;
                ((x * s) as i32, (y * s) as i32)
            });
            let pressed = self.window.get_mouse_down(MouseButton::Left);
            match (pressed, self.down, at) {
                (true, None, Some(at)) => self.down = Some((Instant::now(), at)),
                (true, Some((when, from)), Some(at)) => match self.pressed {
                    None => {
                        let still = (at.0 - from.0).abs().max((at.1 - from.1).abs())
                            <= self.size.0 as i32 / 40;
                        if still && when.elapsed() >= Duration::from_secs(1) {
                            self.pressed = Some(from);
                            return Gesture::Press(from.0, from.1);
                        }
                    }
                    Some(last) if (at.0 - last.0).abs().max((at.1 - last.1).abs()) >= 10 => {
                        self.pressed = Some(at);
                        self.dragged = true;
                        return Gesture::Drag(at.0, at.1);
                    }
                    _ => {}
                },
                (false, Some((when, from)), Some(to)) => {
                    self.down = None;
                    let dragged = std::mem::take(&mut self.dragged);
                    if self.pressed.take().is_some() {
                        return if dragged {
                            Gesture::Release(to.0, to.1)
                        } else {
                            Gesture::Hold(from.0, from.1)
                        };
                    }
                    return gesture(from, to, when.elapsed(), self.size);
                }
                _ => {}
            }
            self.window.update();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A press and release as the touch screen would read them.
fn gesture(
    from: (i32, i32),
    to: (i32, i32),
    held: Duration,
    (width, height): (u32, u32),
) -> Gesture {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    if let Some(g) = crate::touch::slide(from, to, height) {
        return g;
    }
    if dx.abs() >= width as i32 / 8 && dx.abs() > 2 * dy.abs() {
        return if dx < 0 {
            Gesture::SwipeLeft
        } else {
            Gesture::SwipeRight
        };
    }
    if held > Duration::from_secs(1) {
        return Gesture::Hold(from.0, from.1);
    }
    Gesture::Tap(from.0, from.1)
}

fn blit(screen: &mut [u8], stride: u32, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
    for row in 0..h as usize {
        let dst = (y as usize + row) * stride as usize + x as usize;
        let src = row * w as usize;
        if dst + w as usize <= screen.len() && src + w as usize <= grey.len() {
            screen[dst..dst + w as usize].copy_from_slice(&grey[src..src + w as usize]);
        }
    }
}

/// Gestures from a script, and every screen drawn saved as a PNG.
struct Script {
    size: (u32, u32),
    screen: Vec<u8>,
    gestures: std::vec::IntoIter<Gesture>,
    frames: PathBuf,
    drawn: usize,
}

impl Script {
    fn new(size: (u32, u32), script: &str, frames: &Path) -> Result<Script, String> {
        let mut gestures = Vec::new();
        for step in script.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let words: Vec<&str> = step.split_whitespace().collect();
            let g = match words.as_slice() {
                ["tap", x, y] => Gesture::Tap(
                    x.parse().map_err(|_| format!("`{step}`"))?,
                    y.parse().map_err(|_| format!("`{step}`"))?,
                ),
                ["swipe", "left"] => Gesture::SwipeLeft,
                ["swipe", "right"] => Gesture::SwipeRight,
                // Held in the corner, where there's no word.
                ["hold"] => Gesture::Hold(0, 0),
                ["hold", x, y] => Gesture::Hold(
                    x.parse().map_err(|_| format!("`{step}`"))?,
                    y.parse().map_err(|_| format!("`{step}`"))?,
                ),
                ["power"] => Gesture::Power,
                ["press", x, y] => Gesture::Press(
                    x.parse().map_err(|_| format!("`{step}`"))?,
                    y.parse().map_err(|_| format!("`{step}`"))?,
                ),
                // Pressed at one place, dragged to another and let go.
                ["drag", x0, y0, x1, y1] => {
                    let n = |v: &str| v.parse::<i32>().map_err(|_| format!("`{step}`"));
                    let (to_x, to_y) = (n(x1)?, n(y1)?);
                    gestures.push(Gesture::Press(n(x0)?, n(y0)?));
                    gestures.push(Gesture::Drag(to_x, to_y));
                    Gesture::Release(to_x, to_y)
                }
                ["slide", x, dy] => Gesture::Slide(
                    x.parse().map_err(|_| format!("`{step}`"))?,
                    dy.parse().map_err(|_| format!("`{step}`"))?,
                ),
                _ => {
                    return Err(format!(
                    "`{step}` isn't a gesture: tap X Y, swipe left, swipe right, hold [X Y], slide X DY, drag X0 Y0 X1 Y1, power"
                ))
                }
            };
            gestures.push(g);
        }
        std::fs::create_dir_all(frames).map_err(|e| format!("{}: {e}", frames.display()))?;
        Ok(Script {
            size,
            screen: vec![255; (size.0 * size.1) as usize],
            gestures: gestures.into_iter(),
            frames: frames.to_path_buf(),
            drawn: 0,
        })
    }
}

impl Platform for Script {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn draw(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, _flash: bool) {
        blit(&mut self.screen, self.size.0, grey, w, h, x, y);
        self.drawn += 1;
        let path = self.frames.join(format!("frame-{:03}.png", self.drawn));
        if let Err(e) =
            wordhord_typeset::render::write_png(&path, &self.screen, self.size.0, self.size.1)
        {
            eprintln!("{e}");
        }
    }

    fn next(&mut self) -> Gesture {
        self.gestures.next().unwrap_or(Gesture::Power)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mouse_makes_the_touch_screens_gestures() {
        let quick = Duration::from_millis(100);
        assert_eq!(
            gesture((700, 300), (702, 301), quick, (1404, 1872)),
            Gesture::Tap(700, 300)
        );
        assert_eq!(
            gesture((1000, 900), (500, 920), quick, (1404, 1872)),
            Gesture::SwipeLeft
        );
        assert_eq!(
            gesture((500, 900), (1000, 900), quick, (1404, 1872)),
            Gesture::SwipeRight
        );
        assert_eq!(
            gesture((700, 300), (700, 300), Duration::from_secs(2), (1404, 1872)),
            Gesture::Hold(700, 300)
        );
        assert_eq!(
            gesture((300, 1200), (310, 900), quick, (1404, 1872)),
            Gesture::Slide(300, -300)
        );
    }
}
