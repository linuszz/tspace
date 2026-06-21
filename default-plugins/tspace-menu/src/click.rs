//! Mouse hit-testing infrastructure for the command palette.
//!
//! During [`CommandPaletteState::render`](crate::screens::CommandPaletteState::render),
//! each visible result row registers a [`ClickRegion`] covering its
//! pane-relative screen area. Mouse events (click / hover) are then
//! resolved to a [`ClickAction`] via [`hit_test`].
//!
//! All coordinates are **pane-relative**: `(0, 0)` = top-left of the
//! plugin pane, matching both the ratatui `Buffer` origin and the
//! Zellij `Mouse` coordinate system.

// ===========================================================================
// ClickAction
// ===========================================================================

/// Action triggered by a mouse event in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    /// Click on a result row — execute the command at this `matches`
    /// index (NOT the `all_commands()` index; the caller resolves via
    /// `PaletteMatch::cmd_index`).
    ExecuteCommand(usize),
    /// Hover / click to highlight (select) this match index without
    /// executing. Reserved for future use; the current hover path reuses
    /// [`ExecuteCommand`](Self::ExecuteCommand).
    #[allow(dead_code)]
    SelectRow(usize),
    /// Click outside the palette result area — close the palette.
    #[allow(dead_code)]
    ClosePalette,
}

// ===========================================================================
// ClickRegion
// ===========================================================================

/// A rectangular hit-test region in pane-relative coordinates.
///
/// Row / column ranges are **start-inclusive, end-exclusive**:
/// `row_start <= row < row_end` and `col_start <= col < col_end`.
#[derive(Debug, Clone)]
pub struct ClickRegion {
    pub row_start: usize,
    pub row_end: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub action: ClickAction,
}

impl ClickRegion {
    /// Create a single-row region spanning columns `[col_start, col_end)`.
    pub fn row(row: usize, col_start: usize, col_end: usize, action: ClickAction) -> Self {
        Self {
            row_start: row,
            row_end: row + 1,
            col_start,
            col_end,
            action,
        }
    }
}

// ===========================================================================
// hit_test
// ===========================================================================

/// Find the topmost (first-inserted) region containing `(row, col)` and
/// return a reference to its action. Returns `None` on miss.
///
/// When regions overlap, the **first** match in slice order wins — this
/// mirrors a paint-order / z-stack where earlier draws are conceptually
/// "on top" for click purposes.
pub fn hit_test(regions: &[ClickRegion], row: usize, col: usize) -> Option<&ClickAction> {
    regions
        .iter()
        .find(|r| row >= r.row_start && row < r.row_end && col >= r.col_start && col < r.col_end)
        .map(|r| &r.action)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_hit_returns_action() {
        let regions = vec![
            ClickRegion::row(5, 0, 80, ClickAction::ExecuteCommand(0)),
            ClickRegion::row(6, 0, 80, ClickAction::ExecuteCommand(1)),
        ];
        let action = hit_test(&regions, 5, 10);
        assert_eq!(action, Some(&ClickAction::ExecuteCommand(0)));

        let action = hit_test(&regions, 6, 79);
        assert_eq!(action, Some(&ClickAction::ExecuteCommand(1)));
    }

    #[test]
    fn miss_returns_none() {
        let regions = vec![ClickRegion::row(5, 10, 80, ClickAction::ExecuteCommand(0))];
        // Wrong row.
        assert_eq!(hit_test(&regions, 4, 40), None);
        assert_eq!(hit_test(&regions, 6, 40), None);
        // Right row, wrong column (past col_end).
        assert_eq!(hit_test(&regions, 5, 80), None);
        // Right row, wrong column (before col_start).
        assert_eq!(hit_test(&regions, 5, 9), None);
    }

    #[test]
    fn boundary_conditions_are_half_open() {
        let regions = vec![ClickRegion::row(5, 10, 20, ClickAction::ExecuteCommand(42))];
        // col_start inclusive.
        assert_eq!(
            hit_test(&regions, 5, 10),
            Some(&ClickAction::ExecuteCommand(42))
        );
        // col_end - 1 inclusive.
        assert_eq!(
            hit_test(&regions, 5, 19),
            Some(&ClickAction::ExecuteCommand(42))
        );
        // col_end exclusive.
        assert_eq!(hit_test(&regions, 5, 20), None);
        // col_start - 1 excluded.
        assert_eq!(hit_test(&regions, 5, 9), None);
        // row_start inclusive, row_end exclusive.
        assert_eq!(hit_test(&regions, 4, 15), None);
        assert_eq!(hit_test(&regions, 6, 15), None);
    }

    #[test]
    fn overlapping_regions_first_match_wins() {
        let regions = vec![
            ClickRegion::row(5, 0, 80, ClickAction::ExecuteCommand(0)),
            ClickRegion::row(5, 0, 80, ClickAction::ExecuteCommand(1)),
        ];
        let action = hit_test(&regions, 5, 10);
        assert_eq!(action, Some(&ClickAction::ExecuteCommand(0)));
    }

    #[test]
    fn empty_region_list_returns_none() {
        let regions: Vec<ClickRegion> = vec![];
        assert_eq!(hit_test(&regions, 0, 0), None);
    }
}
