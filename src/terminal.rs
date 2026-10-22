use alacritty_terminal::{
    event::VoidListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{Config, Term},
    vte::ansi::Processor,
};

struct TerminalSize {
    columns: usize,
    rows: usize,
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

pub struct TerminalEmulator {
    term: Term<VoidListener>,
    processor: Processor,
}

impl TerminalEmulator {
    pub fn new() -> Self {
        let size = TerminalSize {
            columns: 80,
            rows: 24,
        };
        Self {
            term: Term::new(Config::default(), &size, VoidListener),
            processor: Processor::new(),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.processor.advance(&mut self.term, bytes);
    }

    pub fn snapshot(&self) -> String {
        (0..self.term.screen_lines())
            .map(|row| {
                let row = &self.term.grid()[Line(row as i32)];
                let text = (0..self.term.columns())
                    .map(|column| row[Column(column)].c)
                    .collect::<String>();
                text.trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalEmulator;

    #[test]
    fn ansi_cursor_and_color_sequences_update_terminal_state() {
        let mut terminal = TerminalEmulator::new();
        terminal.feed(b"discarded\x1b[1;1Hready\x1b[31m!");

        let screen = terminal.snapshot();
        assert!(screen.starts_with("ready!"));
        assert!(!screen.contains("discarded"));
    }
}
