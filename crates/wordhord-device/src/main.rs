//! The Wordhord reader. On a Kobo, NickelMenu starts it through
//! `wordhord.sh`, which pauses Nickel first and resumes it afterwards. On
//! the desktop it runs in a window, for development.
//!
//! ```text
//! wordhord-reader <library> <fonts> <screen file>      on a Kobo
//! wordhord-reader --restore <screen file>              on a Kobo
//! wordhord-reader <library> <fonts> [--scale N]        on the desktop
//! ```

mod app;
mod book;
#[cfg(not(target_arch = "arm"))]
mod desktop;
#[cfg(target_arch = "arm")]
mod kobo;
mod platform;
mod reader;
// Raw input events are the Kobo's; the desktop only needs gestures.
#[cfg_attr(not(target_arch = "arm"), allow(dead_code))]
mod touch;

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_arch = "arm")]
    {
        kobo::main()
    }
    #[cfg(not(target_arch = "arm"))]
    {
        desktop::main()
    }
}
