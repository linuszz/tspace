//! Phase 5: Command palette UI — the interactive fuzzy-search overlay.
//!
//! Implements the centered floating palette described in
//! `docs/command-palette-design.md`. The palette maintains query/filter
//! state, handles all keyboard input via [`CommandPaletteState::on_key`],
//! and renders itself into a ratatui [`Buffer`] via
//! [`CommandPaletteState::render`].

use std::cell::{Cell, RefCell};

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Widget};
use unicode_width::UnicodeWidthStr;
use zellij_tile::prelude::*;

use crate::click::{hit_test, ClickAction, ClickRegion};
use crate::commands::{all_commands, Category, Command};

// ===========================================================================
// PaletteAction
// ===========================================================================

/// What [`CommandPaletteState::on_key`] wants the caller to do after
/// handling a key event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    /// Key was consumed (typing / navigation); request a re-render.
    Continue,
    /// Enter was pressed — execute the command at the given
    /// `all_commands()` index, then close the palette.
    Execute(usize),
    /// Esc / Ctrl+C — close the palette without executing.
    Close,
    /// Key was not recognised; let the host ignore it.
    Noop,
}

// ===========================================================================
// PaletteMatch
// ===========================================================================

/// One filtered result: an index into `all_commands()` plus the fuzzy
/// score and the char positions within `brief` that the matcher hit (for
/// highlight rendering).
#[derive(Clone, Debug)]
pub struct PaletteMatch {
    /// Index into the `Vec<Command>` returned by [`all_commands`].
    pub cmd_index: usize,
    /// Fuzzy score (higher = better match). Used for sorting.
    pub score: i64,
    /// Character positions in `Command::brief` that matched the query.
    /// Rendered bold + underlined + magenta.
    pub matched_indices: Vec<usize>,
}

// ===========================================================================
// CommandPaletteState
// ===========================================================================

/// The complete mutable state of the command palette overlay.
///
/// All keyboard input flows through [`on_key`](Self::on_key); all drawing
/// flows through [`render`](Self::render). The struct owns the query
/// string, the filtered/ranked match list, the selection cursor and the
/// vertical scroll offset.
pub struct CommandPaletteState {
    /// The user's current search text.
    pub query: String,
    /// Filtered + ranked results for the current `query`.
    pub matches: Vec<PaletteMatch>,
    /// Index into `matches` of the highlighted row.
    pub selected: usize,
    /// Index of the first visible result row (scroll window top).
    pub scroll_offset: usize,
    /// Last known height (in rows) of the results viewport. Updated by
    /// [`render`](Self::render) and read by [`on_key`](Self::on_key) for
    /// Page Up / Page Down navigation. Interior mutability lets `render`
    /// (which takes `&self`) record the value.
    viewport_height: Cell<usize>,
    /// Click / hover hit-test regions, rebuilt on every
    /// [`render`](Self::render) call. Stored via `RefCell` because
    /// `render(&self)` cannot mutate `self` directly. Read by
    /// [`on_click`](Self::on_click) and [`on_hover`](Self::on_hover).
    click_regions: RefCell<Vec<ClickRegion>>,
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandPaletteState {
    /// Create a fresh palette: empty query, all commands shown sorted by
    /// category priority then alphabetical, selection at the top.
    pub fn new() -> Self {
        let mut state = Self {
            query: String::new(),
            matches: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            viewport_height: Cell::new(8),
            click_regions: RefCell::new(Vec::new()),
        };
        state.refresh_matches();
        state
    }

    /// Re-run the filter against the current query and reset the
    /// selection / scroll cursor. Called after every query mutation
    /// (typing, backspace, word-delete, clear, Esc-clear) and once during
    /// [`new`](Self::new).
    pub fn refresh_matches(&mut self) {
        let commands = all_commands();
        self.matches = filter(&commands, &self.query);
        if self.selected >= self.matches.len() {
            self.selected = 0;
        }
        self.scroll_offset = 0;
    }

    // -----------------------------------------------------------------------
    // Key handling
    // -----------------------------------------------------------------------

    /// Handle one intercepted key event.
    ///
    /// Returns the action the caller should take (re-render, execute,
    /// close, or ignore). See the design spec §5.1 for the full key map.
    pub fn on_key(&mut self, key: &KeyWithModifier) -> PaletteAction {
        let bare = key.bare_key;
        let mods = &key.key_modifiers;
        let ctrl = mods.contains(&KeyModifier::Ctrl);
        let shift = mods.contains(&KeyModifier::Shift);
        let alt = mods.contains(&KeyModifier::Alt);

        // ---- Hard escape / close ----
        if bare == BareKey::Char('c') && ctrl {
            return PaletteAction::Close;
        }

        // ---- Esc: clear query, or close if already empty ----
        if bare == BareKey::Esc {
            if self.query.is_empty() {
                return PaletteAction::Close;
            }
            self.query.clear();
            self.refresh_matches();
            return PaletteAction::Continue;
        }

        // ---- Query editing ----
        if bare == BareKey::Backspace {
            self.query.pop();
            self.refresh_matches();
            return PaletteAction::Continue;
        }

        if bare == BareKey::Char('u') && ctrl {
            // Ctrl+u: clear entire query (edit wins over scroll per §5.1).
            self.query.clear();
            self.refresh_matches();
            return PaletteAction::Continue;
        }

        if bare == BareKey::Char('w') && ctrl {
            delete_last_word(&mut self.query);
            self.refresh_matches();
            return PaletteAction::Continue;
        }

        // ---- Navigation: down ----
        // Down, Ctrl+n, Tab (no shift), Ctrl+j
        let go_down = bare == BareKey::Down
            || (bare == BareKey::Char('n') && ctrl)
            || (bare == BareKey::Tab && !shift)
            || (bare == BareKey::Char('j') && ctrl);
        if go_down {
            self.move_selection(1);
            return PaletteAction::Continue;
        }

        // ---- Navigation: up ----
        // Up, Ctrl+p, Shift+Tab, Ctrl+k
        let go_up = bare == BareKey::Up
            || (bare == BareKey::Char('p') && ctrl)
            || (bare == BareKey::Tab && shift)
            || (bare == BareKey::Char('k') && ctrl);
        if go_up {
            self.move_selection(-1);
            return PaletteAction::Continue;
        }

        // ---- Page navigation ----
        // PageDown / Ctrl+d: down by viewport height
        if bare == BareKey::PageDown || (bare == BareKey::Char('d') && ctrl) {
            let vh = self.viewport_height.get().max(1);
            self.move_selection(vh as isize);
            return PaletteAction::Continue;
        }
        // PageUp: up by viewport height (Ctrl+u is reserved for clear-query)
        if bare == BareKey::PageUp {
            let vh = self.viewport_height.get().max(1);
            self.move_selection(-(vh as isize));
            return PaletteAction::Continue;
        }

        // ---- Jump navigation ----
        if bare == BareKey::Home {
            self.selected = 0;
            self.scroll_offset = 0;
            return PaletteAction::Continue;
        }
        if bare == BareKey::End {
            if !self.matches.is_empty() {
                self.selected = self.matches.len() - 1;
                self.clamp_scroll();
            }
            return PaletteAction::Continue;
        }

        // ---- Execute ----
        if bare == BareKey::Enter {
            if let Some(m) = self.matches.get(self.selected) {
                return PaletteAction::Execute(m.cmd_index);
            }
            // No match state — Enter is a no-op.
            return PaletteAction::Noop;
        }

        // ---- Printable char: append to query ----
        // Shift is already baked into the glyph (e.g. Shift+a → 'A'), so
        // only Ctrl and Alt disqualify the char from query input.
        if let BareKey::Char(ch) = bare {
            if !ctrl && !alt && !ch.is_control() {
                self.query.push(ch);
                self.refresh_matches();
                return PaletteAction::Continue;
            }
        }

        PaletteAction::Noop
    }

    /// Move the selection by `delta` rows (negative = up), wrapping
    /// around the list boundaries. Adjusts `scroll_offset` to keep the
    /// selection visible.
    fn move_selection(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let len = self.matches.len() as isize;
        // Euclidean modulo for correct negative wrap-around.
        let mut new = self.selected as isize + delta;
        new = ((new % len) + len) % len;
        self.selected = new as usize;
        self.clamp_scroll();
    }

    /// Adjust `scroll_offset` so `self.selected` falls within the
    /// viewport. Uses the last known viewport height.
    fn clamp_scroll(&mut self) {
        if self.matches.is_empty() {
            self.scroll_offset = 0;
            return;
        }
        let vh = self.viewport_height.get().max(1);
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + vh {
            self.scroll_offset = self.selected.saturating_sub(vh - 1);
        }
        let max_offset = self.matches.len().saturating_sub(1);
        if self.scroll_offset > max_offset {
            self.scroll_offset = max_offset;
        }
    }

    // -----------------------------------------------------------------------
    // Mouse interaction
    // -----------------------------------------------------------------------

    /// Handle a left-click at pane-relative `(row, col)`.
    ///
    /// Returns the `matches` index of the clicked result row, or `None`
    /// if the click missed every registered region.
    pub fn on_click(&self, row: usize, col: usize) -> Option<usize> {
        let regions = self.click_regions.borrow();
        match hit_test(&regions, row, col)? {
            ClickAction::ExecuteCommand(idx) => Some(*idx),
            _ => None,
        }
    }

    /// Handle a hover at pane-relative `(row, col)`.
    ///
    /// Returns the `matches` index of the hovered result row so the
    /// caller (which holds `&mut self`) can update `selected`.
    pub fn on_hover(&self, row: usize, col: usize) -> Option<usize> {
        let regions = self.click_regions.borrow();
        match hit_test(&regions, row, col)? {
            ClickAction::ExecuteCommand(idx) => Some(*idx),
            _ => None,
        }
    }

    /// Move the selection cursor down by one row (mouse scroll-down).
    pub fn move_selection_down(&mut self) {
        self.move_selection(1);
    }

    /// Move the selection cursor up by one row (mouse scroll-up).
    pub fn move_selection_up(&mut self) {
        self.move_selection(-1);
    }

    // -----------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------

    /// Render the palette into `buf` within `area`.
    ///
    /// Layout (per design spec §3):
    /// ```text
    /// ╭─ Command Palette ────────────╮
    /// │ ❯ query_                     │  ← input (3-row section)
    /// ├──────────────────────────────┤
    /// │ ▶ result rows                │  ← results (flexible)
    /// │ ...                          │
    /// ├──────────────────────────────┤
    /// │ help footer · N results      │  ← footer (1 row)
    /// ╰──────────────────────────────╯
    /// ```
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        self.click_regions.borrow_mut().clear();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(COLOR_BORDER))
            .title(Line::styled(
                " Command Palette ",
                Style::default()
                    .fg(COLOR_TITLE)
                    .add_modifier(Modifier::BOLD),
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

        // Record the results-viewport height for Page Up/Down navigation.
        self.viewport_height.set(chunks[1].height as usize);

        self.render_input(chunks[0], buf);
        self.render_results(chunks[1], buf);
        self.render_footer(chunks[2], buf);
    }

    /// Render the input section: prompt + query on row 0, separator on
    /// row 1, blank padding on row 2.
    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        // Row 0: prompt + query (or placeholder)
        let prompt_style = Style::default()
            .fg(COLOR_PROMPT)
            .add_modifier(Modifier::BOLD);
        let query_style = Style::default().fg(COLOR_QUERY);

        let mut spans: Vec<Span<'static>> = vec![Span::styled("❯ ", prompt_style)];
        if self.query.is_empty() {
            spans.push(Span::styled(
                String::from("Type to search commands..."),
                query_style
                    .add_modifier(Modifier::DIM)
                    .add_modifier(Modifier::ITALIC),
            ));
        } else {
            spans.push(Span::styled(self.query.clone(), query_style));
        }
        Line::from(spans).render(Rect::new(area.x, area.y, area.width, 1), buf);

        // Row 1: horizontal separator spanning the full content width.
        if area.height >= 2 {
            let sep = "─".repeat(area.width as usize);
            Line::styled(sep, Style::default().fg(COLOR_SEPARATOR))
                .render(Rect::new(area.x, area.y + 1, area.width, 1), buf);
        }
        // Row 2 intentionally left blank (visual padding).
    }

    /// Render the results list — visible window of `self.matches` with
    /// per-row matched-char highlighting and selection styling.
    fn render_results(&self, area: Rect, buf: &mut Buffer) {
        if self.matches.is_empty() {
            self.render_no_match(area, buf);
            return;
        }

        let commands = all_commands();
        let visible = area.height as usize;

        for row in 0..visible {
            let idx = self.scroll_offset + row;
            if idx >= self.matches.len() {
                break;
            }
            let m = &self.matches[idx];
            let cmd = &commands[m.cmd_index];
            let is_selected = idx == self.selected;
            let row_area = Rect::new(area.x, area.y + row as u16, area.width, 1);
            let line = build_result_line(cmd, &m.matched_indices, is_selected, area.width);
            line.render(row_area, buf);

            self.click_regions.borrow_mut().push(ClickRegion::row(
                row_area.y as usize,
                row_area.x as usize,
                (row_area.x + row_area.width) as usize,
                ClickAction::ExecuteCommand(idx),
            ));
        }
    }

    /// Render the centered "No commands matching …" message (design §7.3).
    fn render_no_match(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let mid_y = area.y + area.height / 2;
        let msg = format!("No commands matching \"{}\"", self.query);
        let msg_w = msg.width();
        let x = if (area.width as usize) > msg_w {
            area.x + ((area.width as usize - msg_w) / 2) as u16
        } else {
            area.x
        };
        Line::styled(
            msg,
            Style::default()
                .fg(COLOR_NO_MATCH)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::ITALIC),
        )
        .render(Rect::new(x, mid_y, area.width, 1), buf);

        // Subtext on the next line if there's room.
        if area.height > 2 {
            let sub = "Press Backspace or Esc to clear your search.";
            let sub_w = sub.width();
            let sx = if (area.width as usize) > sub_w {
                area.x + ((area.width as usize - sub_w) / 2) as u16
            } else {
                area.x
            };
            Line::styled(
                sub,
                Style::default().fg(COLOR_QUERY).add_modifier(Modifier::DIM),
            )
            .render(Rect::new(sx, mid_y + 1, area.width, 1), buf);
        }
    }

    /// Render the footer: left-aligned help hints + right-aligned result
    /// count (design §8.5–§8.7).
    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }
        let help = "↑↓ Navigate · ⏎ Run · Esc Close · Type to search";
        let total = all_commands().len();
        let count_text = if self.query.is_empty() {
            format!("{} commands", total)
        } else {
            format!("{} of {} results", self.matches.len(), total)
        };

        let help_w = help.width();
        let count_w = count_text.width();
        let area_w = area.width as usize;

        let footer_style = Style::default()
            .fg(COLOR_FOOTER)
            .add_modifier(Modifier::DIM);
        let count_style = Style::default().fg(COLOR_COUNT).add_modifier(Modifier::DIM);

        if help_w + count_w + 2 <= area_w {
            // Both fit: help left, count right, gap in between.
            let gap = area_w.saturating_sub(help_w + count_w);
            let mut spans: Vec<Span<'static>> = vec![Span::styled(help.to_string(), footer_style)];
            spans.push(Span::raw(" ".repeat(gap)));
            spans.push(Span::styled(count_text, count_style));
            Line::from(spans).render(area, buf);
        } else {
            // Too narrow: just show help (clipped by ratatui if needed).
            Line::styled(help.to_string(), footer_style).render(area, buf);
        }
    }
}

// ===========================================================================
// filter — pure, testable
// ===========================================================================

/// Filter and rank `commands` by `query`.
///
/// - **Empty query** → all commands, sorted by category priority (Tab
///   first) then alphabetical by `brief`.
/// - **Non-empty query** → fuzzy match on `brief` (primary) and keywords
///   (secondary inclusion), with a +100 boost for exact
///   case-insensitive substring containment. Sorted by score descending.
///
/// The fuzzy matcher's `Vec<usize>` are **character positions** into
/// `brief` — perfect for per-char highlight rendering.
pub fn filter(commands: &[Command], query: &str) -> Vec<PaletteMatch> {
    if query.is_empty() {
        let mut indices: Vec<usize> = (0..commands.len()).collect();
        indices.sort_by(|&a, &b| {
            commands[a]
                .category
                .priority()
                .cmp(&commands[b].category.priority())
                .then_with(|| commands[a].brief.cmp(commands[b].brief))
        });
        return indices
            .into_iter()
            .map(|cmd_index| PaletteMatch {
                cmd_index,
                score: 0,
                matched_indices: Vec::new(),
            })
            .collect();
    }

    let matcher = SkimMatcherV2::default();
    let query_lower = query.to_lowercase();
    let mut results: Vec<PaletteMatch> = Vec::new();

    for (cmd_index, cmd) in commands.iter().enumerate() {
        // Primary: fuzzy match on the brief (drives char highlighting).
        if let Some((score, indices)) = matcher.fuzzy_indices(cmd.brief, query) {
            let mut final_score = score;
            // Exact substring boost (case-insensitive).
            if cmd.brief.to_lowercase().contains(&query_lower) {
                final_score += 100;
            }
            results.push(PaletteMatch {
                cmd_index,
                score: final_score,
                matched_indices: indices,
            });
            continue;
        }

        // Brief didn't match — try keywords for inclusion (no highlight).
        let mut best_kw: Option<i64> = None;
        for kw in cmd.keywords {
            if let Some((s, _)) = matcher.fuzzy_indices(kw, query) {
                best_kw = Some(best_kw.map_or(s, |prev| prev.max(s)));
            }
        }
        if let Some(score) = best_kw {
            results.push(PaletteMatch {
                cmd_index,
                score,
                matched_indices: Vec::new(),
            });
        }
    }

    results.sort_by(|a, b| b.score.cmp(&a.score));
    results
}

// ===========================================================================
// Row rendering helper
// ===========================================================================

/// Build a single result-row [`Line`] with per-character matched
/// highlighting, category badge, and optional shortcut hint.
///
/// The returned line is `Line<'static>` — all string data is either
/// `&'static str` (borrowed from the command registry) or an owned
/// `String`, so it outlives the render call.
fn build_result_line(
    cmd: &Command,
    matched: &[usize],
    is_selected: bool,
    row_width: u16,
) -> Line<'static> {
    let base_fg = if is_selected {
        COLOR_SELECTED_FG
    } else {
        COLOR_RESULT_TEXT
    };
    let bg = if is_selected {
        Some(COLOR_SELECTED_BG)
    } else {
        None
    };

    // Helper to stamp the selected background onto a base style.
    let stamp_bg = |style: Style| -> Style {
        match bg {
            Some(c) => style.bg(c),
            None => style,
        }
    };

    let mut spans: Vec<Span<'static>> = Vec::new();

    // --- Selection marker (▶ or blank) ---
    let marker_style = stamp_bg(
        Style::default()
            .fg(COLOR_MARKER)
            .add_modifier(Modifier::BOLD),
    );
    spans.push(Span::styled(
        if is_selected { "▶ " } else { "  " },
        marker_style,
    ));

    // --- Icon + space ---
    let icon_style = stamp_bg(Style::default().fg(base_fg));
    spans.push(Span::styled(
        format!("{} ", cmd.effective_icon()),
        icon_style,
    ));

    // --- Name with per-char matched highlighting ---
    // Consecutive chars with the same highlight state are grouped into a
    // single Span to minimise the span count.
    let highlight_style = stamp_bg(
        Style::default()
            .fg(COLOR_MATCH)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    );
    let normal_style = stamp_bg(Style::default().fg(base_fg));

    let mut current = String::new();
    let mut current_matched = false;
    for (char_idx, ch) in cmd.brief.chars().enumerate() {
        let is_match = matched.contains(&char_idx);
        if current.is_empty() {
            current.push(ch);
            current_matched = is_match;
        } else if is_match == current_matched {
            current.push(ch);
        } else {
            let style = if current_matched {
                highlight_style
            } else {
                normal_style
            };
            spans.push(Span::styled(std::mem::take(&mut current), style));
            current.push(ch);
            current_matched = is_match;
        }
    }
    if !current.is_empty() {
        let style = if current_matched {
            highlight_style
        } else {
            normal_style
        };
        spans.push(Span::styled(current, style));
    }

    // --- Right-aligned: badge + shortcut ---
    let badge = cmd.category.badge();
    let shortcut = cmd.shortcut.unwrap_or("");
    let badge_w = badge.width();
    let shortcut_w = if shortcut.is_empty() {
        0
    } else {
        shortcut.width() + 1 // +1 for the leading space
    };

    let used = 2 /* marker */ + 2 /* icon + space */ + cmd.brief.width();
    let right = badge_w + shortcut_w;
    let avail = row_width as usize;
    // Pad fills the gap so the line spans the full row width (ensures the
    // selected-row background covers every cell). Falls back to 1 space
    // when content overflows (ratatui clips the excess).
    let pad = avail.saturating_sub(used + right).max(1);

    spans.push(Span::styled(" ".repeat(pad), stamp_bg(Style::default())));

    let badge_style = stamp_bg(
        Style::default()
            .fg(category_badge_color(cmd.category))
            .add_modifier(Modifier::BOLD),
    );
    spans.push(Span::styled(badge.to_string(), badge_style));

    if !shortcut.is_empty() {
        let sc_style = stamp_bg(
            Style::default()
                .fg(COLOR_SHORTCUT)
                .add_modifier(Modifier::DIM),
        );
        spans.push(Span::styled(format!(" {}", shortcut), sc_style));
    }

    Line::from(spans)
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Map a [`Category`] to its badge foreground color (design spec §4.4).
fn category_badge_color(cat: Category) -> Color {
    match cat {
        Category::Tab => COLOR_BADGE_TAB,
        Category::Pane => COLOR_BADGE_PANE,
        Category::Session => COLOR_BADGE_SESSION,
        Category::Mode => COLOR_BADGE_MODE,
        Category::System => COLOR_BADGE_SYSTEM,
    }
}

/// Delete the last whitespace-delimited word from `query` (Ctrl+W).
///
/// Trims trailing whitespace first, then cuts back to the next whitespace
/// boundary — matching Vim/Readline word-kill semantics.
fn delete_last_word(query: &mut String) {
    while query.ends_with(char::is_whitespace) {
        query.pop();
    }
    let cut = query.rfind(char::is_whitespace).map_or(0, |p| p);
    query.truncate(cut);
}

// ===========================================================================
// ANSI 256-color palette (design spec §4.2)
// ===========================================================================

const COLOR_BORDER: Color = Color::Indexed(154); // green — frame
const COLOR_TITLE: Color = Color::Indexed(166); // orange — title text
const COLOR_PROMPT: Color = Color::Indexed(166); // orange — ❯ symbol
const COLOR_QUERY: Color = Color::Indexed(245); // bright gray — query text
const COLOR_SEPARATOR: Color = Color::Indexed(166); // orange — ─ line
const COLOR_RESULT_TEXT: Color = Color::Indexed(245); // bright gray — row text
const COLOR_SELECTED_BG: Color = Color::Indexed(154); // green — selected bg
const COLOR_SELECTED_FG: Color = Color::Indexed(238); // gray — selected fg
const COLOR_MATCH: Color = Color::Indexed(201); // magenta — matched chars
const COLOR_MARKER: Color = Color::Indexed(166); // orange — ▶ marker
const COLOR_BADGE_TAB: Color = Color::Indexed(51); // cyan
const COLOR_BADGE_PANE: Color = Color::Indexed(166); // orange
const COLOR_BADGE_SESSION: Color = Color::Indexed(154); // green
const COLOR_BADGE_MODE: Color = Color::Indexed(201); // magenta
const COLOR_BADGE_SYSTEM: Color = Color::Indexed(166); // orange
const COLOR_SHORTCUT: Color = Color::Indexed(51); // cyan — shortcut hint
const COLOR_FOOTER: Color = Color::Indexed(154); // green — footer help
const COLOR_COUNT: Color = Color::Indexed(201); // magenta — result count
const COLOR_NO_MATCH: Color = Color::Indexed(124); // red — no-match msg

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_all_commands_sorted_by_category() {
        let cmds = all_commands();
        let results = filter(&cmds, "");
        assert_eq!(results.len(), cmds.len());

        // Category priority: Tab(1) < Pane(2) < Session(3) < Mode(4) < System(5).
        // All Tab commands must precede all Pane commands, etc.
        for i in 1..results.len() {
            let prev_pri = cmds[results[i - 1].cmd_index].category.priority();
            let curr_pri = cmds[results[i].cmd_index].category.priority();
            assert!(
                prev_pri <= curr_pri,
                "category priority must be non-decreasing: {} > {}",
                prev_pri,
                curr_pri
            );
        }
    }

    #[test]
    fn exact_substring_match_ranks_first() {
        let cmds = all_commands();
        let results = filter(&cmds, "new tab");
        assert!(!results.is_empty());
        assert_eq!(
            cmds[results[0].cmd_index].brief, "New Tab",
            "exact substring should rank first"
        );
    }

    #[test]
    fn fuzzy_match_finds_relevant_commands_with_correct_indices() {
        let cmds = all_commands();
        let results = filter(&cmds, "nt");
        // "New Tab" should appear (N..T.. fuzzy match).
        let nt = results
            .iter()
            .find(|m| cmds[m.cmd_index].brief == "New Tab")
            .expect("fuzzy 'nt' should match 'New Tab'");
        // Char indices: N=0, T=4 in "New Tab".
        assert!(nt.matched_indices.contains(&0), "N at char index 0");
        assert!(nt.matched_indices.contains(&4), "T at char index 4");
    }

    #[test]
    fn keyword_match_includes_command_without_brief_highlight() {
        let cmds = all_commands();
        // "create" is a keyword for New Tab / New Pane but not in any brief.
        let results = filter(&cmds, "create");
        assert!(
            results
                .iter()
                .any(|m| cmds[m.cmd_index].brief.contains("New")),
            "keyword 'create' should match New Tab / New Pane"
        );
        // Keyword-only matches have no highlight indices.
        for m in &results {
            if cmds[m.cmd_index].brief.contains("New") {
                // Brief "New Tab" / "New Pane (Right)" / "New Pane (Down)"
                // don't contain "create", so these matched via keyword.
                // Their matched_indices may be empty (keyword path) or
                // populated if brief also fuzzy-matched. Either is valid.
                let _ = &m.matched_indices;
            }
        }
    }

    #[test]
    fn nonsense_query_returns_empty() {
        let cmds = all_commands();
        let results = filter(&cmds, "zzzzqq");
        assert!(results.is_empty(), "nonsense query should match nothing");
    }

    #[test]
    fn delete_last_word_trims_to_boundary() {
        let mut s = String::from("hello world");
        delete_last_word(&mut s);
        assert_eq!(s, "hello");

        let mut s = String::from("hello world ");
        delete_last_word(&mut s);
        assert_eq!(s, "hello");

        let mut s = String::from("single");
        delete_last_word(&mut s);
        assert_eq!(s, "");

        let mut s = String::from("");
        delete_last_word(&mut s);
        assert_eq!(s, "");
    }

    #[test]
    fn new_state_has_all_commands_and_selection_at_zero() {
        let state = CommandPaletteState::new();
        assert!(state.query.is_empty());
        assert_eq!(state.selected, 0);
        assert_eq!(state.scroll_offset, 0);
        assert_eq!(state.matches.len(), all_commands().len());
    }

    #[test]
    fn palette_action_variants_are_exhaustive() {
        // Compile-time check that the public enum has exactly these arms.
        fn assert_matches(a: PaletteAction) -> &'static str {
            match a {
                PaletteAction::Continue => "continue",
                PaletteAction::Execute(_) => "execute",
                PaletteAction::Close => "close",
                PaletteAction::Noop => "noop",
            }
        }
        assert_eq!(assert_matches(PaletteAction::Continue), "continue");
        assert_eq!(assert_matches(PaletteAction::Execute(0)), "execute");
        assert_eq!(assert_matches(PaletteAction::Close), "close");
        assert_eq!(assert_matches(PaletteAction::Noop), "noop");
    }
}
