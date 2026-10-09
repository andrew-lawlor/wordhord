//! The front light: how bright, and how warm. Nickel is paused while
//! Wordhord runs, so Wordhord sets the light itself, through the same
//! files in /sys KOReader writes to. On exit it puts the light back as
//! Nickel left it, so Nickel's own setting stays true.
//!
//! The paths are the Elipsa 2E's and the Libra Colour's. A Kobo without them has
//! no light here.

use std::path::{Path, PathBuf};

/// Brightness, 0 to 100.
const WHITE: &str = "backlight/mxc_msp430.0/brightness";
/// The warm/cool mixer, 0 to 10, with 10 the coolest: the Elipsa 2E's,
/// then the Libra Colour's, as KOReader has them.
const MIXERS: [&str; 2] = ["leds/aw99703-bl_FL1/color", "backlight/lm3630a_led/color"];
const MIXER_MAX: i32 = 10;
/// Nickel's colour setting runs in kelvin, warmest to coolest.
const KELVIN: (i32, i32) = (1500, 6400);

pub struct Light {
    white: PathBuf,
    mixer: Option<PathBuf>,
    brightness: i32,
    /// 0, coolest, to 10, warmest.
    warmth: Option<i32>,
    /// As Nickel left it.
    was: (i32, Option<i32>),
}

fn read(path: &Path) -> Option<i32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn write(path: &Path, value: i32) {
    if let Err(e) = std::fs::write(path, value.to_string()) {
        eprintln!("light: {}: {e}", path.display());
    }
}

/// A setting from Nickel's own configuration, under [PowerOptions].
fn nickel(conf: &str, key: &str) -> Option<i32> {
    conf.lines()
        .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
        .and_then(|v| v.trim().parse().ok())
}

impl Light {
    /// The light under `class` (/sys/class on a Kobo), reading its levels
    /// from there or, where the files don't say, from Nickel's `conf`.
    pub fn find(class: &Path, conf: &Path) -> Option<Light> {
        let white = class.join(WHITE);
        if !white.exists() {
            return None;
        }
        let conf = std::fs::read_to_string(conf).unwrap_or_default();
        let brightness = read(&white)
            .or_else(|| nickel(&conf, "FrontLightLevel"))
            .unwrap_or(0)
            .clamp(0, 100);
        let mixer = MIXERS.iter().map(|m| class.join(m)).find(|m| m.exists());
        let warmth = mixer.as_ref().map(|m| {
            read(m)
                .map(|v| MIXER_MAX - v)
                .or_else(|| {
                    let k = nickel(&conf, "ColorSetting")?;
                    let share = (KELVIN.1 - k) as f32 / (KELVIN.1 - KELVIN.0) as f32;
                    Some((share * MIXER_MAX as f32).round() as i32)
                })
                .unwrap_or(0)
                .clamp(0, MIXER_MAX)
        });
        Some(Light {
            white,
            mixer,
            brightness,
            warmth,
            was: (brightness, warmth),
        })
    }

    pub fn brightness(&self) -> i32 {
        self.brightness
    }

    pub fn warmth(&self) -> Option<i32> {
        self.warmth
    }

    pub fn set_brightness(&mut self, to: i32) {
        self.brightness = to.clamp(0, 100);
        write(&self.white, self.brightness);
    }

    pub fn set_warmth(&mut self, to: i32) {
        if let Some(m) = &self.mixer {
            let w = to.clamp(0, MIXER_MAX);
            self.warmth = Some(w);
            write(m, MIXER_MAX - w);
        }
    }

    /// Brighter or dimmer, warmer or cooler, by a share of the screen's
    /// height a finger slid up (negative: down). The whole height is the
    /// whole range.
    pub fn slide(&mut self, warm: bool, by: f32) {
        if warm {
            if let Some(w) = self.warmth {
                let step = (by * MIXER_MAX as f32).round() as i32;
                // Any slide moves it at least one step.
                let step = if step == 0 { by.signum() as i32 } else { step };
                self.set_warmth(w + step);
            }
        } else {
            self.set_brightness(self.brightness + (by * 100.0).round() as i32);
        }
    }

    pub fn max_warmth(&self) -> i32 {
        MIXER_MAX
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        let (b, w) = self.was;
        if self.brightness != b {
            write(&self.white, b);
        }
        if let (Some(w), Some(m)) = (w, &self.mixer) {
            if self.warmth != Some(w) {
                write(m, MIXER_MAX - w);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_set_the_files_and_exit_puts_them_back() {
        let dir = std::env::temp_dir().join(format!("wordhord-light-{}", std::process::id()));
        let class = dir.join("class");
        std::fs::create_dir_all(class.join("backlight/mxc_msp430.0")).unwrap();
        std::fs::create_dir_all(class.join("leds/aw99703-bl_FL1")).unwrap();
        std::fs::write(class.join(WHITE), "19\n").unwrap();
        std::fs::write(class.join(MIXERS[0]), "").unwrap();
        // The mixer can't be read back: Nickel's 6400 K is the coolest.
        std::fs::write(dir.join("conf"), "[PowerOptions]\nColorSetting=6400\n").unwrap();
        {
            let mut l = Light::find(&class, &dir.join("conf")).unwrap();
            assert_eq!((l.brightness(), l.warmth()), (19, Some(0)));
            l.slide(false, 0.25);
            l.slide(true, 0.02);
            assert_eq!(read(&class.join(WHITE)), Some(44));
            assert_eq!(read(&class.join(MIXERS[0])), Some(9));
            l.slide(false, -2.0);
            assert_eq!(l.brightness(), 0);
        }
        assert_eq!(read(&class.join(WHITE)), Some(19));
        assert_eq!(read(&class.join(MIXERS[0])), Some(10));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
