//! The Wordhord reader on a Kobo. NickelMenu starts it through
//! `wordhord.sh`, which pauses Nickel first and resumes it afterwards.
//!
//! ```text
//! wordhord-reader <library> <fonts> <screen file>
//! wordhord-reader --restore <screen file>
//! ```
//!
//! It saves the screen Nickel left, reads the book read last, and puts
//! Nickel's screen back on exit. If it fails, the script runs it again with
//! `--restore`, so the screen always comes back.

#![cfg_attr(not(target_arch = "arm"), allow(dead_code))]

mod book;
#[cfg(target_arch = "arm")]
mod device;
mod reader;
mod touch;

use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(target_arch = "arm")]
    {
        device::main()
    }
    #[cfg(not(target_arch = "arm"))]
    {
        eprintln!("wordhord-reader runs on a Kobo: build it with scripts/package-kobo.sh");
        ExitCode::FAILURE
    }
}
