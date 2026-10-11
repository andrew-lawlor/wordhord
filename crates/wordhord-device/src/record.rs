//! Recording a session, for a demonstration: every change to the screen,
//! every gesture and every point of the pen, each with when it came, as
//! the reader used it. `scripts/replay.py` makes a video of it at the
//! speed it happened. On while a file named `record` is in the app
//! folder; each session goes to `recordings/`, named by when it began.
//!
//! The file: `WHREC1\n`, the screen's width and height (u32), then
//! records, all little-endian, each starting with its time in
//! microseconds (u64) and a letter:
//!
//! - `D`: a draw. x, y, w, h (u32), how (u8: 0 drawn, 1 with a flash,
//!   2 fast, 3 ink), then its pixels, deflated, with their length (u32).
//! - `G`: a gesture, as Rust writes it (`Tap(600, 30)`), its length (u16)
//!   first.
//! - `P`: the pen's points since last asked: how many (u32), then each
//!   x, y (i32) and pressure (u16).
//!
//! The writing is done by a thread of its own, so recording doesn't slow
//! the reader and change the very timing it records.

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use crate::platform::Platform;
use crate::touch::{Gesture, Tool};

/// A record's start, and pixels to deflate and add, if a draw's.
type Record = (Vec<u8>, Option<Vec<u8>>);

pub struct Recorder<'a> {
    inner: &'a mut dyn Platform,
    started: Instant,
    /// To the writer, which does the deflating, off the reader's thread.
    out: Option<Sender<Record>>,
    writer: Option<JoinHandle<()>>,
}

impl<'a> Recorder<'a> {
    /// A recorder of `inner`, if the app folder asks for one.
    pub fn wanted(app: &Path) -> bool {
        app.join("record").exists()
    }

    pub fn new(inner: &'a mut dyn Platform, app: &Path) -> Result<Recorder<'a>, String> {
        let dir = app.join("recordings");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = format!("{}.whrec", crate::hoard::now());
        let path = dir.join(name);
        let mut file =
            std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (w, h) = inner.size();
        let mut head = b"WHREC1\n".to_vec();
        head.extend(w.to_le_bytes());
        head.extend(h.to_le_bytes());
        file.write_all(&head).map_err(|e| e.to_string())?;
        let (tx, rx) = channel::<Record>();
        let writer = std::thread::spawn(move || {
            let mut file = std::io::BufWriter::new(file);
            for (mut record, pixels) in rx {
                if let Some(pixels) = pixels {
                    let packed = miniz_oxide::deflate::compress_to_vec(&pixels, 1);
                    record.extend((packed.len() as u32).to_le_bytes());
                    record.extend(packed);
                }
                if file.write_all(&record).is_err() {
                    break;
                }
            }
            let _ = file.flush();
        });
        eprintln!("record: {}", path.display());
        Ok(Recorder {
            inner,
            started: Instant::now(),
            out: Some(tx),
            writer: Some(writer),
        })
    }

    fn record(&self, kind: u8, body: &[u8], pixels: Option<Vec<u8>>) {
        let mut r = Vec::with_capacity(9 + body.len());
        r.extend((self.started.elapsed().as_micros() as u64).to_le_bytes());
        r.push(kind);
        r.extend(body);
        if let Some(out) = &self.out {
            let _ = out.send((r, pixels));
        }
    }

    fn drawn(&self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, how: u8) {
        let mut body = Vec::with_capacity(17);
        for v in [x, y, w, h] {
            body.extend(v.to_le_bytes());
        }
        body.push(how);
        self.record(b'D', &body, Some(grey.to_vec()));
    }
}

impl Drop for Recorder<'_> {
    fn drop(&mut self) {
        // Everything sent is written before the reader goes.
        self.out.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
    }
}

impl Platform for Recorder<'_> {
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    fn draw(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, flash: bool) {
        self.inner.draw(grey, w, h, x, y, flash);
        self.drawn(grey, w, h, x, y, flash as u8);
    }

    fn draw_fast(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
        self.inner.draw_fast(grey, w, h, x, y);
        self.drawn(grey, w, h, x, y, 2);
    }

    fn draw_ink(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
        self.inner.draw_ink(grey, w, h, x, y);
        self.drawn(grey, w, h, x, y, 3);
    }

    fn ink(&mut self, on: bool) {
        self.inner.ink(on);
    }

    fn take_ink(&mut self) -> Vec<(i32, i32, u16)> {
        let points = self.inner.take_ink();
        if !points.is_empty() {
            let mut body = Vec::with_capacity(4 + points.len() * 10);
            body.extend((points.len() as u32).to_le_bytes());
            for (x, y, p) in &points {
                body.extend(x.to_le_bytes());
                body.extend(y.to_le_bytes());
                body.extend(p.to_le_bytes());
            }
            self.record(b'P', &body, None);
        }
        points
    }

    fn tool(&self) -> Tool {
        self.inner.tool()
    }

    fn battery(&self) -> Option<(u8, bool)> {
        self.inner.battery()
    }

    fn next(&mut self) -> Gesture {
        let g = self.inner.next();
        let said = format!("{g:?}");
        let mut body = (said.len() as u16).to_le_bytes().to_vec();
        body.extend(said.as_bytes());
        self.record(b'G', &body, None);
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Blank;

    impl Platform for Blank {
        fn size(&self) -> (u32, u32) {
            (4, 2)
        }
        fn draw(&mut self, _: &[u8], _: u32, _: u32, _: u32, _: u32, _: bool) {}
        fn next(&mut self) -> Gesture {
            Gesture::Tap(3, 1)
        }
    }

    #[test]
    fn a_session_is_written_as_it_happened() {
        let app = std::env::temp_dir().join(format!("wordhord-record-{}", std::process::id()));
        std::fs::create_dir_all(&app).unwrap();
        let mut blank = Blank;
        {
            let mut r = Recorder::new(&mut blank, &app).unwrap();
            r.draw(&[0, 255, 0, 255, 9, 9, 9, 9], 4, 2, 0, 0, true);
            assert!(matches!(r.next(), Gesture::Tap(3, 1)));
        }
        let file = std::fs::read_dir(app.join("recordings"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let data = std::fs::read(&file).unwrap();
        assert!(data.starts_with(b"WHREC1\n"));
        assert_eq!(&data[7..15], &[4, 0, 0, 0, 2, 0, 0, 0]);
        // A draw, with its flash, and its pixels deflated back as they were.
        assert_eq!(data[23], b'D');
        assert_eq!(data[24 + 16], 1);
        let n = u32::from_le_bytes(data[41..45].try_into().unwrap()) as usize;
        let pixels = miniz_oxide::inflate::decompress_to_vec(&data[45..45 + n]).unwrap();
        assert_eq!(pixels, [0, 255, 0, 255, 9, 9, 9, 9]);
        // Then the tap, as written.
        let at = 45 + n;
        assert_eq!(data[at + 8], b'G');
        assert_eq!(&data[at + 11..], b"Tap(3, 1)");
        std::fs::remove_dir_all(&app).unwrap();
    }
}
