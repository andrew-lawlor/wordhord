//! What the reader needs from the hardware it runs on (SPEC.md §13): a
//! screen to draw grey pixels on, and gestures. The Kobo, the desktop
//! window and the test script each provide them.

use crate::touch::Gesture;

pub trait Platform {
    /// The screen's size in pixels.
    fn size(&self) -> (u32, u32);

    /// Draws a `w`×`h` block of 8-bit grey at (`x`, `y`) and refreshes it.
    /// A flash clears the ghosts of earlier pages; it's slower.
    fn draw(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32, flash: bool);

    /// Waits for the next gesture.
    fn next(&mut self) -> Gesture;
}
