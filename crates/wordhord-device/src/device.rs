//! The screen and the input devices, through FBInk (src/fb.c).

use std::ffi::{c_char, c_int, CString};
use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use wordhord_typeset::fonts::{Face, FontFile, Fonts};
use wordhord_typeset::page::{Page, Run};
use wordhord_typeset::render::{self, Renderer};

use crate::book::{self, Book};
use crate::reader::{Effect, Reader};
use crate::touch::{self, Event, Gesture, Gestures, Panel, EVENT_SIZE};

extern "C" {
    fn wh_open() -> c_int;
    fn wh_close(fd: c_int);
    fn wh_state(out: *mut u32);
    fn wh_draw(fd: c_int, grey: *const u8, w: u32, h: u32, x: u32, y: u32, flash: c_int) -> c_int;
    fn wh_save(fd: c_int, path: *const c_char) -> c_int;
    fn wh_restore(fd: c_int, path: *const c_char) -> c_int;
    fn wh_inputs(fds: *mut c_int, types: *mut u32, max: c_int) -> c_int;
}

const INPUT_TOUCHSCREEN: u32 = 1 << 3;
const INPUT_TABLET: u32 = 1 << 5;
/// `_IOW('E', 0x90, int)`: take a device's events for ourselves, so
/// Nickel doesn't find our taps waiting when it resumes.
const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
/// Nickel's sleep timer is paused with it, so the reader closes itself
/// after this long untouched, and Nickel takes over again.
const IDLE_MS: c_int = 30 * 60 * 1000;

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--restore", screen] => restore(screen),
        [library, fonts, screen] => run(Path::new(library), Path::new(fonts), screen),
        _ => {
            eprintln!(
                "usage: wordhord-reader <library> <fonts> <screen file> | --restore <screen file>"
            );
            ExitCode::from(2)
        }
    }
}

fn restore(screen: &str) -> ExitCode {
    let path = CString::new(screen).expect("no NUL in a path");
    let fd = unsafe { wh_open() };
    if fd < 0 {
        eprintln!("can't open the framebuffer");
        return ExitCode::FAILURE;
    }
    let ok = unsafe { wh_restore(fd, path.as_ptr()) } == 0;
    unsafe { wh_close(fd) };
    if ok {
        ExitCode::SUCCESS
    } else {
        eprintln!("couldn't put the screen back from {screen}");
        ExitCode::FAILURE
    }
}

struct Input {
    fd: c_int,
    types: u32,
}

fn run(library: &Path, fonts: &Path, screen: &str) -> ExitCode {
    let path = CString::new(screen).expect("no NUL in a path");
    let fd = unsafe { wh_open() };
    if fd < 0 {
        eprintln!("can't open the framebuffer");
        return ExitCode::FAILURE;
    }
    // No way back to Nickel's screen, no reader.
    if unsafe { wh_save(fd, path.as_ptr()) } != 0 {
        eprintln!("couldn't save the screen to {screen}");
        unsafe { wh_close(fd) };
        return ExitCode::FAILURE;
    }
    let mut state = [0u32; 6];
    unsafe { wh_state(state.as_mut_ptr()) };
    let panel = Panel {
        width: state[0],
        height: state[1],
        swap: state[3] != 0,
        mirror_x: state[4] != 0,
        mirror_y: state[5] != 0,
    };
    eprintln!(
        "screen {}×{} at {} bpp; touch: swap {}, mirror x {}, mirror y {}",
        panel.width, panel.height, state[2], panel.swap, panel.mirror_x, panel.mirror_y
    );
    let mut fds = [0 as c_int; 8];
    let mut types = [0u32; 8];
    let n = unsafe { wh_inputs(fds.as_mut_ptr(), types.as_mut_ptr(), fds.len() as c_int) };
    let inputs: Vec<Input> = (0..n.max(0) as usize)
        .map(|k| Input {
            fd: fds[k],
            types: types[k],
        })
        .collect();
    for i in &inputs {
        let grabbed = unsafe { libc::ioctl(i.fd, EVIOCGRAB as _, 1 as c_int) } == 0;
        let why = if grabbed {
            String::new()
        } else {
            format!(" ({})", std::io::Error::last_os_error())
        };
        eprintln!(
            "input fd {} types {:#x} grabbed {grabbed}{why}",
            i.fd, i.types
        );
    }

    let result = read(fd, &panel, &inputs, library, fonts);

    for i in &inputs {
        unsafe {
            libc::ioctl(i.fd, EVIOCGRAB as _, 0 as c_int);
            libc::close(i.fd);
        }
    }
    let restored = unsafe { wh_restore(fd, path.as_ptr()) } == 0;
    unsafe { wh_close(fd) };
    match result {
        Ok(()) if restored => ExitCode::SUCCESS,
        Ok(()) => {
            eprintln!("couldn't put the screen back");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn read(
    fd: c_int,
    panel: &Panel,
    inputs: &[Input],
    library: &Path,
    fonts: &Path,
) -> Result<(), String> {
    let started = Instant::now();
    let loaded = book::choose(library)
        .ok_or_else(|| format!("no books in {}", library.display()))
        .and_then(|dir| Book::load(&dir, fonts));
    let book = match loaded {
        Ok(b) => b,
        Err(e) => {
            say(
                fd,
                panel,
                fonts,
                &[
                    "Wordhord can't open a book.",
                    &e,
                    "Hold a finger down to go back.",
                ],
            );
            wait_to_close(panel, inputs);
            return Err(e);
        }
    };
    let t = &book.typeset;
    eprintln!(
        "{}: {} pages, {} notes, loaded in {:?}",
        book.dir.display(),
        t.pages.len(),
        t.notes.len(),
        started.elapsed()
    );
    if (t.width, t.height) != (panel.width, panel.height) {
        let e = format!(
            "These pages are set for a {}×{} screen. This one is {}×{}.",
            t.width, t.height, panel.width, panel.height
        );
        say(fd, panel, fonts, &[&e, "Hold a finger down to go back."]);
        wait_to_close(panel, inputs);
        return Err(e);
    }
    if let (Some(name), Some(parent)) = (book.dir.file_name(), book.dir.parent()) {
        let _ = std::fs::write(parent.join("last"), name.to_string_lossy().as_bytes());
    }
    // A compiled book keeps its images beside its pages.
    let images = book.dir.join("images");
    let read = move |src: &str| std::fs::read(images.join(src)).ok();
    let mut renderer = Renderer::new(&book.fonts).with_images(&read);
    let mut reader = Reader::new(t, book.position());
    // The page as drawn, to put back where the panel was.
    let mut shown = draw(fd, &mut renderer, &t.pages[reader.page], panel, true);
    let mut gestures = Gestures::default();
    loop {
        let gesture = next_gesture(panel, inputs, &mut gestures);
        let effect = reader.act(gesture);
        eprintln!("{gesture:?}: {effect:?}");
        match effect {
            Effect::Nothing => {}
            Effect::Close => break,
            Effect::Page { flash } => {
                shown = draw(fd, &mut renderer, &t.pages[reader.page], panel, flash);
                book.remember(reader.page);
            }
            Effect::Panel => {
                let Some(open) = reader.open else { continue };
                let area = &t.panel;
                let (w, h) = (area.width as u32, area.height as u32);
                let page = &t.notes[open.note].pages[open.page];
                let pixels = render::panel(&mut renderer, page, w, h);
                unsafe { wh_draw(fd, pixels.as_ptr(), w, h, area.x as u32, area.y as u32, 0) };
            }
            Effect::ClosePanel => {
                let top = t.panel.y as u32;
                let rows = &shown[top as usize * panel.width as usize..];
                let h = panel.height - top;
                unsafe { wh_draw(fd, rows.as_ptr(), panel.width, h, 0, top, 0) };
            }
        }
    }
    book.remember(reader.page);
    Ok(())
}

/// Draws a whole page; returns its pixels.
fn draw(fd: c_int, renderer: &mut Renderer, page: &Page, panel: &Panel, flash: bool) -> Vec<u8> {
    let started = Instant::now();
    let pixels = renderer.page(page, panel.width, panel.height);
    let rendered = started.elapsed();
    let rc = unsafe {
        wh_draw(
            fd,
            pixels.as_ptr(),
            panel.width,
            panel.height,
            0,
            0,
            flash as c_int,
        )
    };
    eprintln!(
        "page: rendered in {rendered:?}, shown in {:?} (rc {rc})",
        started.elapsed() - rendered
    );
    pixels
}

/// Blocks until a finger lifts or the power button is pressed. After too
/// long untouched, it's as if the power button were.
fn next_gesture(panel: &Panel, inputs: &[Input], gestures: &mut Gestures) -> Gesture {
    // Fingers come from the touch screen; a pen's own device is ignored
    // for now. If the touch screen also takes a pen, it's still the one.
    let fingers = inputs
        .iter()
        .position(|i| i.types & INPUT_TOUCHSCREEN != 0 && i.types & INPUT_TABLET == 0)
        .or_else(|| inputs.iter().position(|i| i.types & INPUT_TOUCHSCREEN != 0));
    let mut polls: Vec<libc::pollfd> = inputs
        .iter()
        .map(|i| libc::pollfd {
            fd: i.fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    loop {
        if polls.is_empty() {
            // Nothing to read from: nothing can close the reader.
            return Gesture::Power;
        }
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, IDLE_MS) };
        if n == 0 {
            eprintln!("untouched for {} minutes: closing", IDLE_MS / 60_000);
            return Gesture::Power;
        }
        if n < 0 {
            continue;
        }
        for (k, p) in polls.iter_mut().enumerate() {
            if p.revents & libc::POLLIN == 0 {
                continue;
            }
            p.revents = 0;
            let mut buf = [0u8; EVENT_SIZE * 64];
            loop {
                let got = unsafe { libc::read(p.fd, buf.as_mut_ptr().cast(), buf.len()) };
                if got <= 0 {
                    break;
                }
                for chunk in buf[..got as usize].chunks_exact(EVENT_SIZE) {
                    let e = Event::parse(chunk.try_into().expect("16 bytes"));
                    let gesture = if Some(k) == fingers {
                        gestures.touch(e, panel)
                    } else if inputs[k].types & (INPUT_TOUCHSCREEN | INPUT_TABLET) == 0 {
                        touch::key(e)
                    } else {
                        None
                    };
                    if let Some(g) = gesture {
                        return g;
                    }
                }
            }
        }
    }
}

fn wait_to_close(panel: &Panel, inputs: &[Input]) {
    let mut gestures = Gestures::default();
    while !matches!(
        next_gesture(panel, inputs, &mut gestures),
        Gesture::Hold | Gesture::Power
    ) {}
}

/// A few lines of text, centred on a white screen: for when there's no book
/// to show.
fn say(fd: c_int, panel: &Panel, fonts: &Path, lines: &[&str]) {
    let Some(fonts) = message_font(fonts) else {
        eprintln!("no font to show a message with");
        return;
    };
    let size = panel.width as f32 / 40.0;
    let mut page = Page::default();
    let mut y = panel.height as f32 / 3.0;
    for line in lines {
        let shaped = fonts.shape(line, Face::Regular, size, &[]);
        let width: f32 = shaped.iter().map(|s| s.width).sum();
        let mut x = ((panel.width as f32 - width) / 2.0).max(size);
        for s in shaped {
            page.runs.push(Run {
                font: s.font,
                size: s.size,
                x,
                y,
                glyphs: s.glyphs.iter().map(|g| (g.id, g.x, g.y)).collect(),
            });
            x += s.width;
        }
        y += size * 2.0;
    }
    let mut renderer = Renderer::new(&fonts);
    draw(fd, &mut renderer, &page, panel, true);
}

fn message_font(dir: &Path) -> Option<Fonts> {
    let path = dir.join("Junicode-Regular.otf");
    let data = std::fs::read(&path).ok()?;
    Fonts::from_files(vec![FontFile {
        path: path.display().to_string(),
        sha256: String::new(),
        data: Box::leak(data.into_boxed_slice()),
    }])
    .ok()
}
