use std::path::PathBuf;

use color_eyre::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Position},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, Paragraph},
    DefaultTerminal, Frame,
};
use rocraters::ro_crate::rocrate::RoCrate;

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
    state: Vec<(PathBuf, RoCrate)>,
    cursor: usize,
}

enum InputMode {
    Normal,
    Editing,
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
        let (cmd, arg) = self.input.split_once(' ').unwrap_or((&self.input, ""));
        match (cmd, arg) {
            ("load", path) => {
                let crate_path = if path.ends_with('/') {
                    PathBuf::from(format!("{}ro-crate-metadata.json", path))
                } else {
                    PathBuf::from(format!("{}/ro-crate-metadata.json", path))
                };
                match rocraters::ro_crate::read::read_crate(&crate_path, 0) {
                    Ok(rocrate) => {
                        self.state.push((PathBuf::from(path), rocrate));
                        self.view = "Loaded crate ...".to_string();
                        self.cursor = self.state.len() - 1;
                    }
                    Err(err) => {
                        self.view = format!("{err:?}");
                    }
                }
            }
            ("ls", _) => match self.state.get(self.cursor) {
                Some(rocrate) => {
                    self.view = match serde_json::to_string_pretty(rocrate) {
                        Ok(prettyfied) => prettyfied,
                        Err(err) => format!("{err:?}"),
                    };
                }
                None => {
                    self.view = format!("No crate loaded yet");
                }
            },
            ("get", id) => match self.state.get(self.cursor) {
                Some((_, rocrate)) => match rocrate.get_entity(id) {
                    Some(entity) => {
                        self.view = match serde_json::to_string_pretty(entity) {
                            Ok(prettyfied) => prettyfied,
                            Err(err) => format!("{err:?}"),
                        };
                    }
                    None => {
                        self.view = format!("Nothing found for id: {}", id);
                    }
                },
                None => {
                    self.view = format!("No crate loaded yet");
                }
            },
            ("cd", subcrate_id) => {
                if let Some((upperpath, _)) = self.state.get(self.cursor) {
                    let (subdir, subpath) = if upperpath.to_str().unwrap().ends_with('/') {
                        (
                            upperpath.join(subcrate_id),
                            upperpath.join(format!("{}ro-crate-metadata.json", subcrate_id)),
                        )
                    } else {
                        (
                            upperpath.join(format!("/{}", subcrate_id)),
                            upperpath.join(format!("{}/ro-crate-metadata.json", subcrate_id)),
                        )
                    };
                    match rocraters::ro_crate::read::read_crate(&subpath, 0) {
                        Ok(subcrate) => {
                            self.state.push((subdir, subcrate));
                            self.view = format!("Loaded subcrate {subcrate_id}");
                            self.cursor = self.state.len() - 1;
                        }
                        Err(err) => {
                            self.view = format!("{err:?}\n{subpath:?}");
                        }
                    }
                }
            }
            (cmd, arg) => {
                self.view = format!("Unkown command: {cmd} {arg}");
            }
        };

        self.history.push(self.input.clone());
        self.input.clear();
        self.reset_cursor();
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
                    InputMode::Editing => {}
                }
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
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
        };
        let text = Text::from(Line::from(msg)).patch_style(style);
        let help_message = Paragraph::new(text);
        frame.render_widget(help_message, help_area);

        let input = Paragraph::new(self.input.as_str())
            .style(match self.input_mode {
                InputMode::Normal => Style::default(),
                InputMode::Editing => Style::default().fg(Color::Yellow),
            })
            .block(Block::bordered().title("Input"));
        frame.render_widget(input, input_area);
        match self.input_mode {
            // Hide the cursor. `Frame` does this by default, so we don't need to do anything here
            InputMode::Normal => {}

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
        }

        let title = format!(
            "Command: {}",
            self.history.last().cloned().unwrap_or(String::new())
        );

        let view = Paragraph::new(self.view.clone()).block(Block::bordered().title(title));
        frame.render_widget(view, view_area);
    }
}
