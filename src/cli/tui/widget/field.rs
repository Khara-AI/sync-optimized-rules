//! Text fields and yes/no questions.

use super::{Key, Line, Step, Tone, Widget, answered, question};

type Check<'a> = Box<dyn Fn(&str) -> Result<(), String> + 'a>;

/// A one-line text field, checked when validated.
pub struct Input<'a> {
    title: String,
    pub value: Vec<char>,
    cursor: usize,
    check: Check<'a>,
    error: Option<String>,
}

impl<'a> Input<'a> {
    pub fn new(
        title: &str,
        default: &str,
        check: impl Fn(&str) -> Result<(), String> + 'a,
    ) -> Self {
        let value: Vec<char> = default.chars().collect();
        Self {
            title: title.into(),
            cursor: value.len(),
            value,
            check: Box::new(check),
            error: None,
        }
    }

    pub fn text(&self) -> String {
        self.value.iter().collect::<String>().trim().to_owned()
    }

    fn prefix(&self) -> usize {
        2 + self.title.chars().count() + 3
    }
}

impl Widget for Input<'_> {
    fn key(&mut self, key: Key, _page: usize) -> Step {
        self.error = None;
        match key {
            Key::Char(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += 1;
            }
            Key::Space => {
                self.value.insert(self.cursor, ' ');
                self.cursor += 1;
            }
            Key::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.value.remove(self.cursor);
            }
            Key::Left => self.cursor = self.cursor.saturating_sub(1),
            Key::Right => self.cursor = (self.cursor + 1).min(self.value.len()),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = self.value.len(),
            Key::Clear => {
                self.value.clear();
                self.cursor = 0;
            }
            Key::Enter => match (self.check)(&self.text()) {
                Ok(()) => return Step::Done,
                Err(e) => self.error = Some(e),
            },
            Key::Esc => return Step::Back,
            _ => {}
        }
        Step::Stay
    }

    fn lines(&mut self, _height: usize) -> Vec<Line> {
        let mut help = vec![(
            Tone::Dim,
            "  enter confirm · esc back · ctrl+u clear".into(),
        )];
        if let Some(e) = &self.error {
            help.push((Tone::Bad, format!("  ✖ {e}")));
        }
        let mut line = question(&self.title);
        line.push((Tone::Dim, " › ".into()));
        line.push((Tone::Plain, self.value.iter().collect()));
        vec![line, help]
    }

    fn cursor(&self) -> Option<(usize, usize)> {
        Some((0, self.prefix() + self.cursor))
    }

    fn summary(&self) -> Line {
        let text = self.text();
        answered(&self.title, if text.is_empty() { "—" } else { &text })
    }
}

/// A yes/no question answered with one key.
pub struct Confirm {
    title: String,
    pub value: bool,
}

impl Confirm {
    pub fn new(title: &str, default: bool) -> Self {
        Self {
            title: title.into(),
            value: default,
        }
    }
}

impl Widget for Confirm {
    fn key(&mut self, key: Key, _page: usize) -> Step {
        match key {
            Key::Char('y' | 'Y' | 'o' | 'O') => self.value = true,
            Key::Char('n' | 'N') => self.value = false,
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Space => {
                self.value = !self.value;
                return Step::Stay;
            }
            Key::Enter => {}
            Key::Esc => return Step::Back,
            _ => return Step::Stay,
        }
        Step::Done
    }

    fn lines(&mut self, _height: usize) -> Vec<Line> {
        let pick = |on: bool, word: &str| {
            if on {
                (Tone::Accent, format!("[{word}]"))
            } else {
                (Tone::Dim, format!(" {word} "))
            }
        };
        let mut line = question(&self.title);
        line.push((Tone::Plain, "  ".into()));
        line.push(pick(self.value, "yes"));
        line.push(pick(!self.value, "no"));
        line.push((Tone::Dim, "   y/n · ←→ · enter".into()));
        vec![line]
    }

    fn summary(&self) -> Line {
        answered(&self.title, if self.value { "yes" } else { "no" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tui::widget::text;

    #[test]
    fn inputs_edit_and_validate() {
        let mut i = Input::new("name", "ab", |s| {
            if s.contains(' ') {
                Err("no spaces".into())
            } else {
                Ok(())
            }
        });
        i.key(Key::Left, 0);
        i.key(Key::Char('x'), 0);
        assert_eq!(i.text(), "axb");
        i.key(Key::Space, 0);
        assert_eq!(i.key(Key::Enter, 0), Step::Stay);
        assert!(text(&i.lines(5))[1].contains("no spaces"));
        i.key(Key::Clear, 0);
        i.key(Key::Char('z'), 0);
        assert_eq!(i.key(Key::Enter, 0), Step::Done);
        assert_eq!(i.text(), "z");
    }

    #[test]
    fn confirm_takes_one_key() {
        let mut c = Confirm::new("sure", true);
        assert_eq!(c.key(Key::Char('n'), 0), Step::Done);
        assert!(!c.value);
        assert_eq!(c.key(Key::Right, 0), Step::Stay);
        assert_eq!(c.key(Key::Enter, 0), Step::Done);
        assert!(c.value);
    }
}
