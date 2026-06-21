//! Phase 3: Command registry for the tspace-menu command palette.
//!
//! Maps user-visible command names to concrete zellij plugin API (shim) calls.
//! Every `CommandAction::dispatch()` arm calls a REAL shim function — the
//! signatures were verified against `zellij-tile/src/shim.rs`.
//!
//! Design reference: `docs/command-palette-design.md` Appendix A.

use std::collections::BTreeMap;
use zellij_tile::prelude::actions::Action;
use zellij_tile::prelude::*;

// ---------------------------------------------------------------------------
// Category
// ---------------------------------------------------------------------------

/// Top-level grouping for a palette command. Drives the badge glyph, the sort
/// priority (Tab < Pane < Session < Mode < System) and the accent colour used
// by the renderer (later phase).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Tab,
    Pane,
    Session,
    Mode,
    System,
}

impl Category {
    /// Short fixed-width badge rendered before the command brief, e.g. `[Tab]`.
    pub fn badge(&self) -> &'static str {
        match self {
            Category::Tab => "[Tab]",
            Category::Pane => "[Pane]",
            Category::Session => "[Sess]",
            Category::Mode => "[Mode]",
            Category::System => "[Sys] ",
        }
    }

    /// ASCII fallback icon (nerd-font icons are a post-MVP concern; the config
    /// flag will swap these later). Keep these single-char and ASCII so CJK
    /// width math in the renderer stays trivial.
    pub fn icon(&self) -> char {
        match self {
            Category::Tab => '#',
            Category::Pane => '[',
            Category::Session => '~',
            Category::Mode => '>',
            Category::System => '*',
        }
    }

    /// Lower number = higher priority in the empty-query sort.
    pub fn priority(&self) -> u8 {
        match self {
            Category::Tab => 1,
            Category::Pane => 2,
            Category::Session => 3,
            Category::Mode => 4,
            Category::System => 5,
        }
    }
}

// ---------------------------------------------------------------------------
// CommandAction
// ---------------------------------------------------------------------------

/// A resolved, parameter-less action the palette can fire.
///
/// Variants are intentionally unit — any directional / indexed argument is
/// baked into the matching `dispatch` arm so the UI layer never has to prompt
/// for parameters in the MVP.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CommandAction {
    // --- Tab ---
    NewTab,
    CloseTab,
    NextTab,
    PreviousTab,
    /// Enter Tab mode so the user can pick a tab interactively.
    GoToTab,
    GoToTab1,
    GoToTab2,
    GoToTab3,
    GoToTab4,
    GoToTab5,
    /// Enter the native RenameTab mode.
    RenameTab,
    // --- Pane ---
    NewPaneRight,
    NewPaneDown,
    ClosePane,
    MoveFocusUp,
    MoveFocusDown,
    MoveFocusLeft,
    MoveFocusRight,
    /// Toggle visibility of the floating layer for the active tab.
    ToggleFloating,
    ToggleFullscreen,
    ToggleFrames,
    /// Enter Resize mode.
    EnterResize,
    // --- Session ---
    /// Enter Session mode (interactive session switcher).
    SwitchSession,
    DetachSession,
    /// Enter Session mode where the destructive kill lives; the UI layer shows
    /// its own Y/N confirmation before reaching dispatch in a later phase.
    KillSession,
    // --- Mode ---
    EnterPaneMode,
    EnterTabMode,
    EnterMoveMode,
    EnterSearchMode,
    EnterLockedMode,
    EnterNormalMode,
    // --- System ---
    /// Re-apply configuration. NOTE: the shim has no dedicated "reload from
    /// disk" entry point, so this issues `reconfigure` with an empty payload;
    /// a proper reload API is pending upstream.
    ReloadConfig,
    Quit,
}

impl CommandAction {
    /// Execute the action against the running zellij host.
    ///
    /// Each arm calls the verified shim function. There is intentionally NO
    /// wildcard arm — adding a new `CommandAction` variant without a matching
    /// arm here is a compile error, which is exactly the safety net we want.
    pub fn dispatch(&self) {
        use CommandAction as A;
        match self {
            // ----- Tab -----
            A::NewTab => {
                // `new_tab` returns the new tab index; the palette does not
                // need it, so the `Option<usize>` is dropped on purpose.
                let _ = new_tab::<&str>(None, None);
            },
            A::CloseTab => close_focused_tab(),
            A::NextTab => go_to_next_tab(),
            A::PreviousTab => go_to_previous_tab(),
            A::GoToTab => switch_to_input_mode(&InputMode::Tab),
            A::GoToTab1 => go_to_tab(1),
            A::GoToTab2 => go_to_tab(2),
            A::GoToTab3 => go_to_tab(3),
            A::GoToTab4 => go_to_tab(4),
            A::GoToTab5 => go_to_tab(5),
            A::RenameTab => switch_to_input_mode(&InputMode::RenameTab),
            // ----- Pane -----
            A::NewPaneRight => run_action(
                Action::NewPane {
                    direction: Some(Direction::Right),
                    pane_name: None,
                    start_suppressed: false,
                },
                BTreeMap::new(),
            ),
            A::NewPaneDown => run_action(
                Action::NewPane {
                    direction: Some(Direction::Down),
                    pane_name: None,
                    start_suppressed: false,
                },
                BTreeMap::new(),
            ),
            A::ClosePane => close_focus(),
            A::MoveFocusUp => move_focus(Direction::Up),
            A::MoveFocusDown => move_focus(Direction::Down),
            A::MoveFocusLeft => move_focus(Direction::Left),
            A::MoveFocusRight => move_focus(Direction::Right),
            A::ToggleFloating => run_action(Action::ToggleFloatingPanes, BTreeMap::new()),
            A::ToggleFullscreen => toggle_focus_fullscreen(),
            A::ToggleFrames => toggle_pane_frames(),
            A::EnterResize => switch_to_input_mode(&InputMode::Resize),
            // ----- Session -----
            A::SwitchSession => switch_to_input_mode(&InputMode::Session),
            A::DetachSession => detach(),
            A::KillSession => switch_to_input_mode(&InputMode::Session),
            // ----- Mode -----
            A::EnterPaneMode => switch_to_input_mode(&InputMode::Pane),
            A::EnterTabMode => switch_to_input_mode(&InputMode::Tab),
            A::EnterMoveMode => switch_to_input_mode(&InputMode::Move),
            A::EnterSearchMode => switch_to_input_mode(&InputMode::Search),
            A::EnterLockedMode => switch_to_input_mode(&InputMode::Locked),
            A::EnterNormalMode => switch_to_input_mode(&InputMode::Normal),
            // ----- System -----
            A::ReloadConfig => reconfigure(String::new(), false),
            A::Quit => quit_zellij(),
        }
    }
}

// ---------------------------------------------------------------------------
// Command
// ---------------------------------------------------------------------------

/// A single palette entry. All fields are `&'static` so a `Command` is cheap
/// to copy around the hot render loop without allocations.
#[derive(Clone, Debug)]
pub struct Command {
    /// Stable unique identifier, e.g. `"tab.new"`. Used for frecency bookkeeping.
    pub id: &'static str,
    /// One-line label shown in the result list.
    pub brief: &'static str,
    /// Longer description (rendered in a future help overlay).
    pub doc: &'static str,
    /// Extra fuzzy keywords beyond the words in `brief`.
    pub keywords: &'static [&'static str],
    /// Optional single-character ASCII icon; falls back to `category.icon()`.
    pub icon: Option<char>,
    /// Optional default keybinding hint, e.g. `"Ctrl+T"`. Display-only.
    pub shortcut: Option<&'static str>,
    /// Grouping used for badges, colours and sort priority.
    pub category: Category,
    /// What to run when the user selects this command.
    pub action: CommandAction,
}

impl Command {
    /// Effective icon: per-command override if present, else the category icon.
    pub fn effective_icon(&self) -> char {
        self.icon.unwrap_or_else(|| self.category.icon())
    }
}

// ---------------------------------------------------------------------------
// all_commands
// ---------------------------------------------------------------------------

/// The static MVP registry. Returns 33 commands across the five categories,
/// matching `docs/command-palette-design.md` Appendix A.
pub fn all_commands() -> Vec<Command> {
    vec![
        // ===================== Tab =====================
        Command {
            id: "tab.new",
            brief: "New Tab",
            doc: "Open a new tab with the default layout.",
            keywords: &["create", "open", "add"],
            icon: Some('+'),
            shortcut: Some("Ctrl+T"),
            category: Category::Tab,
            action: CommandAction::NewTab,
        },
        Command {
            id: "tab.close",
            brief: "Close Tab",
            doc: "Close the currently focused tab.",
            keywords: &["destroy", "remove", "kill"],
            icon: Some('x'),
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::CloseTab,
        },
        Command {
            id: "tab.next",
            brief: "Next Tab",
            doc: "Focus the next tab, looping back to the first.",
            keywords: &["forward", "right"],
            icon: Some('>'),
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::NextTab,
        },
        Command {
            id: "tab.previous",
            brief: "Previous Tab",
            doc: "Focus the previous tab, looping back to the last.",
            keywords: &["back", "left"],
            icon: Some('<'),
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::PreviousTab,
        },
        Command {
            id: "tab.go_to",
            brief: "Go to Tab",
            doc: "Enter Tab mode to interactively pick a tab.",
            keywords: &["switch", "select", "jump"],
            icon: Some('#'),
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::GoToTab,
        },
        Command {
            id: "tab.go_to_1",
            brief: "Go to Tab 1",
            doc: "Switch directly to tab index 1.",
            keywords: &["one", "first"],
            icon: Some('1'),
            shortcut: Some("Alt+1"),
            category: Category::Tab,
            action: CommandAction::GoToTab1,
        },
        Command {
            id: "tab.go_to_2",
            brief: "Go to Tab 2",
            doc: "Switch directly to tab index 2.",
            keywords: &["two", "second"],
            icon: Some('2'),
            shortcut: Some("Alt+2"),
            category: Category::Tab,
            action: CommandAction::GoToTab2,
        },
        Command {
            id: "tab.go_to_3",
            brief: "Go to Tab 3",
            doc: "Switch directly to tab index 3.",
            keywords: &["three", "third"],
            icon: Some('3'),
            shortcut: Some("Alt+3"),
            category: Category::Tab,
            action: CommandAction::GoToTab3,
        },
        Command {
            id: "tab.go_to_4",
            brief: "Go to Tab 4",
            doc: "Switch directly to tab index 4.",
            keywords: &["four"],
            icon: Some('4'),
            shortcut: Some("Alt+4"),
            category: Category::Tab,
            action: CommandAction::GoToTab4,
        },
        Command {
            id: "tab.go_to_5",
            brief: "Go to Tab 5",
            doc: "Switch directly to tab index 5.",
            keywords: &["five"],
            icon: Some('5'),
            shortcut: Some("Alt+5"),
            category: Category::Tab,
            action: CommandAction::GoToTab5,
        },
        Command {
            id: "tab.rename",
            brief: "Rename Tab",
            doc: "Enter RenameTab mode to give the focused tab a new name.",
            keywords: &["name", "title", "label"],
            icon: Some('r'),
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::RenameTab,
        },
        // ===================== Pane =====================
        Command {
            id: "pane.new_right",
            brief: "New Pane (Right)",
            doc: "Split the focused pane horizontally, creating a new pane to the right.",
            keywords: &["split", "horizontal", "create", "vsplit"],
            icon: Some('|'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::NewPaneRight,
        },
        Command {
            id: "pane.new_down",
            brief: "New Pane (Down)",
            doc: "Split the focused pane vertically, creating a new pane below.",
            keywords: &["split", "vertical", "create", "hsplit"],
            icon: Some('-'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::NewPaneDown,
        },
        Command {
            id: "pane.close",
            brief: "Close Pane",
            doc: "Close the currently focused pane.",
            keywords: &["destroy", "kill", "remove"],
            icon: Some('x'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::ClosePane,
        },
        Command {
            id: "pane.move_focus_up",
            brief: "Move Focus Up",
            doc: "Move focus to the pane above the current one.",
            keywords: &["up", "north", "k"],
            icon: Some('^'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::MoveFocusUp,
        },
        Command {
            id: "pane.move_focus_down",
            brief: "Move Focus Down",
            doc: "Move focus to the pane below the current one.",
            keywords: &["down", "south", "j"],
            icon: Some('v'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::MoveFocusDown,
        },
        Command {
            id: "pane.move_focus_left",
            brief: "Move Focus Left",
            doc: "Move focus to the pane to the left of the current one.",
            keywords: &["left", "west", "h"],
            icon: Some('<'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::MoveFocusLeft,
        },
        Command {
            id: "pane.move_focus_right",
            brief: "Move Focus Right",
            doc: "Move focus to the pane to the right of the current one.",
            keywords: &["right", "east", "l"],
            icon: Some('>'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::MoveFocusRight,
        },
        Command {
            id: "pane.toggle_floating",
            brief: "Toggle Floating",
            doc: "Show or hide all floating panes in the current tab.",
            keywords: &["float", "layer", "overlay"],
            icon: Some('o'),
            shortcut: Some("Ctrl+G"),
            category: Category::Pane,
            action: CommandAction::ToggleFloating,
        },
        Command {
            id: "pane.toggle_fullscreen",
            brief: "Toggle Fullscreen",
            doc: "Expand the focused pane to fill the tab, or restore it.",
            keywords: &["maximize", "zoom", "full"],
            icon: Some('F'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::ToggleFullscreen,
        },
        Command {
            id: "pane.toggle_frames",
            brief: "Toggle Pane Frames",
            doc: "Toggle the UI frame drawn around panes.",
            keywords: &["border", "frame", "ui"],
            icon: Some('['),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::ToggleFrames,
        },
        Command {
            id: "pane.enter_resize",
            brief: "Enter Resize Mode",
            doc: "Switch to Resize mode so the focused pane can be resized with the arrow keys.",
            keywords: &["resize", "grow", "shrink", "size"],
            icon: Some('R'),
            shortcut: None,
            category: Category::Pane,
            action: CommandAction::EnterResize,
        },
        // ===================== Session =====================
        Command {
            id: "session.switch",
            brief: "Switch Session",
            doc: "Enter Session mode to switch between or create sessions.",
            keywords: &["change", "connect", "attach"],
            icon: Some('~'),
            shortcut: None,
            category: Category::Session,
            action: CommandAction::SwitchSession,
        },
        Command {
            id: "session.detach",
            brief: "Detach Session",
            doc: "Detach this client from the active session, leaving it running in the background.",
            keywords: &["background", "leave", "logout"],
            icon: Some('D'),
            shortcut: None,
            category: Category::Session,
            action: CommandAction::DetachSession,
        },
        Command {
            id: "session.kill",
            brief: "Kill Session",
            doc: "Enter Session mode to terminate a session. Destructive — the UI asks for Y/N confirmation.",
            keywords: &["destroy", "terminate", "end"],
            icon: Some('K'),
            shortcut: None,
            category: Category::Session,
            action: CommandAction::KillSession,
        },
        // ===================== Mode =====================
        Command {
            id: "mode.pane",
            brief: "Pane Mode",
            doc: "Switch to Pane mode (split, close, move between panes).",
            keywords: &["enter", "switch"],
            icon: Some('P'),
            shortcut: Some("Ctrl+P"),
            category: Category::Mode,
            action: CommandAction::EnterPaneMode,
        },
        Command {
            id: "mode.tab",
            brief: "Tab Mode",
            doc: "Switch to Tab mode (create, close, switch tabs).",
            keywords: &["enter", "switch"],
            icon: Some('T'),
            shortcut: Some("Ctrl+T"),
            category: Category::Mode,
            action: CommandAction::EnterTabMode,
        },
        Command {
            id: "mode.move",
            brief: "Move Mode",
            doc: "Switch to Move mode to reorder panes within the tab.",
            keywords: &["reorder", "arrange"],
            icon: Some('M'),
            shortcut: None,
            category: Category::Mode,
            action: CommandAction::EnterMoveMode,
        },
        Command {
            id: "mode.search",
            brief: "Search Mode",
            doc: "Switch to Search mode to find text in the scrollback of the focused pane.",
            keywords: &["find", "grep", "lookup"],
            icon: Some('/'),
            shortcut: Some("Ctrl+S"),
            category: Category::Mode,
            action: CommandAction::EnterSearchMode,
        },
        Command {
            id: "mode.locked",
            brief: "Locked Mode",
            doc: "Switch to Locked mode — all shortcuts except the unlock key are disabled.",
            keywords: &["lock", "passthrough", "raw"],
            icon: Some('L'),
            shortcut: Some("Ctrl+G"),
            category: Category::Mode,
            action: CommandAction::EnterLockedMode,
        },
        Command {
            id: "mode.normal",
            brief: "Normal Mode",
            doc: "Return to Normal mode from any other mode.",
            keywords: &["default", "reset", "esc"],
            icon: Some('N'),
            shortcut: None,
            category: Category::Mode,
            action: CommandAction::EnterNormalMode,
        },
        // ===================== System =====================
        Command {
            id: "system.reload_config",
            brief: "Reload Config",
            doc: "Re-apply the zellij configuration. (MVP note: issues an empty reconfigure call pending a dedicated reload API.)",
            keywords: &["reconfigure", "refresh", "settings"],
            icon: Some('*'),
            shortcut: None,
            category: Category::System,
            action: CommandAction::ReloadConfig,
        },
        Command {
            id: "system.quit",
            brief: "Quit Zellij",
            doc: "Completely quit Zellij for this and all other connected clients. Destructive — the UI asks for Y/N confirmation.",
            keywords: &["exit", "shutdown", "close"],
            icon: Some('Q'),
            shortcut: Some("Ctrl+Q"),
            category: Category::System,
            action: CommandAction::Quit,
        },
    ]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Mirror of the dispatch match that maps each variant to a stable name.
    /// Compiles ONLY if every variant is listed (no wildcard), so this doubles
    /// as the exhaustiveness proof for `CommandAction::dispatch`.
    fn action_name(a: &CommandAction) -> &'static str {
        match a {
            CommandAction::NewTab => "NewTab",
            CommandAction::CloseTab => "CloseTab",
            CommandAction::NextTab => "NextTab",
            CommandAction::PreviousTab => "PreviousTab",
            CommandAction::GoToTab => "GoToTab",
            CommandAction::GoToTab1 => "GoToTab1",
            CommandAction::GoToTab2 => "GoToTab2",
            CommandAction::GoToTab3 => "GoToTab3",
            CommandAction::GoToTab4 => "GoToTab4",
            CommandAction::GoToTab5 => "GoToTab5",
            CommandAction::RenameTab => "RenameTab",
            CommandAction::NewPaneRight => "NewPaneRight",
            CommandAction::NewPaneDown => "NewPaneDown",
            CommandAction::ClosePane => "ClosePane",
            CommandAction::MoveFocusUp => "MoveFocusUp",
            CommandAction::MoveFocusDown => "MoveFocusDown",
            CommandAction::MoveFocusLeft => "MoveFocusLeft",
            CommandAction::MoveFocusRight => "MoveFocusRight",
            CommandAction::ToggleFloating => "ToggleFloating",
            CommandAction::ToggleFullscreen => "ToggleFullscreen",
            CommandAction::ToggleFrames => "ToggleFrames",
            CommandAction::EnterResize => "EnterResize",
            CommandAction::SwitchSession => "SwitchSession",
            CommandAction::DetachSession => "DetachSession",
            CommandAction::KillSession => "KillSession",
            CommandAction::EnterPaneMode => "EnterPaneMode",
            CommandAction::EnterTabMode => "EnterTabMode",
            CommandAction::EnterMoveMode => "EnterMoveMode",
            CommandAction::EnterSearchMode => "EnterSearchMode",
            CommandAction::EnterLockedMode => "EnterLockedMode",
            CommandAction::EnterNormalMode => "EnterNormalMode",
            CommandAction::ReloadConfig => "ReloadConfig",
            CommandAction::Quit => "Quit",
        }
    }

    #[test]
    fn registry_is_non_empty() {
        let cmds = all_commands();
        assert!(!cmds.is_empty(), "all_commands() must return commands");
        assert_eq!(
            cmds.len(),
            33,
            "expected 33 MVP commands, got {}",
            cmds.len()
        );
    }

    #[test]
    fn every_command_has_a_non_empty_brief() {
        for cmd in all_commands() {
            assert!(
                !cmd.brief.trim().is_empty(),
                "command `{}` has an empty brief",
                cmd.id
            );
            assert!(
                !cmd.doc.trim().is_empty(),
                "command `{}` has an empty doc",
                cmd.id
            );
        }
    }

    #[test]
    fn every_command_id_is_unique() {
        let cmds = all_commands();
        let ids: HashSet<&str> = cmds.iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), cmds.len(), "duplicate command ids detected");
    }

    #[test]
    fn every_action_variant_is_handled() {
        // Every variant produced by all_commands() must be recognised by the
        // exhaustive matcher (which itself mirrors dispatch's arm set).
        let mut seen: HashSet<&'static str> = HashSet::new();
        for cmd in all_commands() {
            let name = action_name(&cmd.action);
            assert!(
                !name.is_empty(),
                "unhandled action for command `{}`",
                cmd.id
            );
            seen.insert(name);
        }
        // Sanity: the registry exercises all 33 distinct variants.
        assert_eq!(
            seen.len(),
            33,
            "expected 33 distinct actions, got {}",
            seen.len()
        );
    }

    #[test]
    fn all_categories_are_represented() {
        let mut cats = HashSet::new();
        for cmd in all_commands() {
            cats.insert(cmd.category);
        }
        assert_eq!(cats.len(), 5, "expected all 5 categories to be represented");
        assert!(cats.contains(&Category::Tab));
        assert!(cats.contains(&Category::Pane));
        assert!(cats.contains(&Category::Session));
        assert!(cats.contains(&Category::Mode));
        assert!(cats.contains(&Category::System));
    }

    #[test]
    fn category_badges_and_icons_are_non_empty() {
        for cat in [
            Category::Tab,
            Category::Pane,
            Category::Session,
            Category::Mode,
            Category::System,
        ] {
            assert!(!cat.badge().is_empty());
            assert_ne!(cat.icon(), '\0');
            assert!(cat.priority() >= 1 && cat.priority() <= 5);
        }
    }

    #[test]
    fn effective_icon_falls_back_to_category() {
        let cmd = Command {
            id: "test",
            brief: "Test",
            doc: "Test",
            keywords: &[],
            icon: None,
            shortcut: None,
            category: Category::Tab,
            action: CommandAction::NewTab,
        };
        assert_eq!(cmd.effective_icon(), Category::Tab.icon());
    }
}
