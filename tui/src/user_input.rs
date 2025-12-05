use color_eyre::Result;
use ratatui::{
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    DefaultTerminal, Frame,
};
use rocrate_indexer::CrateIndex;
use rocraters::ro_crate::rocrate::RoCrate;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// App holds the state of the application
pub struct App {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    pub idxer: CrateIndex,
    /// Current value of the input box
    pub input: String,
    /// Position of cursor in the editor area.
    pub character_index: usize,
    /// Current input mode
    pub input_mode: InputMode,
    /// History of recorded messages
    pub history: Vec<String>,
    pub completion: CompletionState,

    /// ROCrate
    /// View of the current item
    pub view: String,
    /// All loaded rocrates
    pub state: Vec<(String, RoCrate)>,
    /// Cursor pointing to the current entry in state
    pub cursor: usize,

    /// Scrolling state
    pub vertical_scroll_state: ScrollbarState,
    pub horizontal_scroll_state: ScrollbarState,
    pub vertical_scroll: usize,
    pub horizontal_scroll: usize,
}

#[derive(Default)]
pub struct CompletionState {
    pub subs_idx: usize,
    pub subs: Vec<String>,
    pub ids_idx: usize,
    pub ids: Vec<String>,
}

pub enum InputMode {
    Normal,
    Editing,
    Scrolling,
}

impl App {
    pub fn new() -> Self {
        let idxer = rocrate_indexer::CrateIndex::open_or_create().unwrap();

        // Load these once at the start of your program
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();

        Self {
            syntax_set: ps,
            theme_set: ts,
            idxer,
            input: String::new(),
            input_mode: InputMode::Normal,
            history: Vec::new(),
            character_index: 0,
            state: vec![],
            completion: CompletionState::default(),
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

    fn complete_command(&mut self) {
        if self.input.contains("cd") {
            if self.completion.subs.len() > self.completion.subs_idx {
                self.completion.subs_idx += 1;
            } else {
                self.completion.subs_idx = 0;
            }
            self.input = format!(
                "cd {}",
                self.completion
                    .subs
                    .get(self.completion.subs_idx)
                    .unwrap_or(&"".to_string())
            )
        } else if self.input.contains("get") {
            if self.completion.ids.len() > self.completion.ids_idx {
                self.completion.ids_idx += 1;
            } else {
                self.completion.ids_idx = 0;
            }

            self.input = format!(
                "get {}",
                self.completion
                    .ids
                    .get(self.completion.ids_idx)
                    .unwrap_or(&"".to_string())
            )
        }
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
                        KeyCode::Tab => self.complete_command(),
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

    fn render_help(&self, help_area: Rect, frame: &mut Frame) {
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
    }

    fn render_input(&self, input_area: Rect, frame: &mut Frame) {
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
    }

    fn render_view(&mut self, view_area: Rect, frame: &mut Frame) {
        let syntax = self.syntax_set.find_syntax_by_extension("json").unwrap();
        let mut h = HighlightLines::new(syntax, &self.theme_set.themes["base16-ocean.dark"]);
        let mut text = Text::default();
        for line in LinesWithEndings::from(&self.view) {
            // LinesWithEndings enables use of newlines mode
            let ranges: Vec<_> = h
                .highlight_line(line, &self.syntax_set)
                .unwrap()
                .iter()
                .map(|(synstyle, content)| -> Span {
                    Span::default().content(*content).style(Style {
                        fg: Some(Color::Rgb(
                            synstyle.foreground.r,
                            synstyle.foreground.g,
                            synstyle.foreground.b,
                        )),
                        bg: None,
                        ..Default::default()
                    })
                })
                .collect();
            for span in ranges {
                text.push_span(span);
            }
            text.push_line(Line::from("\n"));
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
        for line in text.iter() {
            vertical_len += 1;
            if line.width() > horizontal_len {
                horizontal_len = line.width()
            };
        }

        self.vertical_scroll_state = self.vertical_scroll_state.content_length(vertical_len);
        self.horizontal_scroll_state = self.horizontal_scroll_state.content_length(horizontal_len);

        let view = Paragraph::new(text.clone())
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

    fn draw(&mut self, frame: &mut Frame) {
        let vertical = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Min(1),
        ]);
        let [help_area, input_area, view_area] = vertical.areas(frame.area());

        self.render_help(help_area, frame);

        self.render_input(input_area, frame);

        self.render_view(view_area, frame);
    }
}
