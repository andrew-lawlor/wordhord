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

    /// As `draw`, as fast as the screen can, in black and white only: for
    /// what follows a finger.
    fn draw_fast(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
        self.draw(grey, w, h, x, y, false);
    }

    /// As `draw_fast`, faster still, for ink as it's written: black on
    /// white only, and greys it touches go black or white until next drawn.
    fn draw_ink(&mut self, grey: &[u8], w: u32, h: u32, x: u32, y: u32) {
        self.draw_fast(grey, w, h, x, y);
    }

    /// Whether a pen that moves writes, from here on: in a book it does;
    /// elsewhere it's a finger.
    fn ink(&mut self, _on: bool) {}

    /// The pen's points since last asked: screen position and pressure,
    /// 0 to 4095.
    fn take_ink(&mut self) -> Vec<(i32, i32, u16)> {
        Vec::new()
    }

    /// What the pen's stroke does: the tool it was begun with.
    fn tool(&self) -> crate::touch::Tool {
        crate::touch::Tool::Pen
    }

    /// Waits for the next gesture.
    fn next(&mut self) -> Gesture;
}
