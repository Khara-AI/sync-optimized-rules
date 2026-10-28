//! Lists to pick one item from, or to tick several.

use super::{Key, Line, Step, Tone, Widget, answered, question};

pub struct Item {
    pub label: String,
    pub hint: String,
}

impl Item {
    pub fn new(label: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            hint: hint.into(),
        }
    }
}

/// A list to pick one item from, or to tick several.
pub struct List {
    title: String,
    notes: Vec<String>,
    items: Vec<Item>,
    multi: bool,
    /// A tick list that cannot be validated empty.
    required: bool,
    pub checked: Vec<bool>,
    filter: String,
    cursor: usize,
    offset: usize,
    warning: Option<&'static str>,
}

impl List {
    pub fn single(title: &str, items: Vec<Item>) -> Self {
        Self::new(title, items, false, Vec::new())
    }

    pub fn multi(title: &str, items: Vec<Item>, checked: Vec<bool>) -> Self {
        Self::new(title, items, true, checked)
    }

    fn new(title: &str, items: Vec<Item>, multi: bool, mut checked: Vec<bool>) -> Self {
        checked.resize(items.len(), false);
        Self {
            title: title.into(),
            notes: Vec::new(),
            items,
            multi,
            required: false,
            checked,
            filter: String::new(),
            cursor: 0,
            offset: 0,
            warning: None,
        }
    }

    /// Dim lines shown between the title and the items.
    pub fn notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    fn visible(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        (0..self.items.len())
            .filter(|&i| f.is_empty() || self.items[i].label.to_lowercase().contains(&f))
            .collect()
    }

    pub fn current(&self) -> Option<usize> {
        self.visible().get(self.cursor).copied()
    }

    pub fn ticked(&self) -> Vec<usize> {
        (0..self.items.len()).filter(|&i| self.checked[i]).collect()
    }

    fn refilter(&mut self) {
        self.cursor = 0;
        self.offset = 0;
    }
}

impl Widget for List {
    fn key(&mut self, key: Key, page: usize) -> Step {
        let n = self.visible().len();
        let last = n.saturating_sub(1);
        self.warning = None;
        match key {
            Key::Up => {
                self.cursor = if self.cursor == 0 {
                    last
                } else {
                    self.cursor - 1
                }
            }
            Key::Down | Key::Tab => {
                self.cursor = if self.cursor >= last {
                    0
                } else {
                    self.cursor + 1
                }
            }
            Key::PageUp => self.cursor = self.cursor.saturating_sub(page),
            Key::PageDown => self.cursor = (self.cursor + page).min(last),
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = last,
            Key::Space if self.multi => {
                if let Some(i) = self.current() {
                    self.checked[i] = !self.checked[i];
                }
            }
            Key::All if self.multi => {
                let vis = self.visible();
                let all = vis.iter().all(|&i| self.checked[i]);
                vis.into_iter().for_each(|i| self.checked[i] = !all);
            }
            Key::Enter if self.multi => {
                if self.required && !self.checked.contains(&true) {
                    self.warning = Some("tick at least one with space");
                } else {
                    return Step::Done;
                }
            }
            Key::Enter if self.current().is_some() => return Step::Done,
            Key::Esc if self.filter.is_empty() => return Step::Back,
            Key::Esc => {
                self.filter.clear();
                self.refilter();
            }
            Key::Backspace => {
                self.filter.pop();
                self.refilter();
            }
            Key::Char(c) => {
                self.filter.push(c);
                self.refilter();
            }
            _ => {}
        }
        Step::Stay
    }

    fn lines(&mut self, height: usize) -> Vec<Line> {
        let mut head = question(&self.title);
        if !self.filter.is_empty() {
            head.push((Tone::Dim, "  filter: ".into()));
            head.push((Tone::Accent, self.filter.clone()));
        }
        let mut out = vec![head];
        out.extend(
            self.notes
                .iter()
                .map(|n| vec![(Tone::Dim, format!("  {n}"))]),
        );
        let vis = self.visible();
        let rows = height.saturating_sub(out.len() + 2).max(3);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + rows {
            self.offset = self.cursor + 1 - rows;
        }
        if vis.is_empty() {
            out.push(vec![(Tone::Dim, "  nothing matches".into())]);
        }
        for (pos, &i) in vis.iter().enumerate().skip(self.offset).take(rows) {
            let here = pos == self.cursor;
            let mut line = vec![(Tone::Accent, if here { "› " } else { "  " }.into())];
            if self.multi {
                line.push(if self.checked[i] {
                    (Tone::Good, "◉ ".into())
                } else {
                    (Tone::Dim, "○ ".into())
                });
            }
            let tone = if here { Tone::Strong } else { Tone::Plain };
            line.push((tone, self.items[i].label.clone()));
            if !self.items[i].hint.is_empty() {
                line.push((Tone::Dim, format!("  {}", self.items[i].hint)));
            }
            out.push(line);
        }
        let mut help = String::from("  ↑↓ move");
        if self.multi {
            help.push_str(" · space tick · ctrl+a all · enter confirm");
        } else {
            help.push_str(" · enter choose");
        }
        help.push_str(" · esc back · type to filter");
        if vis.len() > rows {
            help.push_str(&format!(" · {}/{}", self.cursor + 1, vis.len()));
        }
        let mut foot = vec![(Tone::Dim, help)];
        if let Some(w) = self.warning {
            foot.push((Tone::Bad, format!("  {w}")));
        }
        out.push(foot);
        out
    }

    fn summary(&self) -> Line {
        let answer = if self.multi {
            let names: Vec<&str> = self
                .ticked()
                .into_iter()
                .map(|i| self.items[i].label.as_str())
                .collect();
            match names.len() {
                0 => "none".to_owned(),
                1..=3 => names.join(", "),
                n => format!("{n} items"),
            }
        } else {
            self.current()
                .map(|i| self.items[i].label.clone())
                .unwrap_or_default()
        };
        answered(&self.title, &answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::tui::widget::text;

    fn items(names: &[&str]) -> Vec<Item> {
        names.iter().map(|n| Item::new(*n, "")).collect()
    }

    #[test]
    fn arrows_wrap_and_typing_filters() {
        let mut l = List::single("pick", items(&["alpha", "beta", "gamma"]));
        l.key(Key::Up, 10);
        assert_eq!(l.current(), Some(2));
        l.key(Key::Down, 10);
        assert_eq!(l.current(), Some(0));
        for c in "MM".chars() {
            l.key(Key::Char(c), 10);
        }
        assert_eq!(l.current(), Some(2), "filtering is case-insensitive");
        assert_eq!(
            l.key(Key::Esc, 10),
            Step::Stay,
            "esc clears the filter first"
        );
        assert_eq!(l.key(Key::Esc, 10), Step::Back);
    }

    #[test]
    fn ticking_and_required_lists() {
        let mut l = List::multi("tick", items(&["a", "b", "c"]), vec![false, true]).required();
        l.key(Key::All, 10);
        assert_eq!(l.ticked(), [0, 1, 2]);
        l.key(Key::All, 10);
        assert_eq!(l.key(Key::Enter, 10), Step::Stay, "nothing ticked");
        assert!(
            text(&l.lines(20))
                .last()
                .unwrap()
                .contains("tick at least one")
        );
        l.key(Key::Down, 10);
        l.key(Key::Space, 10);
        assert_eq!(l.key(Key::Enter, 10), Step::Done);
        assert_eq!(l.ticked(), [1]);
    }

    #[test]
    fn long_lists_scroll_with_the_cursor() {
        let names: Vec<String> = (0..50).map(|i| format!("item{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut l = List::single("pick", items(&refs));
        l.key(Key::End, 10);
        let shown = text(&l.lines(10));
        assert!(shown.len() <= 10);
        assert!(shown.iter().any(|s| s.contains("› item49")));
        assert!(shown.last().unwrap().contains("50/50"));
    }
}
