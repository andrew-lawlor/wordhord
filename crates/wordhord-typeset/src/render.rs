//! A page drawn to pixels, the way the device would: glyph outlines
//! rasterised with anti-aliasing, then quantised to the 16 greys an e-ink
//! panel shows. The preview, and the reference the device must match.

use std::collections::HashMap;
use std::path::Path;

use swash::scale::{image::Image, Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

use crate::fonts::Fonts;
use crate::page::Page;

pub struct Renderer<'a> {
    fonts: &'a Fonts,
    context: ScaleContext,
    cache: HashMap<(usize, u32, u16), Option<Image>>,
}

impl<'a> Renderer<'a> {
    pub fn new(fonts: &'a Fonts) -> Renderer<'a> {
        Renderer {
            fonts,
            context: ScaleContext::new(),
            cache: HashMap::new(),
        }
    }

    fn glyph(&mut self, font: usize, size: f32, id: u16) -> Option<&Image> {
        let key = (font, size.to_bits(), id);
        if !self.cache.contains_key(&key) {
            let data = self.fonts.files[font].data;
            let image = FontRef::from_index(data, 0).and_then(|f| {
                let mut scaler = self.context.builder(f).size(size).hint(false).build();
                Render::new(&[Source::Outline])
                    .format(Format::Alpha)
                    .render(&mut scaler, id)
            });
            self.cache.insert(key, image);
        }
        self.cache.get(&key).and_then(|i| i.as_ref())
    }

    /// The page as 8-bit grey pixels, row by row, in 16 levels.
    pub fn page(&mut self, page: &Page, width: u32, height: u32) -> Vec<u8> {
        let (w, h) = (width as i32, height as i32);
        let mut pixels = vec![255u8; (width * height) as usize];
        for run in &page.runs {
            for &(id, gx, gy) in &run.glyphs {
                let (x, y) = ((run.x + gx).round() as i32, (run.y + gy).round() as i32);
                let Some(image) = self.glyph(run.font, run.size, id) else {
                    continue;
                };
                let p = image.placement;
                for row in 0..p.height as i32 {
                    let py = y - p.top + row;
                    if !(0..h).contains(&py) {
                        continue;
                    }
                    for col in 0..p.width as i32 {
                        let px = x + p.left + col;
                        if !(0..w).contains(&px) {
                            continue;
                        }
                        let alpha = image.data[(row * p.width as i32 + col) as usize] as u32;
                        let i = (py * w + px) as usize;
                        pixels[i] = ((pixels[i] as u32 * (255 - alpha)) / 255) as u8;
                    }
                }
            }
        }
        // Sixteen greys: 0, 17, 34, … 255.
        for p in &mut pixels {
            *p = ((*p as u32 + 8) / 17 * 17).min(255) as u8;
        }
        pixels
    }
}

/// A note panel as the device draws it: the note's page under a rule
/// across the top.
pub fn panel(renderer: &mut Renderer, page: &Page, width: u32, height: u32) -> Vec<u8> {
    let mut pixels = renderer.page(page, width, height);
    for p in &mut pixels[..(width * 3) as usize] {
        *p = 0;
    }
    pixels
}

pub fn write_png(path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixels).map_err(|e| e.to_string())
}
