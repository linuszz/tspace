//! Screen definitions for the tspace-menu plugin.

pub mod command_palette;

pub use command_palette::{CommandPaletteState, PaletteAction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveScreen {
    Hidden,
    Palette,
}
