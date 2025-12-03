use std::path::PathBuf;

use color_eyre::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    DefaultTerminal, Frame,
};
use rocraters::ro_crate::{rocrate::RoCrate, write::is_not_url};

use crate::error::TuiError;

/// App holds the state of the application
pub struct App {
    /// Current value of the input box
    input: String,
    /// Position of cursor in the editor area.
    character_index: usize,
    /// Current input mode
    input_mode: InputMode,
    /// History of recorded messages
    history: Vec<String>,

    /// ROCrate
    view: String,
    state: Vec<(String, RoCrate)>,
    cursor: usize,

    /// Scrolling state
    vertical_scroll_state: ScrollbarState,
    horizontal_scroll_state: ScrollbarState,
    vertical_scroll: usize,
    horizontal_scroll: usize,
}

enum InputMode {
    Normal,
    Editing,
    Scrolling,
}

impl App {
    pub const fn new() -> Self {
        Self {
            input: String::new(),
            input_mode: InputMode::Normal,
            history: Vec::new(),
            character_index: 0,
            state: vec![],
            view: String::new(),
            cursor: 0,
            vertical_scroll: 0,
            horizontal_scroll: 0,
            vertical_scroll_state: ScrollbarState::new(0),
            horizontal_scroll_state: ScrollbarState::new(0),
        }
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.character_index.saturating_sub(1);
        self.character_index = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.character_index.saturating_add(1);
        self.character_index = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        let index = self.byte_index();
        self.input.insert(index, new_char);
        self.move_cursor_right();
    }

    /// Returns the byte index based on the character position.
    ///
    /// Since each character in a string can be contain multiple bytes, it's necessary to calculate
    /// the byte index based on the index of the character.
    fn byte_index(&self) -> usize {
        self.input
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.character_index)
            .unwrap_or(self.input.len())
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.character_index != 0;
        if is_not_cursor_leftmost {
            // Method "remove" is not used on the saved text for deleting the selected char.
            // Reason: Using remove on String works on bytes instead of the chars.
            // Using remove would require special care because of char boundaries.

            let current_index = self.character_index;
            let from_left_to_current_index = current_index - 1;

            // Getting all characters before the selected character.
            let before_char_to_delete = self.input.chars().take(from_left_to_current_index);
            // Getting all characters after selected character.
            let after_char_to_delete = self.input.chars().skip(current_index);

            // Put all characters together except the selected one.
            // By leaving the selected one out, it is forgotten and therefore deleted.
            self.input = before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.input.chars().count())
    }

    fn reset_cursor(&mut self) {
        self.character_index = 0;
    }

    fn submit_message(&mut self) {
        if let Err(err) = self.handle_commands() {
            self.view = err.to_string();
        }

        self.history.push(self.input.clone());
        self.input.clear();
        self.reset_cursor();
    }

    fn handle_commands(&mut self) -> Result<(), TuiError> {
        let (cmd, arg) = self.input.split_once(' ').unwrap_or((&self.input, ""));
        match (cmd, arg) {
            ("load", path) => {
                let rocrate = if is_not_url(path) {
                    let crate_path = if path.ends_with('/') {
                        PathBuf::from(format!("{}ro-crate-metadata.json", path))
                    } else {
                        PathBuf::from(format!("{}/ro-crate-metadata.json", path))
                    };
                    rocraters::ro_crate::read::read_crate(&crate_path, 0)
                } else {
                    let url = url::Url::parse(path)?;
                    rocraters::ro_crate::read::load_remote(url, 0)
                };

                self.state.push((path.to_string(), rocrate?));
                self.view = "Loaded crate ...".to_string();
                self.cursor = self.state.len() - 1;
            }
            ("ls", _) => {
                if let Some(rocrate) = self.state.get(self.cursor) {
                    self.view = serde_json::to_string_pretty(rocrate)?;
                } else {
                    return Err(TuiError::NoState);
                }
            }
            ("get", id) => match self.state.get(self.cursor) {
                Some((_, rocrate)) => match rocrate.get_entity(id) {
                    Some(entity) => {
                        self.view = serde_json::to_string_pretty(entity)?;
                    }
                    None => {
                        return Err(TuiError::IdNotFound(id.to_string()));
                    }
                },
                None => {
                    return Err(TuiError::NoState);
                }
            },
            ("cd", subcrate_id) => {
                match subcrate_id {
                    ".." => {
                        self.view = format!("Changed to upper level");
                        self.cursor -= 1;
                    }
                    "/" => {
                        self.view = format!("Changed to root level");
                        self.cursor = 0;
                    }
                    _ => {
                        // TODO: Improve!
                        // This is only a hacky way of checking if subcrate was already loaded
                        if let Some(already_loaded_idx) =
                            self.state.iter().enumerate().find_map(|(idx, (p, _))| {
                                if p.contains(subcrate_id) {
                                    Some(idx)
                                } else {
                                    None
                                }
                            })
                        {
                            self.view = format!("Loaded subcrate {subcrate_id}");
                            self.cursor = already_loaded_idx;
                            return Ok(());
                        }

                        let (subdir, subcrate) = if is_not_url(subcrate_id) {
                            if let Some((upperpath, _)) = self.state.get(self.cursor) {
                                let (subdir, subpath) = if upperpath.ends_with('/') {
                                    (
                                        format!("{upperpath}{subcrate_id}"),
                                        format!("{upperpath}{subcrate_id}ro-crate-metadata.json"),
                                    )
                                } else {
                                    (
                                        format!("{upperpath}/{subcrate_id}"),
                                        format!("{upperpath}/{subcrate_id}/ro-crate-metadata.json",),
                                    )
                                };
                                (
                                    subdir,
                                    rocraters::ro_crate::read::read_crate(
                                        &PathBuf::from(&subpath),
                                        0,
                                    ),
                                )
                            } else {
                                return Err(TuiError::IdNotFound(subcrate_id.to_string()));
                            }
                        } else {
                            let url =
                                url::Url::parse(&format!("{subcrate_id}ro-crate-metadata.json"))?;
                            (
                                url.to_string(),
                                rocraters::ro_crate::read::load_remote(url, 0),
                            )
                        };
                        self.state.push((subdir, subcrate?));
                        self.view = format!("Loaded subcrate {subcrate_id}");
                        self.cursor = self.state.len() - 1;
                    }
                }
            }
            ("pwd", _) => {
                self.view = match self.state.get(self.cursor) {
                    Some((p, _)) => p.to_string(),
                    None => return Err(TuiError::NoState),
                };
            }
            ("help", _) => {
                self.view = format!(
                    "Commands:
  load <url|path>    Load RO-Crate from URL or .zip file
  ls                 List hasPart of current dataset/folder
  ls -a              List all entities (data + contextual)
  get <@id>          Pretty-print JSON for entity
  cd <id>            Enter subcrate or folder
  cd ..              Return to parent
  cd /               Return to root crate
  pwd                Print current path
  help               Show this message
Supported formats:
  - .zip archives containing ro-crate-metadata.json
  - Direct URLs to RO-Crate archives
  - DOIs resolving to Zenodo/similar repositories"
                );
            }
            (cmd, arg) => {
                self.view = format!(
                    "Unkown command: {cmd} {arg}

Commands:
  load <url|path>    Load RO-Crate from URL or .zip file
  ls                 List hasPart of current dataset/folder
  ls -a              List all entities (data + contextual)
  get <@id>          Pretty-print JSON for entity
  cd <id>            Enter subcrate or folder
  cd ..              Return to parent
  cd /               Return to root crate
  pwd                Print current path
  help               Show this message
Supported formats:
  - .zip archives containing ro-crate-metadata.json
  - Direct URLs to RO-Crate archives
  - DOIs resolving to Zenodo/similar repositories"
                );
            }
        };

        Ok(())
    }

    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if let Event::Key(key) = event::read()? {
                match self.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('e') => {
                            self.input_mode = InputMode::Editing;
                        }
                        KeyCode::Char('q') => {
                            return Ok(());
                        }
                        KeyCode::Char('v') => {
                            self.input_mode = InputMode::Scrolling;
                        }
                        _ => {}
                    },
                    InputMode::Editing if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Enter => self.submit_message(),
                        KeyCode::Char(to_insert) => self.enter_char(to_insert),
                        KeyCode::Backspace => self.delete_char(),
                        KeyCode::Left => self.move_cursor_left(),
                        KeyCode::Right => self.move_cursor_right(),
                        KeyCode::Esc => self.input_mode = InputMode::Normal,
                        _ => {}
                    },
                    InputMode::Scrolling if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            self.vertical_scroll = self.vertical_scroll.saturating_add(1);
                            self.vertical_scroll_state =
                                self.vertical_scroll_state.position(self.vertical_scroll);
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
                            self.vertical_scroll_state =
                                self.vertical_scroll_state.position(self.vertical_scroll);
                        }
                        KeyCode::Char('h') | KeyCode::Left => {
                            self.horizontal_scroll = self.horizontal_scroll.saturating_sub(1);
                            self.horizontal_scroll_state = self
                                .horizontal_scroll_state
                                .position(self.horizontal_scroll);
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            self.horizontal_scroll = self.horizontal_scroll.saturating_add(1);
                            self.horizontal_scroll_state = self
                                .horizontal_scroll_state
                                .position(self.horizontal_scroll);
                        }
                        KeyCode::Esc => self.input_mode = InputMode::Normal,
                        _ => {}
                    },

                    InputMode::Editing => {}
                    _ => {}
                }
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let vertical = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ]);
        let [help_area, input_area, view_area] = vertical.areas(frame.area());

        let (msg, style) = match self.input_mode {
            InputMode::Normal => (
                vec![
                    "Press ".into(),
                    "q".bold(),
                    " to exit, ".into(),
                    "v".bold(),
                    " to start scrolling, ".bold(),
                    "e".bold(),
                    " to start editing.".bold(),
                ],
                Style::default().add_modifier(Modifier::RAPID_BLINK),
            ),
            InputMode::Editing => (
                vec![
                    "Press ".into(),
                    "Esc".bold(),
                    " to stop editing, ".into(),
                    "Enter".bold(),
                    " to record the message".into(),
                ],
                Style::default(),
            ),

            InputMode::Scrolling => (
                vec![
                    "Press ".into(),
                    "Esc".bold(),
                    " to stop scrolling, ".into(),
                    "h".bold(),
                    " to scroll left, ".into(),
                    "l".bold(),
                    " to scroll right, ".into(),
                    "j".bold(),
                    " to scroll down, ".into(),
                    "k".bold(),
                    " to scroll up.".into(),
                ],
                Style::default(),
            ),
        };
        let text = Text::from(Line::from(msg)).patch_style(style);
        let help_message = Paragraph::new(text);
        frame.render_widget(help_message, help_area);

        let input = Paragraph::new(self.input.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Yellow),
                InputMode::Scrolling => Style::default(),
            })
            .block(Block::bordered().title("Input"));
        frame.render_widget(input, input_area);
        match self.input_mode {
            // Make the cursor visible and ask ratatui to put it at the specified coordinates after
            // rendering
            #[allow(clippy::cast_possible_truncation)]
            InputMode::Editing => frame.set_cursor_position(Position::new(
                // Draw the cursor at the current position in the input field.
                // This position is can be controlled via the left and right arrow key
                input_area.x + self.character_index as u16 + 1,
                // Move one line down, from the border to the input line
                input_area.y + 1,
            )),

            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            _ => {}
        }

        let title = format!(
            "Command: {}",
            self.history.last().cloned().unwrap_or(String::new())
        );

        let style = if matches!(self.input_mode, InputMode::Scrolling) {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let mut horizontal_len = 0;
        let mut vertical_len = 0;
        for line in self.view.lines() {
            vertical_len += 1;
            if line.len() > horizontal_len {
                horizontal_len = line.len()
            };
        }

        self.vertical_scroll_state = self.vertical_scroll_state.content_length(vertical_len);
        self.horizontal_scroll_state = self.horizontal_scroll_state.content_length(horizontal_len);

        let view = Paragraph::new(self.view.clone())
            .block(Block::bordered().title(title).style(style))
            .style(style)
            .scroll((self.vertical_scroll as u16, 0));
        frame.render_widget(view, view_area);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓")),
            view_area,
            &mut self.vertical_scroll_state,
        );
    }
}
