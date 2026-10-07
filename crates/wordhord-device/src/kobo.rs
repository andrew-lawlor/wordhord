//! The Kobo: the screen and the input devices, through FBInk (src/fb.c).
//! Nickel is paused by `wordhord.sh`; the reader saves Nickel's screen
//! first and puts it back on exit.

use std::ffi::{c_char, c_int, CString};
use std::path::Path;
use std::process::ExitCode;

use crate::app;
use crate::platform::Platform;
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

    let mut kobo = Kobo {
        fd,
        panel,
        inputs,
        gestures: Gestures::default(),
    };
    let result = app::run(&mut kobo, library, fonts);
    let Kobo { fd, inputs, .. } = kobo;

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

struct Kobo {
    fd: c_int,
    panel: Panel,
    inputs: Vec<Input>,
    gestures: Gestures,
}

impl Platform for Kobo {
    fn size(&self) -> (u32, u32) {
        (self.panel.width, self.panel.height)
    }

    fn draw(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, flash: bool) {
        let rc = unsafe { wh_draw(self.fd, grey.as_ptr(), w, h, x, y, flash as c_int) };
        if rc < 0 {
            eprintln!("drawing failed: {rc}");
        }
    }

    fn next(&mut self) -> Gesture {
        next_gesture(&self.panel, &self.inputs, &mut self.gestures)
    }
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
