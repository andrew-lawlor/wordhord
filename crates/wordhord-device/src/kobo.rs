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
const INPUT_POWER_BUTTON: u32 = 1 << 16;
/// `EVIOCGKEY(96)`: which keys are down now. Nickel holds the power
/// button's events, but anyone may ask whether it's pressed.
const EVIOCGKEY: libc::c_ulong = 0x8060_4518;
const KEY_POWER: usize = 116;
/// How often the power button is asked after.
const POWER_MS: c_int = 50;
/// `_IOW('E', 0x90, int)`: take a device's events for ourselves, so
/// Nickel doesn't find our taps waiting when it resumes.
const EVIOCGRAB: libc::c_ulong = 0x4004_4590;
/// Nickel's sleep timer is paused with it, so the reader puts the Kobo to
/// sleep itself after this long untouched.
const IDLE_SECONDS: u64 = 15 * 60;

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["--restore", screen] => restore(screen),
        [library, app, screen] => run(Path::new(library), Path::new(app), screen),
        _ => {
            eprintln!(
                "usage: wordhord-reader <library> <app folder> <screen file> | --restore <screen file>"
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

fn run(library: &Path, app: &Path, screen: &str) -> ExitCode {
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

    let power = inputs
        .iter()
        .find(|i| i.types & INPUT_POWER_BUTTON != 0)
        .map(|i| i.fd);
    let mut kobo = Kobo {
        fd,
        panel,
        inputs,
        gestures: Gestures::default(),
        pending: std::collections::VecDeque::new(),
        held: None,
        power,
        app: app.to_path_buf(),
    };
    let result = app::run(&mut kobo, library, app);
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
    /// Events read but not yet followed, by input: a read can bring more
    /// than one gesture's worth.
    pending: std::collections::VecDeque<(usize, Event)>,
    /// A gesture found while catching up with drags, to give next.
    held: Option<Gesture>,
    /// The power button's input, to ask whether it's pressed.
    power: Option<c_int>,
    /// The app folder, where the sleep screen is.
    app: std::path::PathBuf,
}

/// Whether the key is down on an input now.
fn key_down(fd: c_int, key: usize) -> bool {
    let mut keys = [0u8; 96];
    let rc = unsafe { libc::ioctl(fd, EVIOCGKEY as _, keys.as_mut_ptr()) };
    rc >= 0 && keys[key / 8] & (1 << (key % 8)) != 0
}

fn write(path: &str, value: &str) -> bool {
    std::fs::write(path, value).is_ok()
}

/// Whether the Kobo is on its charger: MediaTek Kobos crash if they sleep
/// then, KOReader found.
fn charging() -> bool {
    std::fs::read_to_string("/sys/class/power_supply/bd71827_ac/online")
        .is_ok_and(|v| v.trim() == "1")
        || std::fs::read_to_string("/sys/class/power_supply/bd71827_bat/status")
            .is_ok_and(|v| v.trim() == "Charging")
}

/// A wireless interface, if Nickel left one up: Wi-Fi switched on before
/// Wordhord opened stays so while Nickel is paused, and KOReader found that
/// suspending with its driver loaded can crash the kernel. The loopback
/// and the kernel's virtual interfaces don't count.
fn radio() -> Option<String> {
    std::fs::read_dir("/sys/class/net")
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|name| {
            !matches!(name.as_str(), "lo" | "sit0" | "tunl0")
                && !name.starts_with("ifb")
                && !name.starts_with("ip6")
        })
}

/// The Kobo to sleep, as KOReader does it: subsystems flagged, a pause,
/// the disks synced, the wakeup count matched, then suspend to memory. Back
/// when it wakes; false if it wouldn't sleep.
///
/// The e-ink controller keeps the panel powered a while after drawing, and
/// refuses to suspend until it has let go ("xon off timer pending"); so
/// a refusal is tried again, a second later, a few times; unless `stop`
/// says the power button has asked to wake meanwhile.
fn suspend(stop: impl Fn() -> bool) -> bool {
    if !write("/sys/power/state-extended", "1") {
        eprintln!("sleep: the kernel wouldn't flag subsystems");
        return false;
    }
    std::thread::sleep(std::time::Duration::from_secs(2));
    unsafe { libc::sync() };
    if let Ok(count) = std::fs::read_to_string("/sys/power/wakeup_count") {
        if !write("/sys/power/wakeup_count", count.trim()) {
            eprintln!("sleep: woken meanwhile");
            write("/sys/power/state-extended", "0");
            return false;
        }
    }
    let mut tries = 0;
    let slept = loop {
        tries += 1;
        match std::fs::write("/sys/power/state", "mem") {
            Ok(()) => break Ok(()),
            Err(e) if tries < 8 && !stop() => {
                eprintln!("sleep: not yet ({e}); again in a second");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            Err(e) => break Err(e),
        }
    };
    write("/sys/power/state-extended", "0");
    if let Err(e) = &slept {
        eprintln!("sleep: the kernel wouldn't sleep after {tries} tries: {e}");
        wakeup_sources();
    } else {
        eprintln!("sleep: slept, at try {tries}");
    }
    slept.is_ok()
}

/// What's keeping the Kobo awake: the wakeup sources active now, or that
/// were lately. Temporary, to find out why it won't sleep.
fn wakeup_sources() {
    eprintln!(
        "sleep: wakeup_count {:?}",
        std::fs::read_to_string("/sys/power/wakeup_count").map(|s| s.trim().to_string())
    );
    // The kernel's own account of who refused.
    if let Ok(out) = std::process::Command::new("dmesg").output() {
        let text = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = text.lines().collect();
        for line in &lines[lines.len().saturating_sub(40)..] {
            eprintln!("sleep: dmesg {line}");
        }
    }
    if let Ok(dir) = std::fs::read_dir("/sys/class/net") {
        for e in dir.flatten() {
            let state = std::fs::read_to_string(e.path().join("operstate")).unwrap_or_default();
            eprintln!(
                "sleep: net {} {}",
                e.file_name().to_string_lossy(),
                state.trim()
            );
        }
    }
    if let Ok(mods) = std::fs::read_to_string("/proc/modules") {
        let names: Vec<&str> = mods
            .lines()
            .filter_map(|l| l.split_whitespace().next())
            .collect();
        eprintln!("sleep: modules {}", names.join(" "));
    }
    let Ok(dir) = std::fs::read_dir("/sys/class/wakeup") else {
        eprintln!("sleep: no /sys/class/wakeup");
        if let Ok(all) = std::fs::read_to_string("/sys/kernel/debug/wakeup_sources") {
            for line in all.lines().filter(|l| !l.contains("\t0\t0\t0")).take(40) {
                eprintln!("sleep: {line}");
            }
        }
        return;
    };
    for e in dir.flatten() {
        let get = |leaf: &str| {
            std::fs::read_to_string(e.path().join(leaf))
                .map(|s| s.trim().to_string())
                .unwrap_or_default()
        };
        let active = get("active_time_ms");
        let events = get("event_count");
        if active != "0" || get("prevent_suspend_time_ms") != "0" || events != "0" {
            eprintln!(
                "sleep: {} active {active} ms, events {events}, active_count {}, last {}",
                get("name"),
                get("active_count"),
                get("last_change_ms")
            );
        }
    }
}

/// The sleep screen, an 8-bit grey PNG the size of the screen.
fn sleep_screen(path: &Path, width: u32, height: u32) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    (info.width == width && info.height == height && info.color_type == png::ColorType::Grayscale)
        .then(|| buf[..(width * height) as usize].to_vec())
}

impl Kobo {
    /// Sleep: the screen as it is saved, the sleep screen shown, the light
    /// out, and the Kobo suspended until the power button wakes it; then
    /// all put back as it was. On the charger it doesn't suspend, but
    /// waits on the sleep screen for the button.
    fn sleep(&mut self) {
        let started = std::time::Instant::now();
        let (width, height) = self.size();
        let awake = CString::new("/tmp/wordhord-awake.dump").expect("no NUL");
        let saved = unsafe { wh_save(self.fd, awake.as_ptr()) } == 0;
        match sleep_screen(&self.app.join("sleep.png"), width, height) {
            Some(pixels) => self.draw(&pixels, width, height, 0, 0, true),
            None => self.draw(
                &vec![255; (width * height) as usize],
                width,
                height,
                0,
                0,
                true,
            ),
        }
        let light = "/sys/class/backlight/mxc_msp430.0/brightness";
        let was = std::fs::read_to_string(light).ok();
        write(light, "0");
        let power = self.power;
        let down = |fd: Option<c_int>| fd.is_some_and(|fd| key_down(fd, KEY_POWER));
        // The press that asked for sleep, let go of first.
        while down(power) {
            std::thread::sleep(std::time::Duration::from_millis(POWER_MS as u64));
        }
        let plugged = charging();
        let radio = radio();
        let why = match (plugged, &radio) {
            (true, _) => ", on the charger: not suspending".to_string(),
            (_, Some(r)) => format!(", {r} is up: not suspending"),
            _ => String::new(),
        };
        eprintln!("sleep: in {:?}{why}", started.elapsed());
        if plugged || radio.is_some() || !suspend(|| down(power)) {
            // Awake on the sleep screen, until the button is pressed.
            while !down(power) {
                std::thread::sleep(std::time::Duration::from_millis(POWER_MS as u64));
            }
        }
        let woke = std::time::Instant::now();
        while down(power) {
            std::thread::sleep(std::time::Duration::from_millis(POWER_MS as u64));
        }
        // Touches made asleep, or on the sleep screen, count for nothing.
        read_ready(&self.inputs, &mut self.pending);
        self.pending.clear();
        let ink = self.gestures.ink;
        self.gestures = Gestures::default();
        self.gestures.ink = ink;
        if let Some(was) = was {
            write(light, was.trim());
        }
        if saved {
            unsafe { wh_restore(self.fd, awake.as_ptr()) };
        }
        eprintln!("sleep: awake again in {:?}", woke.elapsed());
    }
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

    fn draw_fast(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
        let rc = unsafe { wh_draw(self.fd, grey.as_ptr(), w, h, x, y, 2) };
        if rc < 0 {
            eprintln!("drawing failed: {rc}");
        }
    }

    fn ink(&mut self, on: bool) {
        self.gestures.ink = on;
    }

    fn tool(&self) -> touch::Tool {
        self.gestures.stroke_tool
    }

    fn take_ink(&mut self) -> Vec<(i32, i32, u16)> {
        std::mem::take(&mut self.gestures.points)
    }

    fn next(&mut self) -> Gesture {
        if let Some(g) = self.held.take() {
            return g;
        }
        // The power button, or long untouched: sleep, and on waking carry
        // on as if nothing had happened.
        let mut g = loop {
            match next_gesture(
                &self.panel,
                &self.inputs,
                &mut self.gestures,
                &mut self.pending,
                self.power,
            ) {
                Gesture::Power if self.power.is_some() => self.sleep(),
                g => break g,
            }
        };
        // A finger dragged on while the last drag was drawn: only where it
        // is now matters. Anything else that comes is kept for next time.
        while let Gesture::Drag(..) | Gesture::Ink(..) = g {
            read_ready(&self.inputs, &mut self.pending);
            let Some((k, e)) = self.pending.pop_front() else {
                break;
            };
            let next = if Some(k) == fingers(&self.inputs) {
                self.gestures.touch(e, &self.panel)
            } else if self.inputs[k].types & (INPUT_TOUCHSCREEN | INPUT_TABLET) == 0 {
                touch::key(e)
            } else {
                None
            };
            match next {
                // The latest place; ink keeps every point meanwhile.
                Some(d @ (Gesture::Drag(..) | Gesture::Ink(..))) if same(&d, &g) => g = d,
                Some(other) => {
                    self.held = Some(other);
                    break;
                }
                None => {}
            }
        }
        g
    }
}

/// Whether two gestures are of a kind: two drags, or two points of ink.
fn same(a: &Gesture, b: &Gesture) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// The input fingers come from: the touch screen. A pen's own device is
/// ignored for now; if the touch screen also takes a pen, it's still the
/// one.
fn fingers(inputs: &[Input]) -> Option<usize> {
    inputs
        .iter()
        .position(|i| i.types & INPUT_TOUCHSCREEN != 0 && i.types & INPUT_TABLET == 0)
        .or_else(|| inputs.iter().position(|i| i.types & INPUT_TOUCHSCREEN != 0))
}

/// Whatever the inputs have ready now, without waiting, onto `pending`.
fn read_ready(inputs: &[Input], pending: &mut std::collections::VecDeque<(usize, Event)>) {
    for (k, i) in inputs.iter().enumerate() {
        let mut buf = [0u8; EVENT_SIZE * 64];
        loop {
            let got = unsafe { libc::read(i.fd, buf.as_mut_ptr().cast(), buf.len()) };
            if got <= 0 {
                break;
            }
            for chunk in buf[..got as usize].chunks_exact(EVENT_SIZE) {
                pending.push_back((k, Event::parse(chunk.try_into().expect("16 bytes"))));
            }
        }
    }
}

/// Blocks until a gesture: a finger lifts, is pressed or drags, or the
/// power button is pressed. After too long untouched, it's as if the power
/// button were.
fn next_gesture(
    panel: &Panel,
    inputs: &[Input],
    gestures: &mut Gestures,
    pending: &mut std::collections::VecDeque<(usize, Event)>,
    power: Option<c_int>,
) -> Gesture {
    let idle_since = std::time::Instant::now();
    // The power button as it was: a press is its going down.
    let mut power_was = power.is_some_and(|fd| key_down(fd, KEY_POWER));
    let fingers = fingers(inputs);
    let mut polls: Vec<libc::pollfd> = inputs
        .iter()
        .map(|i| libc::pollfd {
            fd: i.fd,
            events: libc::POLLIN,
            revents: 0,
        })
        .collect();
    loop {
        // The pen's time up before any finger is read: the sign comes down
        // as fingers start to count.
        if let Some(g) = gestures.away() {
            return g;
        }
        while let Some((k, e)) = pending.pop_front() {
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
        // Every event followed, and a finger still down and still: pressed
        // by now?
        if let Some(g) = gestures.tick() {
            return g;
        }
        if polls.is_empty() {
            // Nothing to read from: nothing can close the reader.
            return Gesture::Power;
        }
        // A finger down and still: wake when it would be pressed. The pen
        // gone quiet: wake when it counts as gone.
        let press = [gestures.press_due(), gestures.away_due()]
            .into_iter()
            .flatten()
            .min();
        // Waking often enough to see the power button pressed.
        let wait = press
            .map_or(POWER_MS, |d| d.as_millis() as c_int + 1)
            .min(if power.is_some() {
                POWER_MS
            } else {
                c_int::MAX
            });
        let n = unsafe { libc::poll(polls.as_mut_ptr(), polls.len() as _, wait) };
        let power_now = power.is_some_and(|fd| key_down(fd, KEY_POWER));
        if power_now && !power_was {
            eprintln!("power button: sleep");
            return Gesture::Power;
        }
        power_was = power_now;
        if n == 0 {
            if press.is_some() {
                if let Some(g) = gestures.tick() {
                    return g;
                }
                continue;
            }
            if idle_since.elapsed().as_secs() >= IDLE_SECONDS {
                eprintln!("untouched for {} minutes: sleep", IDLE_SECONDS / 60);
                return Gesture::Power;
            }
            continue;
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
                    pending.push_back((k, Event::parse(chunk.try_into().expect("16 bytes"))));
                }
            }
        }
    }
}
