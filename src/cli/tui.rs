//! The terminal side of the interactive setup: raw keys in, widgets drawn in
//! place under the previous answers (no full-screen takeover, so the history
//! of a session stays readable in the scrollback).

mod widget;

use std::io::{self, IsTerminal, Stdout, Write};
use std::path::PathBuf;

use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{self, Clear, ClearType};
use crossterm::{execute, queue};
use sync_optimized_rules::error::{Error, Result};

pub use widget::{Confirm, Input, Item, Key, Line, List, Step, Tone, Widget};

/// Stands for the terminal in I/O errors, which end the session.
pub const TERMINAL: &str = "<terminal>";

pub fn available() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

pub fn is_terminal_error(e: &Error) -> bool {
    matches!(e, Error::Io(p, _) if p.as_os_str() == TERMINAL)
}

fn term(e: io::Error) -> Error {
    Error::Io(PathBuf::from(TERMINAL), e)
}

pub struct Ui {
    color: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none(),
        }
    }

    pub fn color(&self) -> bool {
        self.color
    }

    /// Run a widget until it is answered (`true`) or left with esc (`false`).
    pub fn ask(&self, w: &mut impl Widget) -> Result<bool> {
        let mut out = io::stdout();
        terminal::enable_raw_mode().map_err(term)?;
        let result = self.drive(w, &mut out);
        let _ = execute!(out, Show);
        let _ = terminal::disable_raw_mode();
        result.map_err(term)
    }

    fn drive(&self, w: &mut impl Widget, out: &mut Stdout) -> io::Result<bool> {
        let mut row = 0;
        loop {
            let (width, height) = terminal::size().unwrap_or((80, 24));
            let lines = w.lines(usize::from(height).saturating_sub(1));
            row = self.draw(out, row, &lines, w.cursor(), usize::from(width))?;
            let key = match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => map(k),
                _ => None,
            };
            let Some(key) = key else { continue };
            if key == Key::Quit {
                self.draw(out, row, &[], None, usize::from(width))?;
                execute!(out, Show)?;
                terminal::disable_raw_mode()?;
                std::process::exit(130);
            }
            let page = usize::from(height).saturating_sub(6).max(1);
            match w.key(key, page) {
                Step::Stay => {}
                Step::Done => {
                    self.draw(out, row, &[w.summary()], None, usize::from(width))?;
                    queue!(out, Print("\r\n"))?;
                    out.flush()?;
                    return Ok(true);
                }
                Step::Back => {
                    self.draw(out, row, &[], None, usize::from(width))?;
                    return Ok(false);
                }
            }
        }
    }

    /// Replace what the previous call drew, starting `row` lines above the
    /// cursor. Returns the row the cursor is left on.
    fn draw(
        &self,
        out: &mut Stdout,
        row: usize,
        lines: &[Line],
        cursor: Option<(usize, usize)>,
        width: usize,
    ) -> io::Result<usize> {
        queue!(out, MoveToColumn(0))?;
        if row > 0 {
            queue!(out, MoveUp(u16::try_from(row).unwrap_or(u16::MAX)))?;
        }
        queue!(out, Clear(ClearType::FromCursorDown))?;
        for (n, line) in lines.iter().enumerate() {
            if n > 0 {
                queue!(out, Print("\r\n"))?;
            }
            self.print(out, line, width.saturating_sub(1))?;
        }
        let last = lines.len().saturating_sub(1);
        let row = match cursor {
            Some((r, col)) => {
                let up = last.saturating_sub(r);
                if up > 0 {
                    queue!(out, MoveUp(u16::try_from(up).unwrap_or(u16::MAX)))?;
                }
                let col = u16::try_from(col.min(width.saturating_sub(1))).unwrap_or(0);
                queue!(out, MoveToColumn(col), Show)?;
                r
            }
            None => {
                queue!(out, Hide)?;
                last
            }
        };
        out.flush()?;
        Ok(row)
    }

    /// One line, cut at `width` columns so it never wraps.
    fn print(&self, out: &mut impl Write, line: &Line, width: usize) -> io::Result<()> {
        let mut left = width;
        for (tone, text) in line {
            if left == 0 {
                break;
            }
            let part: String = text.chars().take(left).collect();
            left -= part.chars().count();
            self.paint(out, *tone, &part)?;
        }
        Ok(())
    }

    fn paint(&self, out: &mut impl Write, tone: Tone, text: &str) -> io::Result<()> {
        if !self.color || tone == Tone::Plain {
            return queue!(out, Print(text));
        }
        match tone {
            Tone::Strong => queue!(
                out,
                SetAttribute(Attribute::Bold),
                Print(text),
                SetAttribute(Attribute::Reset)
            ),
            _ => {
                let color = match tone {
                    Tone::Dim => Color::DarkGrey,
                    Tone::Good => Color::Green,
                    Tone::Bad => Color::Red,
                    _ => Color::Cyan,
                };
                queue!(out, SetForegroundColor(color), Print(text), ResetColor)
            }
        }
    }

    /// Print a line outside any widget.
    pub fn say(&self, tone: Tone, text: &str) {
        let mut out = io::stdout();
        let _ = self.paint(&mut out, tone, text);
        let _ = queue!(out, Print("\n"));
        let _ = out.flush();
    }

    pub fn pick(&self, list: List) -> Result<Option<usize>> {
        let mut list = list;
        Ok(if self.ask(&mut list)? {
            list.current()
        } else {
            None
        })
    }

    pub fn tick(&self, list: List) -> Result<Option<Vec<usize>>> {
        let mut list = list;
        Ok(self.ask(&mut list)?.then(|| list.ticked()))
    }

    pub fn text(
        &self,
        title: &str,
        default: &str,
        check: impl Fn(&str) -> std::result::Result<(), String>,
    ) -> Result<Option<String>> {
        let mut input = Input::new(title, default, check);
        Ok(self.ask(&mut input)?.then(|| input.text()))
    }

    pub fn confirm(&self, title: &str, default: bool) -> Result<Option<bool>> {
        let mut c = Confirm::new(title, default);
        Ok(self.ask(&mut c)?.then_some(c.value))
    }
}

fn map(k: KeyEvent) -> Option<Key> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    Some(match k.code {
        KeyCode::Char('c') if ctrl => Key::Quit,
        KeyCode::Char('a') if ctrl => Key::All,
        KeyCode::Char('u') if ctrl => Key::Clear,
        KeyCode::Char(_) if ctrl => return None,
        KeyCode::Char(' ') => Key::Space,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Tab => Key::Down,
        KeyCode::BackTab => Key::Up,
        _ => return None,
    })
}
