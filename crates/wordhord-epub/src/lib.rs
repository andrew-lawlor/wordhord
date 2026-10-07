//! Reads any EPUB, without a recipe: the package (what the book is and
//! the order of its chapters), its table of contents, and a small CSS
//! engine that says what each element means: italic, a verse line, a
//! hidden note, a block of code.
//!
//! Pure Rust, so it runs on the Kobo as well as the desktop.

pub mod css;
pub mod package;
pub mod xml;

pub use package::Epub;
