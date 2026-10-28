//! Widget state and drawing, free of terminal I/O so it can be tested.

mod field;
mod list;

pub use field::{Confirm, Input};
pub use list::{Item, List};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Left,
    Right,
    Enter,
    Esc,
    Space,
    Backspace,
    Tab,
    Char(char),
    /// Ctrl+A: tick or untick every visible item.
    All,
    /// Ctrl+U: empty the text field.
    Clear,
    /// Ctrl+C: leave sor.
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Dim,
    Accent,
    Good,
    Bad,
    Strong,
}

pub type Line = Vec<(Tone, String)>;

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Stay,
    Done,
    Back,
}

pub trait Widget {
    fn key(&mut self, key: Key, page: usize) -> Step;
    fn lines(&mut self, height: usize) -> Vec<Line>;
    /// Row and column of the text cursor, for text fields.
    fn cursor(&self) -> Option<(usize, usize)> {
        None
    }
    /// The one line left on screen once answered.
    fn summary(&self) -> Line;
}

fn question(title: &str) -> Vec<(Tone, String)> {
    vec![(Tone::Accent, "? ".into()), (Tone::Strong, title.into())]
}

pub fn answered(title: &str, answer: &str) -> Line {
    vec![
        (Tone::Good, "✔ ".into()),
        (Tone::Plain, title.into()),
        (Tone::Dim, " · ".into()),
        (Tone::Accent, answer.into()),
    ]
}

#[cfg(test)]
fn text(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.iter().map(|(_, s)| s.as_str()).collect())
        .collect()
}
