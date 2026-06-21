//! ratatui [`Backend`] implementation that converts `Cell` diffs into ANSI
//! escape sequences written to the zellij plugin's stdout.
//!
//! Zellij captures the plugin's stdout during `render(rows, cols)` and
//! interprets it as raw ANSI, so a backend that emits cursor-position + SGR +
//! symbol escapes is sufficient to display a ratatui UI inside a floating pane.
//!
//! The escape-generating helpers below are pure functions (`&T -> String`) so
//! they can be unit-tested without the zellij runtime.

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Cell, CellDiffOption};
use ratatui::layout::{Position, Size};
use ratatui::style::{Color, Modifier};

/// Error returned by [`ZellijBackend`].
///
/// Ratatui's `Backend` trait requires an owned error type. Wrapping a descriptive
/// `String` keeps the surface tiny while still surfacing what went wrong.
#[derive(Debug)]
pub struct BackendError(pub String);

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "zellij backend error: {}", self.0)
    }
}

impl std::error::Error for BackendError {}

impl From<std::io::Error> for BackendError {
    fn from(err: std::io::Error) -> Self {
        BackendError(err.to_string())
    }
}

/// A ratatui [`Backend`] that renders by emitting ANSI escape sequences to
/// stdout (which zellij captures from the plugin).
///
/// `size` is provided up-front by the plugin's `render(rows, cols)` hook, and
/// `prev_cursor` records the last cursor position we moved to so that
/// [`Backend::get_cursor_position`] can return something consistent without a
/// real terminal round-trip (impossible inside wasm).
pub struct ZellijBackend {
    size: Size,
    prev_cursor: Option<(u16, u16)>,
}

impl ZellijBackend {
    /// Create a new backend bound to the given terminal `size`.
    pub fn new(size: Size) -> Self {
        Self {
            size,
            prev_cursor: None,
        }
    }
}

impl Backend for ZellijBackend {
    type Error = BackendError;

    fn draw<'a, I>(&mut self, content: I) -> Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        // Batch the entire frame into one `String` and flush with a single
        // `print!`. Calling `print!` per cell would be O(n) host calls into the
        // wasm runtime, which dominates frame time for non-trivial UIs.
        let mut out = String::new();
        for (x, y, cell) in content {
            // Skip the trailing column(s) of a wide grapheme cluster — they
            // carry no symbol of their own and would clobber the lead cell.
            if matches!(cell.diff_option, CellDiffOption::Skip) {
                continue;
            }
            // Move cursor to the logical cell (ANSI is 1-indexed).
            out.push_str(&cursor_to(x, y));
            // Full SGR reset then re-apply fg/bg/modifier. Less efficient than
            // diffing styles against the previous cell, but always correct and
            // cheap enough for a menu pane.
            out.push_str("\x1b[0m");
            out.push_str(&color_to_sgr_fg(&cell.fg));
            out.push_str(&color_to_sgr_bg(&cell.bg));
            out.push_str(&modifier_to_sgr(&cell.modifier));
            // The symbol is a grapheme cluster: may be multi-byte, or empty for
            // a cell whose content was cleared but still needs a bg fill.
            out.push_str(cell.symbol());
        }
        print!("{}", out);
        Ok(())
    }

    fn hide_cursor(&mut self) -> Result<(), Self::Error> {
        print!("\x1b[?25l");
        Ok(())
    }

    fn show_cursor(&mut self) -> Result<(), Self::Error> {
        print!("\x1b[?25h");
        Ok(())
    }

    fn get_cursor_position(&mut self) -> Result<Position, Self::Error> {
        // There is no real cursor to query from wasm. Return the last position
        // we set (or the origin) so ratatui's internal bookkeeping is stable.
        Ok(self
            .prev_cursor
            .map(|(x, y)| Position::new(x, y))
            .unwrap_or_default())
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> Result<(), Self::Error> {
        let pos: Position = position.into();
        self.prev_cursor = Some((pos.x, pos.y));
        print!("{}", cursor_to(pos.x, pos.y));
        Ok(())
    }

    fn clear(&mut self) -> Result<(), Self::Error> {
        print!("\x1b[2J\x1b[H");
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> Result<(), Self::Error> {
        print!("{}", clear_region_to(clear_type));
        Ok(())
    }

    fn size(&self) -> Result<Size, Self::Error> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> Result<WindowSize, Self::Error> {
        // Pixel-accurate window size is meaningless inside a zellij plugin.
        Err(BackendError(
            "window_size is not supported in the zellij plugin backend".into(),
        ))
    }

    fn flush(&mut self) -> Result<(), Self::Error> {
        // We write everything through `print!`, which zellij drains on its own
        // cadence; there is no buffered stdout to explicitly flush here.
        Ok(())
    }
}

// ===========================================================================
// Pure ANSI escape helpers
//
// Each returns an owned `String` so they compose trivially (push_str) and are
// directly assertable in unit tests without touching the terminal.
// ===========================================================================

/// Cursor-position CSI for logical cell `(x, y)`.
///
/// ANSI rows/columns are 1-indexed, so row = y + 1 and col = x + 1.
pub fn cursor_to(x: u16, y: u16) -> String {
    format!("\x1b[{};{}H", y + 1, x + 1)
}

/// Foreground SGR escape for a [`Color`].
pub fn color_to_sgr_fg(color: &Color) -> String {
    match color {
        Color::Reset => "\x1b[39m".into(),
        Color::Black => "\x1b[30m".into(),
        Color::Red => "\x1b[31m".into(),
        Color::Green => "\x1b[32m".into(),
        Color::Yellow => "\x1b[33m".into(),
        Color::Blue => "\x1b[34m".into(),
        Color::Magenta => "\x1b[35m".into(),
        Color::Cyan => "\x1b[36m".into(),
        Color::Gray => "\x1b[37m".into(),
        Color::DarkGray => "\x1b[90m".into(),
        Color::LightRed => "\x1b[91m".into(),
        Color::LightGreen => "\x1b[92m".into(),
        Color::LightYellow => "\x1b[93m".into(),
        Color::LightBlue => "\x1b[94m".into(),
        Color::LightMagenta => "\x1b[95m".into(),
        Color::LightCyan => "\x1b[96m".into(),
        Color::White => "\x1b[97m".into(),
        Color::Indexed(n) => format!("\x1b[38;5;{}m", n),
        Color::Rgb(r, g, b) => format!("\x1b[38;2;{};{};{}m", r, g, b),
    }
}

/// Background SGR escape for a [`Color`].
pub fn color_to_sgr_bg(color: &Color) -> String {
    match color {
        Color::Reset => "\x1b[49m".into(),
        Color::Black => "\x1b[40m".into(),
        Color::Red => "\x1b[41m".into(),
        Color::Green => "\x1b[42m".into(),
        Color::Yellow => "\x1b[43m".into(),
        Color::Blue => "\x1b[44m".into(),
        Color::Magenta => "\x1b[45m".into(),
        Color::Cyan => "\x1b[46m".into(),
        Color::Gray => "\x1b[47m".into(),
        Color::DarkGray => "\x1b[100m".into(),
        Color::LightRed => "\x1b[101m".into(),
        Color::LightGreen => "\x1b[102m".into(),
        Color::LightYellow => "\x1b[103m".into(),
        Color::LightBlue => "\x1b[104m".into(),
        Color::LightMagenta => "\x1b[105m".into(),
        Color::LightCyan => "\x1b[106m".into(),
        Color::White => "\x1b[107m".into(),
        Color::Indexed(n) => format!("\x1b[48;5;{}m", n),
        Color::Rgb(r, g, b) => format!("\x1b[48;2;{};{};{}m", r, g, b),
    }
}

/// Concatenated SGR escapes for every modifier bit set in `modifier`.
///
/// Order follows the modifier bit order; terminals apply SGRs left to right so
/// the final visual result is independent of ordering for non-conflicting bits.
/// `BOLD`/`DIM` share the clear code `\x1b[22m`, which is fine here because we
/// only ever *enable* bits (a full `\x1b[0m` reset precedes every cell in
/// [`Backend::draw`]).
pub fn modifier_to_sgr(modifier: &Modifier) -> String {
    let mut out = String::new();
    if modifier.contains(Modifier::BOLD) {
        out.push_str("\x1b[1m");
    }
    if modifier.contains(Modifier::DIM) {
        out.push_str("\x1b[2m");
    }
    if modifier.contains(Modifier::ITALIC) {
        out.push_str("\x1b[3m");
    }
    if modifier.contains(Modifier::UNDERLINED) {
        out.push_str("\x1b[4m");
    }
    if modifier.contains(Modifier::SLOW_BLINK) {
        out.push_str("\x1b[5m");
    }
    if modifier.contains(Modifier::RAPID_BLINK) {
        out.push_str("\x1b[6m");
    }
    if modifier.contains(Modifier::REVERSED) {
        out.push_str("\x1b[7m");
    }
    // HIDDEN is intentionally not rendered: it would make text invisible.
    if modifier.contains(Modifier::CROSSED_OUT) {
        out.push_str("\x1b[9m");
    }
    out
}

/// Erase escape for a [`ClearType`].
pub fn clear_region_to(clear_type: ClearType) -> &'static str {
    match clear_type {
        ClearType::All => "\x1b[2J\x1b[H",
        ClearType::AfterCursor => "\x1b[J",
        ClearType::BeforeCursor => "\x1b[1J",
        ClearType::CurrentLine => "\x1b[2K",
        ClearType::UntilNewLine => "\x1b[K",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- foreground color -------------------------------------------------

    #[test]
    fn fg_named_color_red() {
        assert_eq!(color_to_sgr_fg(&Color::Red), "\x1b[31m");
    }

    #[test]
    fn fg_indexed_color() {
        assert_eq!(color_to_sgr_fg(&Color::Indexed(154)), "\x1b[38;5;154m");
    }

    #[test]
    fn fg_rgb_color() {
        assert_eq!(
            color_to_sgr_fg(&Color::Rgb(255, 0, 128)),
            "\x1b[38;2;255;0;128m"
        );
    }

    #[test]
    fn fg_all_named_colors_cover_full_ansi_table() {
        // Spot-check the boundaries of the named palette to guard against
        // off-by-one regressions in the fg match arms.
        assert_eq!(color_to_sgr_fg(&Color::Black), "\x1b[30m");
        assert_eq!(color_to_sgr_fg(&Color::Gray), "\x1b[37m");
        assert_eq!(color_to_sgr_fg(&Color::DarkGray), "\x1b[90m");
        assert_eq!(color_to_sgr_fg(&Color::White), "\x1b[97m");
    }

    // --- background color -------------------------------------------------

    #[test]
    fn bg_reset_color() {
        assert_eq!(color_to_sgr_bg(&Color::Reset), "\x1b[49m");
    }

    #[test]
    fn bg_indexed_color() {
        assert_eq!(color_to_sgr_bg(&Color::Indexed(51)), "\x1b[48;5;51m");
    }

    #[test]
    fn bg_rgb_color() {
        assert_eq!(
            color_to_sgr_bg(&Color::Rgb(10, 20, 30)),
            "\x1b[48;2;10;20;30m"
        );
    }

    // --- modifiers --------------------------------------------------------

    #[test]
    fn modifier_bold() {
        assert!(modifier_to_sgr(&Modifier::BOLD).contains("\x1b[1m"));
    }

    #[test]
    fn modifier_all_contains_key_bits() {
        let sgr = modifier_to_sgr(&Modifier::all());
        assert!(sgr.contains("\x1b[1m"), "missing bold: {sgr}");
        assert!(sgr.contains("\x1b[3m"), "missing italic: {sgr}");
        assert!(sgr.contains("\x1b[4m"), "missing underline: {sgr}");
        assert!(sgr.contains("\x1b[7m"), "missing reverse: {sgr}");
    }

    #[test]
    fn modifier_empty_emits_nothing() {
        assert!(modifier_to_sgr(&Modifier::empty()).is_empty());
    }

    #[test]
    fn modifier_individual_bits_emit_exactly_their_escape() {
        assert_eq!(modifier_to_sgr(&Modifier::ITALIC), "\x1b[3m");
        assert_eq!(modifier_to_sgr(&Modifier::UNDERLINED), "\x1b[4m");
        assert_eq!(modifier_to_sgr(&Modifier::REVERSED), "\x1b[7m");
        assert_eq!(modifier_to_sgr(&Modifier::CROSSED_OUT), "\x1b[9m");
    }

    // --- cursor positioning ----------------------------------------------

    #[test]
    fn cursor_to_is_one_indexed() {
        // x=5 -> col 6, y=10 -> row 11
        assert_eq!(cursor_to(5, 10), "\x1b[11;6H");
    }

    #[test]
    fn cursor_to_origin() {
        assert_eq!(cursor_to(0, 0), "\x1b[1;1H");
    }

    // --- clear region -----------------------------------------------------

    #[test]
    fn clear_region_all() {
        assert_eq!(clear_region_to(ClearType::All), "\x1b[2J\x1b[H");
    }

    #[test]
    fn clear_region_variants() {
        assert_eq!(clear_region_to(ClearType::AfterCursor), "\x1b[J");
        assert_eq!(clear_region_to(ClearType::BeforeCursor), "\x1b[1J");
        assert_eq!(clear_region_to(ClearType::CurrentLine), "\x1b[2K");
        assert_eq!(clear_region_to(ClearType::UntilNewLine), "\x1b[K");
    }

    // --- backend behavior -------------------------------------------------

    #[test]
    fn size_returns_configured_value() {
        let backend = ZellijBackend::new(Size::new(120, 40));
        let size = backend.size().expect("size should be Ok");
        assert_eq!(size, Size::new(120, 40));
    }

    #[test]
    fn window_size_is_unsupported() {
        let mut backend = ZellijBackend::new(Size::new(10, 10));
        assert!(backend.window_size().is_err());
    }

    #[test]
    fn set_then_get_cursor_round_trips() {
        let mut backend = ZellijBackend::new(Size::new(80, 24));
        // Before any set, get_cursor_position should report the origin.
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(0, 0));

        backend.set_cursor_position(Position::new(7, 3)).unwrap();
        assert_eq!(backend.get_cursor_position().unwrap(), Position::new(7, 3));
    }
}
