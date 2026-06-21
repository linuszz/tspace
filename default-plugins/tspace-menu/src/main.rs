mod backend;
mod click;
mod commands;
mod screens;

use std::collections::BTreeMap;

use backend::ZellijBackend;
use ratatui::layout::{Rect, Size};
use ratatui::Terminal;
use screens::{ActiveScreen, CommandPaletteState, PaletteAction};
use zellij_tile::prelude::*;

pub struct State {
    terminal: Option<Terminal<ZellijBackend>>,
    screen: ActiveScreen,
    palette: CommandPaletteState,
    is_visible: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            terminal: None,
            screen: ActiveScreen::Palette,
            palette: CommandPaletteState::new(),
            is_visible: true,
        }
    }
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, _config: BTreeMap<String, String>) {
        subscribe(&[
            EventType::InterceptedKeyPress,
            EventType::Mouse,
            EventType::ModeUpdate,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::SessionUpdate,
            EventType::Visible,
        ]);
        intercept_key_presses();
        set_selectable(false);
        self.terminal = Terminal::new(ZellijBackend::new(Size::new(80, 24))).ok();
        self.palette.refresh_matches();
    }

    fn update(&mut self, event: Event) -> bool {
        if self.screen != ActiveScreen::Hidden {
            intercept_key_presses();
        }

        if let Event::Visible(v) = event {
            if v && !self.is_visible {
                self.screen = ActiveScreen::Palette;
                self.palette.query.clear();
                self.palette.selected = 0;
                self.palette.scroll_offset = 0;
                self.palette.refresh_matches();
            }
            self.is_visible = v;
            return true;
        }

        if let Event::InterceptedKeyPress(key) = event {
            match self.palette.on_key(&key) {
                PaletteAction::Execute(idx) => {
                    let commands = crate::commands::all_commands();
                    if idx < commands.len() {
                        commands[idx].action.dispatch();
                    }
                    self.screen = ActiveScreen::Hidden;
                    self.is_visible = false;
                    hide_self();
                    return false;
                },
                PaletteAction::Close => {
                    self.screen = ActiveScreen::Hidden;
                    self.is_visible = false;
                    hide_self();
                    return false;
                },
                PaletteAction::Continue | PaletteAction::Noop => {
                    return true;
                },
            }
        }

        if let Event::Mouse(mouse) = event {
            match mouse {
                Mouse::LeftClick(..) => {
                    if let Some((row, col)) = mouse.position() {
                        if let Some(match_idx) = self.palette.on_click(row, col) {
                            let commands = crate::commands::all_commands();
                            if match_idx < self.palette.matches.len() {
                                let cmd_idx = self.palette.matches[match_idx].cmd_index;
                                if cmd_idx < commands.len() {
                                    commands[cmd_idx].action.dispatch();
                                }
                            }
                            self.screen = ActiveScreen::Hidden;
                            self.is_visible = false;
                            hide_self();
                            return false;
                        }
                    }
                },
                Mouse::Hover(..) => {
                    if let Some((row, col)) = mouse.position() {
                        if let Some(idx) = self.palette.on_hover(row, col) {
                            self.palette.selected = idx;
                            return true;
                        }
                    }
                },
                Mouse::ScrollDown(_) => {
                    self.palette.move_selection_down();
                    return true;
                },
                Mouse::ScrollUp(_) => {
                    self.palette.move_selection_up();
                    return true;
                },
                _ => {},
            }
        }

        true
    }

    fn render(&mut self, rows: usize, cols: usize) {
        let Some(t) = self.terminal.as_mut() else {
            return;
        };
        let _ = t.resize(Rect::new(0, 0, cols as u16, rows as u16));
        let screen = self.screen;
        // Borrow `palette` before `t.draw()` borrows `self.terminal`
        // mutably — disjoint field borrows, so the closure can capture
        // the immutable palette reference without conflict.
        let palette = &self.palette;
        let _ = t.draw(|frame| {
            if screen == ActiveScreen::Hidden {
                return;
            }
            let area = centered_rect(80, 60, frame.area());
            palette.render(area, frame.buffer_mut());
        });
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let h = area.height * percent_y / 100;
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}
