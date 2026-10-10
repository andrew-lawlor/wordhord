//! A page drawn to pixels, the way the device would: glyph outlines
//! rasterised with anti-aliasing, then quantised to the 16 greys an e-ink
//! panel shows. The preview, and the reference the device must match.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use swash::scale::{image::Image, Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::FontRef;

use crate::fonts::Fonts;
use crate::page::Page;

/// Reads an image file's bytes by the source the book gives it.
pub type ReadImage<'a> = dyn Fn(&str) -> Option<Vec<u8>> + 'a;

pub struct Renderer<'a> {
    fonts: &'a Fonts,
    context: ScaleContext,
    cache: HashMap<(usize, u32, u16), Option<Image>>,
    /// An image file's bytes, by its source.
    images: Option<&'a ReadImage<'a>>,
    /// Images decoded, scaled and dithered, by source and size.
    pictures: HashMap<(String, u32, u32), Option<Vec<u8>>>,
    /// Where finished images are kept between runs.
    picture_dir: Option<PathBuf>,
}

impl<'a> Renderer<'a> {
    pub fn new(fonts: &'a Fonts) -> Renderer<'a> {
        Renderer {
            fonts,
            context: ScaleContext::new(),
            cache: HashMap::new(),
            images: None,
            pictures: HashMap::new(),
            picture_dir: None,
        }
    }

    /// Keep finished images in `dir`, so each is decoded, scaled and
    /// dithered once, not every time its page is drawn.
    pub fn with_picture_cache(mut self, dir: PathBuf) -> Renderer<'a> {
        self.picture_dir = Some(dir);
        self
    }

    /// Draw images, read by `bytes`.
    pub fn with_images(mut self, bytes: &'a ReadImage<'a>) -> Renderer<'a> {
        self.images = Some(bytes);
        self
    }

    /// An image in grey at `w`×`h`, dithered to the panel's 16 greys.
    fn picture(&mut self, src: &str, w: u32, h: u32) -> Option<&Vec<u8>> {
        let key = (src.to_string(), w, h);
        if !self.pictures.contains_key(&key) {
            let picture = self
                .images
                .and_then(|read| cached_picture(self.picture_dir.as_deref(), src, w, h, read));
            self.pictures.insert(key.clone(), picture);
        }
        self.pictures.get(&key).and_then(|p| p.as_ref())
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
        for r in &page.rules {
            let (x0, y0) = (r.x.round().max(0.0) as u32, r.y.round().max(0.0) as u32);
            let (x1, y1) = (
                ((r.x + r.width).round() as u32).min(width),
                ((r.y + r.height).round() as u32).min(height),
            );
            for y in y0..y1 {
                let row = (y * width) as usize;
                pixels[row + x0 as usize..row + x1.max(x0) as usize].fill(r.grey);
            }
        }
        // Images next, so text can sit on them.
        for img in &page.images {
            let (w, h) = (img.width.round() as u32, img.height.round() as u32);
            let (x0, y0) = (img.x.round() as i32, img.y.round() as i32);
            let Some(picture) = self.picture(&img.src, w, h) else {
                continue;
            };
            for row in 0..h as i32 {
                let py = y0 + row;
                if !(0..height as i32).contains(&py) {
                    continue;
                }
                for col in 0..w as i32 {
                    let px = x0 + col;
                    if (0..width as i32).contains(&px) {
                        pixels[(py * width as i32 + px) as usize] =
                            picture[(row * w as i32 + col) as usize];
                    }
                }
            }
        }
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
                        pixels[i] = ((pixels[i] as u32 * (255 - alpha) + run.grey as u32 * alpha)
                            / 255) as u8;
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

/// An image file as grey pixels at `w`×`h`: transparency on white,
/// scaled, then dithered to 16 greys (Floyd–Steinberg), so photographs
/// keep their tones on e-ink instead of banding.
pub fn picture(bytes: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    if w == 0 || h == 0 {
        return None;
    }
    let decoded = image::load_from_memory(bytes).ok()?.to_luma_alpha8();
    // Shrinking, the usual case, by averaging boxes of pixels: fast, and
    // smooth enough for 16 greys. Enlarging, bilinear.
    let scaled = if w <= decoded.width() && h <= decoded.height() {
        image::imageops::thumbnail(&decoded, w, h)
    } else {
        image::imageops::resize(&decoded, w, h, image::imageops::FilterType::Triangle)
    };
    let mut grey: Vec<f32> = scaled
        .pixels()
        .map(|p| {
            let [l, a] = p.0;
            let a = a as f32 / 255.0;
            l as f32 * a + 255.0 * (1.0 - a)
        })
        .collect();
    let (w, h) = (w as usize, h as usize);
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let old = grey[i].clamp(0.0, 255.0);
            let new = ((old / 17.0).round() * 17.0).clamp(0.0, 255.0);
            out[i] = new as u8;
            let err = old - new;
            if x + 1 < w {
                grey[i + 1] += err * 7.0 / 16.0;
            }
            if y + 1 < h {
                if x > 0 {
                    grey[i + w - 1] += err * 3.0 / 16.0;
                }
                grey[i + w] += err * 5.0 / 16.0;
                if x + 1 < w {
                    grey[i + w + 1] += err / 16.0;
                }
            }
        }
    }
    Some(out)
}

/// An image ready to draw at `w`×`h`: from `dir` if it was made before, or
/// made now from the file `read` gives, and kept there.
pub fn cached_picture(
    dir: Option<&Path>,
    src: &str,
    w: u32,
    h: u32,
    read: &ReadImage,
) -> Option<Vec<u8>> {
    let path = dir.map(|d| picture_path(d, src, w, h));
    if let Some(bytes) = path.as_ref().and_then(|p| std::fs::read(p).ok()) {
        if bytes.len() == (w * h) as usize {
            return Some(bytes);
        }
    }
    let made = picture(&read(src)?, w, h)?;
    if let Some(path) = path {
        // Written whole, then renamed, so no reader finds half of it.
        let partial = path.with_extension("part");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&partial, &made).is_ok() {
            let _ = std::fs::rename(&partial, &path);
        }
    }
    Some(made)
}

/// Where `cached_picture` keeps an image made at `w`×`h`.
pub fn picture_path(dir: &Path, src: &str, w: u32, h: u32) -> PathBuf {
    use sha2::{Digest, Sha256};
    let name: String = Sha256::digest(src.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    dir.join(format!("{name}-{w}x{h}.grey"))
}

/// An image's size in pixels, from its header, without decoding it.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

pub fn write_png(path: &Path, pixels: &[u8], width: u32, height: u32) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixels).map_err(|e| e.to_string())
}
