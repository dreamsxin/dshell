use std::path::PathBuf;

use alacritty_terminal::{
    event::VoidListener,
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    term::{Config, Term, TermMode, cell::Flags},
    vte::ansi::{Color, NamedColor, Processor},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalTheme {
    #[default]
    Default,
    SolarizedDark,
    SolarizedLight,
}

impl TerminalTheme {
    pub const ALL: [Self; 3] = [Self::Default, Self::SolarizedDark, Self::SolarizedLight];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "Default dark",
            Self::SolarizedDark => "Solarized dark",
            Self::SolarizedLight => "Solarized light",
        }
    }

    pub fn palette(self) -> TerminalPalette {
        match self {
            Self::Default => TerminalPalette {
                foreground: 0xe1e5e9,
                dim_foreground: 0x9da4aa,
                background: 0x0b0f13,
                selection_background: 0x315b80,
                selection_foreground: 0xf4f6f8,
                ansi: [
                    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
                    0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
                ],
            },
            Self::SolarizedDark => TerminalPalette {
                foreground: 0x839496,
                dim_foreground: 0x657b83,
                background: 0x002b36,
                selection_background: 0x0f4b6e,
                selection_foreground: 0xfdf6e3,
                ansi: [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            },
            Self::SolarizedLight => TerminalPalette {
                foreground: 0x657b83,
                dim_foreground: 0x93a1a1,
                background: 0xfdf6e3,
                selection_background: 0x93a1a1,
                selection_foreground: 0x002b36,
                ansi: [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    #[default]
    Block,
    Underline,
    Bar,
}

impl CursorStyle {
    pub const ALL: [Self; 3] = [Self::Block, Self::Underline, Self::Bar];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Block => "Block",
            Self::Underline => "Underline",
            Self::Bar => "Bar",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCursor {
    pub row: u16,
    pub column: u16,
    pub style: CursorStyle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchDirection {
    Forward,
    Backward,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerminalSearchResult {
    pub found: bool,
    pub total: usize,
    pub current: usize,
    pub wrapped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPalette {
    pub foreground: u32,
    pub dim_foreground: u32,
    pub background: u32,
    pub selection_background: u32,
    pub selection_foreground: u32,
    pub ansi: [u32; 16],
}

#[cfg(test)]
pub const CELL_WIDTH_PX: f32 = 8.0;
#[cfg(test)]
pub const CELL_HEIGHT_PX: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSize {
    pub columns: u16,
    pub rows: u16,
}

impl TerminalSize {
    #[cfg(test)]
    pub fn for_window(width: f32, height: f32) -> Self {
        Self::for_window_with(width, height, CELL_WIDTH_PX, CELL_HEIGHT_PX)
    }

    pub fn for_window_with(width: f32, height: f32, cell_width: f32, cell_height: f32) -> Self {
        let columns = ((width - 56.0) / cell_width).floor().clamp(1.0, 240.0);
        let rows = ((height - 152.0) / cell_height).floor().clamp(1.0, 120.0);

        Self {
            columns: columns as u16,
            rows: rows as u16,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalKeyInput {
    pub key: String,
    pub text: Option<String>,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
    pub function: bool,
}

pub enum TerminalRequest {
    Key(TerminalKeyInput),
    Resize(TerminalSize),
    Mouse(TerminalMouseEvent),
    Paste(String),
    StartLog(PathBuf),
    StopLog,
    SetTheme(TerminalTheme),
    SetCursorStyle(CursorStyle),
    Search {
        query: String,
        direction: SearchDirection,
    },
    RefreshServerMetrics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPosition {
    pub row: u16,
    pub column: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseTrackingMode {
    Disabled,
    Click,
    ButtonMotion,
    AnyMotion,
}

impl MouseTrackingMode {
    pub fn reports_motion(self, button_pressed: bool) -> bool {
        match self {
            Self::Disabled | Self::Click => false,
            Self::ButtonMotion => button_pressed,
            Self::AnyMotion => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalMouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMouseEvent {
    Press {
        button: TerminalMouseButton,
        position: TerminalPosition,
        modifiers: TerminalMouseModifiers,
    },
    Release {
        button: TerminalMouseButton,
        position: TerminalPosition,
        modifiers: TerminalMouseModifiers,
    },
    Move {
        button: Option<TerminalMouseButton>,
        position: TerminalPosition,
        modifiers: TerminalMouseModifiers,
    },
    Wheel {
        rows: i32,
        position: TerminalPosition,
        modifiers: TerminalMouseModifiers,
    },
}

impl TerminalKeyInput {
    pub fn encode(&self, application_cursor: bool) -> Option<Vec<u8>> {
        if self.platform || self.function {
            return None;
        }

        let key = self.key.to_ascii_lowercase();
        let modifier =
            1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.control);

        let sequence = match key.as_str() {
            "enter" | "return" => Some(b"\r".to_vec()),
            "backspace" => Some(vec![0x7f]),
            "tab" if self.shift => Some(b"\x1b[Z".to_vec()),
            "tab" => Some(b"\t".to_vec()),
            "escape" => Some(b"\x1b".to_vec()),
            "up" => Some(cursor_key(b'A', application_cursor, modifier)),
            "down" => Some(cursor_key(b'B', application_cursor, modifier)),
            "right" => Some(cursor_key(b'C', application_cursor, modifier)),
            "left" => Some(cursor_key(b'D', application_cursor, modifier)),
            "home" => Some(csi_key(b'H', modifier)),
            "end" => Some(csi_key(b'F', modifier)),
            "insert" => Some(csi_tilde_key(2, modifier)),
            "delete" => Some(csi_tilde_key(3, modifier)),
            "pageup" => Some(csi_tilde_key(5, modifier)),
            "pagedown" => Some(csi_tilde_key(6, modifier)),
            _ => function_key(&key, modifier),
        };

        if let Some(mut sequence) = sequence {
            if self.alt && matches!(key.as_str(), "enter" | "return" | "backspace" | "tab") {
                sequence.insert(0, 0x1b);
            }
            return Some(sequence);
        }

        if self.control {
            let byte = match key.as_str() {
                "space" | "@" => Some(0x00),
                "[" => Some(0x1b),
                "\\" => Some(0x1c),
                "]" => Some(0x1d),
                "^" => Some(0x1e),
                "_" => Some(0x1f),
                "?" => Some(0x7f),
                _ => key
                    .as_bytes()
                    .first()
                    .filter(|byte| byte.is_ascii_alphabetic())
                    .map(|byte| byte.to_ascii_lowercase() - b'a' + 1),
            }?;
            return Some(if self.alt {
                vec![0x1b, byte]
            } else {
                vec![byte]
            });
        }

        let text = self.text.as_ref().filter(|text| !text.is_empty())?;
        let mut bytes = Vec::with_capacity(text.len() + usize::from(self.alt));
        if self.alt {
            bytes.push(0x1b);
        }
        bytes.extend_from_slice(text.as_bytes());
        Some(bytes)
    }
}

fn cursor_key(final_byte: u8, application_cursor: bool, modifier: u8) -> Vec<u8> {
    if modifier == 1 && application_cursor {
        vec![0x1b, b'O', final_byte]
    } else if modifier == 1 {
        vec![0x1b, b'[', final_byte]
    } else {
        format!("\x1b[1;{modifier}{}", final_byte as char).into_bytes()
    }
}

fn csi_key(final_byte: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        vec![0x1b, b'[', final_byte]
    } else {
        format!("\x1b[1;{modifier}{}", final_byte as char).into_bytes()
    }
}

fn csi_tilde_key(code: u8, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        format!("\x1b[{code}~").into_bytes()
    } else {
        format!("\x1b[{code};{modifier}~").into_bytes()
    }
}

fn function_key(key: &str, modifier: u8) -> Option<Vec<u8>> {
    let number = key.strip_prefix('f')?.parse::<u8>().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }

    if number <= 4 {
        let final_byte = b'P' + number - 1;
        Some(if modifier == 1 {
            vec![0x1b, b'O', final_byte]
        } else {
            format!("\x1b[1;{modifier}{}", final_byte as char).into_bytes()
        })
    } else {
        let code = match number {
            5 => 15,
            6 => 17,
            7 => 18,
            8 => 19,
            9 => 20,
            10 => 21,
            11 => 23,
            12 => 24,
            _ => unreachable!(),
        };
        Some(csi_tilde_key(code, modifier))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRun {
    pub text: String,
    pub width_cells: usize,
    pub foreground: u32,
    pub background: u32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalFrame {
    pub rows: Vec<Vec<TerminalRun>>,
    pub selection_text: Option<String>,
    pub mouse_tracking_mode: MouseTrackingMode,
    pub foreground: u32,
    pub background: u32,
    pub cursor: Option<TerminalCursor>,
}

#[derive(Clone, Copy)]
struct TerminalDimensions {
    columns: usize,
    rows: usize,
}

impl From<TerminalSize> for TerminalDimensions {
    fn from(size: TerminalSize) -> Self {
        Self {
            columns: size.columns as usize,
            rows: size.rows as usize,
        }
    }
}

impl Dimensions for TerminalDimensions {
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
    reported_mouse_encodings: [Option<MouseEncoding>; 3],
    theme: TerminalTheme,
    cursor_style: CursorStyle,
    search: Option<SearchState>,
}

struct SearchState {
    query: String,
    matches: Vec<(Point, Point)>,
    current: usize,
}

#[derive(Clone, Copy)]
enum MouseEncoding {
    Legacy,
    Utf8,
    Sgr,
}

impl TerminalEmulator {
    #[cfg(test)]
    pub fn new(size: TerminalSize) -> Self {
        Self::new_with_theme(size, TerminalTheme::Default)
    }

    pub fn new_with_theme(size: TerminalSize, theme: TerminalTheme) -> Self {
        Self {
            term: Term::new(
                Config::default(),
                &TerminalDimensions::from(size),
                VoidListener,
            ),
            processor: Processor::new(),
            reported_mouse_encodings: [None; 3],
            theme,
            cursor_style: CursorStyle::Block,
            search: None,
        }
    }

    pub fn set_theme(&mut self, theme: TerminalTheme) {
        self.theme = theme;
    }

    pub fn set_cursor_style(&mut self, cursor_style: CursorStyle) {
        self.cursor_style = cursor_style;
    }

    pub fn search(&mut self, query: &str, direction: SearchDirection) -> TerminalSearchResult {
        let query = query.trim();
        if query.is_empty() {
            self.search = None;
            return TerminalSearchResult {
                found: false,
                total: 0,
                current: 0,
                wrapped: false,
            };
        }
        let reset = self
            .search
            .as_ref()
            .is_none_or(|state| !state.query.eq_ignore_ascii_case(query));
        let (matches, index, wrapped) = if reset {
            let matches = self.collect_matches(query);
            let index = match direction {
                SearchDirection::Forward => 0,
                SearchDirection::Backward => matches.len().saturating_sub(1),
            };
            (matches, index, false)
        } else {
            let state = self.search.take().expect("search state exists");
            let len = state.matches.len();
            if len == 0 {
                (Vec::new(), 0, false)
            } else {
                let (index, wrapped) = match direction {
                    SearchDirection::Forward => {
                        let wrapped = state.current + 1 >= len;
                        ((state.current + 1) % len, wrapped)
                    }
                    SearchDirection::Backward => {
                        let wrapped = state.current == 0;
                        (if wrapped { len - 1 } else { state.current - 1 }, wrapped)
                    }
                };
                (state.matches, index, wrapped)
            }
        };
        let total = matches.len();
        if total == 0 {
            self.search = None;
            return TerminalSearchResult {
                found: false,
                total: 0,
                current: 0,
                wrapped: false,
            };
        }
        let (start, end) = matches[index];
        self.term.scroll_to_point(start);
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        self.term.selection = Some(selection);
        self.search = Some(SearchState {
            query: query.to_ascii_lowercase(),
            matches,
            current: index,
        });
        TerminalSearchResult {
            found: true,
            total,
            current: index,
            wrapped,
        }
    }

    fn collect_matches(&self, query: &str) -> Vec<(Point, Point)> {
        let query = query.to_ascii_lowercase();
        let history = self.term.grid().history_size() as i32;
        let screen = self.term.screen_lines() as i32;
        let mut matches = Vec::new();
        for line in -history..screen {
            let row = &self.term.grid()[Line(line)];
            let mut text = String::new();
            let mut cells: Vec<(usize, usize, Column, usize)> = Vec::new();
            for column in 0..self.term.columns() {
                let cell = &row[Column(column)];
                let flags = cell.flags;
                if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let start = text.len();
                text.push(if flags.contains(Flags::HIDDEN) {
                    ' '
                } else {
                    cell.c
                });
                if let Some(extra) = cell.zerowidth() {
                    text.extend(extra);
                }
                let width = if flags.contains(Flags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                cells.push((start, text.len(), Column(column), width));
            }
            let haystack = text.to_ascii_lowercase();
            let mut offset = 0;
            while offset < haystack.len() {
                let Some(found) = haystack[offset..].find(&query) else {
                    break;
                };
                let start_byte = offset + found;
                let end_byte = start_byte + query.len();
                let first = cells.iter().find(|(start, _, _, _)| *start == start_byte);
                let last = cells.iter().find(|(_, end, _, _)| *end == end_byte);
                if let (Some((_, _, start_column, _)), Some((_, _, end_column, width))) =
                    (first, last)
                {
                    matches.push((
                        Point::new(Line(line), *start_column),
                        Point::new(Line(line), Column(end_column.0 + width - 1)),
                    ));
                }
                offset = end_byte.max(start_byte + 1);
            }
        }
        matches
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.search = None;
        }
        self.processor.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.search = None;
        self.term.resize(TerminalDimensions::from(size));
    }

    pub fn scroll_display(&mut self, rows: i32) {
        self.term.scroll_display(Scroll::Delta(rows));
    }

    pub fn mouse_tracking_mode(&self) -> MouseTrackingMode {
        let mode = self.term.mode();
        if mode.contains(TermMode::MOUSE_MOTION) {
            MouseTrackingMode::AnyMotion
        } else if mode.contains(TermMode::MOUSE_DRAG) {
            MouseTrackingMode::ButtonMotion
        } else if mode.contains(TermMode::MOUSE_REPORT_CLICK) {
            MouseTrackingMode::Click
        } else {
            MouseTrackingMode::Disabled
        }
    }

    fn mouse_encoding(&self) -> MouseEncoding {
        let mode = self.term.mode();
        if mode.contains(TermMode::SGR_MOUSE) {
            MouseEncoding::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Legacy
        }
    }

    pub fn mouse_report_bytes(&mut self, event: TerminalMouseEvent) -> Option<Vec<u8>> {
        let tracking = self.mouse_tracking_mode();
        let encoding = self.mouse_encoding();
        match event {
            TerminalMouseEvent::Press {
                button,
                position,
                modifiers,
            } => {
                if tracking == MouseTrackingMode::Disabled {
                    return None;
                }
                let bytes = encode_mouse_report(
                    encoding,
                    mouse_button_code(button) + mouse_modifier_bits(modifiers),
                    position,
                    false,
                )?;
                self.reported_mouse_encodings[mouse_button_index(button)] = Some(encoding);
                Some(bytes)
            }
            TerminalMouseEvent::Release {
                button,
                position,
                modifiers,
            } => {
                let encoding = self.reported_mouse_encodings[mouse_button_index(button)]
                    .take()
                    .or_else(|| (tracking != MouseTrackingMode::Disabled).then_some(encoding))?;
                let button_code = if matches!(encoding, MouseEncoding::Sgr) {
                    mouse_button_code(button)
                } else {
                    3
                };
                encode_mouse_report(
                    encoding,
                    button_code + mouse_modifier_bits(modifiers),
                    position,
                    true,
                )
            }
            TerminalMouseEvent::Move {
                button,
                position,
                modifiers,
            } => {
                if !tracking.reports_motion(button.is_some()) {
                    return None;
                }
                let code =
                    button.map_or(3, mouse_button_code) + 32 + mouse_modifier_bits(modifiers);
                encode_mouse_report(encoding, code, position, false)
            }
            TerminalMouseEvent::Wheel {
                rows,
                position,
                modifiers,
            } => {
                if tracking == MouseTrackingMode::Disabled || rows == 0 {
                    return None;
                }
                let code = (if rows > 0 { 64 } else { 65 }) + mouse_modifier_bits(modifiers);
                let reports = rows.unsigned_abs().min(64);
                let mut bytes = Vec::with_capacity(reports as usize * 16);
                for _ in 0..reports {
                    bytes.extend(encode_mouse_report(encoding, code, position, false)?);
                }
                Some(bytes)
            }
        }
    }

    fn selection_point(&self, position: TerminalPosition) -> Point {
        let row = usize::from(position.row).min(self.term.screen_lines().saturating_sub(1));
        let column = usize::from(position.column).min(self.term.columns().saturating_sub(1));
        Point::new(
            Line(row as i32 - self.term.grid().display_offset() as i32),
            Column(column),
        )
    }

    pub fn start_selection(&mut self, position: TerminalPosition) {
        let point = self.selection_point(position);
        self.term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
    }

    pub fn update_selection(&mut self, position: TerminalPosition) {
        let point = self.selection_point(position);
        if let Some(selection) = self.term.selection.as_mut() {
            selection.update(point, Side::Right);
        }
    }

    pub fn paste_bytes(&self, text: &str) -> Vec<u8> {
        let bracketed = self.term.mode().contains(TermMode::BRACKETED_PASTE);
        let mut bytes = Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        bytes
    }

    pub fn application_cursor_mode(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn frame(&self) -> TerminalFrame {
        let cursor = self.term.grid().cursor.point;
        let show_cursor = self.term.mode().contains(TermMode::SHOW_CURSOR);
        let selection = self
            .term
            .selection
            .as_ref()
            .and_then(|selection| selection.to_range(&self.term));
        let selection_text = self.term.selection_to_string();
        let palette = self.theme.palette();
        let first_line = -(self.term.grid().display_offset() as i32);
        let block_cursor = self.cursor_style == CursorStyle::Block;
        let cursor_row = cursor.line.0 - first_line;
        let cursor_info = (show_cursor
            && cursor_row >= 0
            && cursor_row < self.term.screen_lines() as i32
            && cursor.column.0 < self.term.columns())
        .then_some(TerminalCursor {
            row: cursor_row as u16,
            column: cursor.column.0 as u16,
            style: self.cursor_style,
        });
        let rows = (first_line..first_line + self.term.screen_lines() as i32)
            .map(|line| {
                let row = &self.term.grid()[Line(line)];
                let mut runs: Vec<TerminalRun> = Vec::new();

                for column in 0..self.term.columns() {
                    let cell = &row[Column(column)];
                    let flags = cell.flags;
                    if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                        continue;
                    }

                    let is_cursor =
                        show_cursor && cursor.line.0 == line && cursor.column.0 == column;
                    let selected = selection.as_ref().is_some_and(|selection| {
                        selection.contains(Point::new(Line(line), Column(column)))
                    });
                    let mut foreground = color_to_rgb(cell.fg, &palette);
                    let mut background = color_to_rgb(cell.bg, &palette);
                    if flags.contains(Flags::INVERSE) ^ (is_cursor && block_cursor) {
                        std::mem::swap(&mut foreground, &mut background);
                    }
                    let style = (
                        foreground,
                        background,
                        flags.contains(Flags::BOLD),
                        flags.contains(Flags::ITALIC),
                        flags.intersects(Flags::ALL_UNDERLINES),
                        selected,
                    );
                    let width_cells = if flags.contains(Flags::WIDE_CHAR) {
                        2
                    } else {
                        1
                    };
                    let character = if flags.contains(Flags::HIDDEN) {
                        ' '
                    } else {
                        cell.c
                    };

                    if let Some(run) = runs.last_mut().filter(|run| {
                        (
                            run.foreground,
                            run.background,
                            run.bold,
                            run.italic,
                            run.underline,
                            run.selected,
                        ) == style
                    }) {
                        run.text.push(character);
                        if let Some(extra) = cell.zerowidth() {
                            run.text.extend(extra);
                        }
                        run.width_cells += width_cells;
                    } else {
                        let mut text = String::new();
                        text.push(character);
                        if let Some(extra) = cell.zerowidth() {
                            text.extend(extra);
                        }
                        runs.push(TerminalRun {
                            text,
                            width_cells,
                            foreground,
                            background,
                            bold: style.2,
                            italic: style.3,
                            underline: style.4,
                            selected: style.5,
                        });
                    }
                }
                runs
            })
            .collect();

        TerminalFrame {
            rows,
            selection_text,
            mouse_tracking_mode: self.mouse_tracking_mode(),
            foreground: palette.foreground,
            background: palette.background,
            cursor: cursor_info,
        }
    }
}

fn mouse_button_index(button: TerminalMouseButton) -> usize {
    match button {
        TerminalMouseButton::Left => 0,
        TerminalMouseButton::Middle => 1,
        TerminalMouseButton::Right => 2,
    }
}

fn mouse_button_code(button: TerminalMouseButton) -> u16 {
    mouse_button_index(button) as u16
}

fn mouse_modifier_bits(modifiers: TerminalMouseModifiers) -> u16 {
    (if modifiers.shift { 4 } else { 0 })
        + (if modifiers.alt { 8 } else { 0 })
        + (if modifiers.control { 16 } else { 0 })
}

fn encode_mouse_report(
    encoding: MouseEncoding,
    code: u16,
    position: TerminalPosition,
    release: bool,
) -> Option<Vec<u8>> {
    let column = u32::from(position.column) + 1;
    let row = u32::from(position.row) + 1;
    match encoding {
        MouseEncoding::Sgr => Some(
            format!(
                "\x1b[<{code};{column};{row}{}",
                if release { 'm' } else { 'M' }
            )
            .into_bytes(),
        ),
        MouseEncoding::Legacy | MouseEncoding::Utf8 => {
            let mut bytes = vec![0x1b, b'[', b'M', u8::try_from(code + 32).ok()?];
            encode_mouse_coordinate(column, encoding, &mut bytes)?;
            encode_mouse_coordinate(row, encoding, &mut bytes)?;
            Some(bytes)
        }
    }
}

fn encode_mouse_coordinate(
    coordinate: u32,
    encoding: MouseEncoding,
    bytes: &mut Vec<u8>,
) -> Option<()> {
    let value = coordinate + 32;
    match encoding {
        MouseEncoding::Legacy => bytes.push(u8::try_from(value).ok()?),
        MouseEncoding::Utf8 => {
            let character = char::from_u32(value)?;
            let mut buffer = [0; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
        }
        MouseEncoding::Sgr => return None,
    }
    Some(())
}

fn color_to_rgb(color: Color, palette: &TerminalPalette) -> u32 {
    match color {
        Color::Spec(rgb) => ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32,
        Color::Indexed(index) => indexed_color(index, palette),
        Color::Named(named) => named_color(named, palette),
    }
}

fn indexed_color(index: u8, palette: &TerminalPalette) -> u32 {
    match index {
        0..=15 => palette.ansi[index as usize],
        16..=231 => {
            let cube = index - 16;
            const LEVELS: [u32; 6] = [0, 95, 135, 175, 215, 255];
            (LEVELS[(cube / 36) as usize] << 16)
                | (LEVELS[((cube / 6) % 6) as usize] << 8)
                | LEVELS[(cube % 6) as usize]
        }
        _ => {
            let gray = 8 + 10 * (index as u32 - 232);
            (gray << 16) | (gray << 8) | gray
        }
    }
}

fn named_color(color: NamedColor, palette: &TerminalPalette) -> u32 {
    match color {
        NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
            palette.foreground
        }
        NamedColor::DimForeground => palette.dim_foreground,
        NamedColor::Background => palette.background,
        NamedColor::Black | NamedColor::DimBlack => palette.ansi[0],
        NamedColor::Red | NamedColor::DimRed => palette.ansi[1],
        NamedColor::Green | NamedColor::DimGreen => palette.ansi[2],
        NamedColor::Yellow | NamedColor::DimYellow => palette.ansi[3],
        NamedColor::Blue | NamedColor::DimBlue => palette.ansi[4],
        NamedColor::Magenta | NamedColor::DimMagenta => palette.ansi[5],
        NamedColor::Cyan | NamedColor::DimCyan => palette.ansi[6],
        NamedColor::White | NamedColor::DimWhite => palette.ansi[7],
        NamedColor::BrightBlack => palette.ansi[8],
        NamedColor::BrightRed => palette.ansi[9],
        NamedColor::BrightGreen => palette.ansi[10],
        NamedColor::BrightYellow => palette.ansi[11],
        NamedColor::BrightBlue => palette.ansi[12],
        NamedColor::BrightMagenta => palette.ansi[13],
        NamedColor::BrightCyan => palette.ansi[14],
        NamedColor::BrightWhite => palette.ansi[15],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CELL_HEIGHT_PX, CELL_WIDTH_PX, CursorStyle, SearchDirection, TerminalEmulator,
        TerminalKeyInput, TerminalPosition, TerminalSize, TerminalTheme,
    };

    fn key(key: &str, text: Option<&str>) -> TerminalKeyInput {
        TerminalKeyInput {
            key: key.to_owned(),
            text: text.map(str::to_owned),
            control: false,
            alt: false,
            shift: false,
            platform: false,
            function: false,
        }
    }

    #[test]
    fn ansi_output_preserves_colors_and_cursor_navigation_mode() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 80,
            rows: 24,
        });
        terminal.feed(b"discarded\x1b[1;1H\x1b[31mready\x1b[?1h");

        let frame = terminal.frame();
        let first_row = &frame.rows[0];
        assert_eq!(first_row[0].text, "ready");
        assert_eq!(first_row[0].foreground, 0xcd0000);
        assert!(terminal.application_cursor_mode());
    }

    #[test]
    fn cursor_style_is_reported_in_the_frame() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 8,
            rows: 2,
        });
        terminal.feed(b"hi");

        let cursor = terminal.frame().cursor.expect("cursor is visible");
        assert_eq!(cursor.style, CursorStyle::Block);
        assert_eq!(cursor.row, 0);
        assert_eq!(cursor.column, 2);

        terminal.set_cursor_style(CursorStyle::Bar);
        let cursor = terminal.frame().cursor.expect("cursor is visible");
        assert_eq!(cursor.style, CursorStyle::Bar);

        terminal.set_cursor_style(CursorStyle::Underline);
        let cursor = terminal.frame().cursor.expect("cursor is visible");
        assert_eq!(cursor.style, CursorStyle::Underline);
    }

    #[test]
    fn search_finds_history_matches_and_wraps() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 12,
            rows: 2,
        });
        terminal.feed(b"one\r\ntwo\r\none more\r\nthree");

        let first = terminal.search("ONE", SearchDirection::Forward);
        assert!(first.found);
        assert_eq!(first.total, 2);
        assert_eq!(first.current, 0);
        assert!(!first.wrapped);
        assert!(
            terminal
                .frame()
                .selection_text
                .as_deref()
                .is_some_and(|text| text.eq_ignore_ascii_case("one"))
        );

        let second = terminal.search("one", SearchDirection::Forward);
        assert_eq!(second.current, 1);
        assert!(!second.wrapped);

        let wrapped = terminal.search("one", SearchDirection::Forward);
        assert_eq!(wrapped.current, 0);
        assert!(wrapped.wrapped);

        let backward = terminal.search("one", SearchDirection::Backward);
        assert_eq!(backward.current, 1);
        assert!(backward.wrapped);

        assert!(!terminal.search("   ", SearchDirection::Forward).found);
        assert!(!terminal.search("missing", SearchDirection::Forward).found);
    }

    #[test]
    fn theme_changes_frame_background_and_ansi_palette() {
        let mut terminal = TerminalEmulator::new_with_theme(
            TerminalSize {
                columns: 8,
                rows: 2,
            },
            TerminalTheme::SolarizedLight,
        );
        terminal.feed(b"\x1b[31mred");

        let light = TerminalTheme::SolarizedLight.palette();
        let frame = terminal.frame();
        assert_eq!(frame.background, light.background);
        assert_eq!(frame.foreground, light.foreground);
        assert_eq!(frame.rows[0][0].background, light.background);
        assert_eq!(frame.rows[0][0].foreground, light.ansi[1]);

        terminal.set_theme(TerminalTheme::SolarizedDark);
        let dark = TerminalTheme::SolarizedDark.palette();
        let frame = terminal.frame();
        assert_eq!(frame.background, dark.background);
        assert_eq!(frame.rows[0][0].foreground, dark.ansi[1]);
    }

    #[test]
    fn scrollback_viewport_shows_history_and_returns_to_live_screen() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 16,
            rows: 2,
        });
        terminal.feed(b"one\r\ntwo\r\nthree\r\nfour");

        assert!(terminal.frame().rows[0][0].text.starts_with("three"));
        terminal.scroll_display(1);
        assert!(terminal.frame().rows[0][0].text.starts_with("two"));
        terminal.scroll_display(-1);
        assert!(terminal.frame().rows[0][0].text.starts_with("three"));
    }

    #[test]
    fn selection_extracts_and_marks_selected_text() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 16,
            rows: 2,
        });
        terminal.feed(b"hello terminal");
        terminal.start_selection(TerminalPosition { row: 0, column: 0 });
        terminal.update_selection(TerminalPosition { row: 0, column: 4 });

        let frame = terminal.frame();
        assert_eq!(frame.selection_text.as_deref(), Some("hello"));
        assert!(
            frame.rows[0]
                .iter()
                .any(|run| run.selected && run.text.contains("hello"))
        );
    }

    #[test]
    fn terminal_frame_preserves_wide_and_combining_unicode_cells() {
        for (text, expected_width) in [("中", 2), ("🙂", 2), ("e\u{301}", 1)] {
            let mut terminal = TerminalEmulator::new(TerminalSize {
                columns: 8,
                rows: 1,
            });
            terminal.feed(text.as_bytes());

            let frame = terminal.frame();
            let glyph = frame.rows[0]
                .iter()
                .find(|run| run.text.starts_with(text))
                .expect("Unicode glyph should be present in the terminal frame");
            assert_eq!(glyph.text, text);
            assert_eq!(glyph.width_cells, expected_width);
            assert_eq!(
                frame.rows[0]
                    .iter()
                    .map(|run| run.width_cells)
                    .sum::<usize>(),
                8
            );
        }
    }

    #[test]
    fn resizing_terminal_rebuilds_rows_and_columns() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 8,
            rows: 2,
        });
        terminal.feed(b"abcdefghijk");
        terminal.resize(TerminalSize {
            columns: 5,
            rows: 3,
        });

        let frame = terminal.frame();
        assert_eq!(frame.rows.len(), 3);
        assert!(
            frame
                .rows
                .iter()
                .all(|row| { row.iter().map(|run| run.width_cells).sum::<usize>() == 5 })
        );
    }

    #[test]
    fn paste_bytes_respect_bracketed_paste_mode() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 16,
            rows: 2,
        });
        assert_eq!(terminal.paste_bytes("a\nb"), b"a\nb");

        terminal.feed(b"\x1b[?2004h");
        assert_eq!(terminal.paste_bytes("a\nb"), b"\x1b[200~a\nb\x1b[201~");
    }

    #[test]
    fn mouse_tracking_modes_follow_remote_dec_modes() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 80,
            rows: 24,
        });
        assert_eq!(
            terminal.mouse_tracking_mode(),
            super::MouseTrackingMode::Disabled
        );

        terminal.feed(b"\x1b[?1000h");
        assert_eq!(
            terminal.mouse_tracking_mode(),
            super::MouseTrackingMode::Click
        );
        terminal.feed(b"\x1b[?1002h");
        assert_eq!(
            terminal.mouse_tracking_mode(),
            super::MouseTrackingMode::ButtonMotion
        );
        terminal.feed(b"\x1b[?1003h");
        assert_eq!(
            terminal.frame().mouse_tracking_mode,
            super::MouseTrackingMode::AnyMotion
        );
        terminal.feed(b"\x1b[?1003l");
        assert_eq!(
            terminal.mouse_tracking_mode(),
            super::MouseTrackingMode::Disabled
        );
    }

    #[test]
    fn legacy_mouse_reports_encode_press_release_and_motion() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 80,
            rows: 24,
        });
        let position = TerminalPosition { row: 3, column: 4 };
        let modifiers = super::TerminalMouseModifiers::default();
        terminal.feed(b"\x1b[?1000h");

        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Press {
                button: super::TerminalMouseButton::Left,
                position,
                modifiers,
            }),
            Some(b"\x1b[M %$".to_vec())
        );
        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Release {
                button: super::TerminalMouseButton::Left,
                position,
                modifiers,
            }),
            Some(b"\x1b[M#%$".to_vec())
        );

        terminal.feed(b"\x1b[?1002h");
        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Move {
                button: Some(super::TerminalMouseButton::Left),
                position,
                modifiers,
            }),
            Some(b"\x1b[M@%$".to_vec())
        );
    }

    #[test]
    fn sgr_mouse_reports_preserve_modifiers_and_wheel_steps() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 80,
            rows: 24,
        });
        let position = TerminalPosition { row: 0, column: 0 };
        let modifiers = super::TerminalMouseModifiers {
            shift: true,
            alt: false,
            control: true,
        };
        terminal.feed(b"\x1b[?1000h\x1b[?1006h");

        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Press {
                button: super::TerminalMouseButton::Left,
                position,
                modifiers,
            }),
            Some(b"\x1b[<20;1;1M".to_vec())
        );
        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Wheel {
                rows: 2,
                position,
                modifiers,
            }),
            Some(b"\x1b[<84;1;1M\x1b[<84;1;1M".to_vec())
        );
        assert_eq!(
            terminal.mouse_report_bytes(super::TerminalMouseEvent::Release {
                button: super::TerminalMouseButton::Left,
                position,
                modifiers,
            }),
            Some(b"\x1b[<20;1;1m".to_vec())
        );
    }

    #[test]
    fn utf8_mouse_encoding_supports_columns_beyond_legacy_limit() {
        let mut terminal = TerminalEmulator::new(TerminalSize {
            columns: 240,
            rows: 24,
        });
        terminal.feed(b"\x1b[?1000h\x1b[?1005h");
        let report = terminal
            .mouse_report_bytes(super::TerminalMouseEvent::Press {
                button: super::TerminalMouseButton::Left,
                position: TerminalPosition {
                    row: 0,
                    column: 224,
                },
                modifiers: super::TerminalMouseModifiers::default(),
            })
            .unwrap();

        assert_eq!(
            report,
            [b"\x1b[M ".as_slice(), &[0xc4, 0x81, b'!']].concat()
        );
    }

    #[test]
    fn key_input_encodes_text_control_and_terminal_modes() {
        assert_eq!(key("a", Some("a")).encode(false).unwrap(), b"a");
        assert_eq!(
            key("中", Some("中文🙂")).encode(false).unwrap(),
            "中文🙂".as_bytes()
        );

        let mut control_c = key("c", Some("c"));
        control_c.control = true;
        assert_eq!(control_c.encode(false).unwrap(), [0x03]);

        assert_eq!(key("up", None).encode(false).unwrap(), b"\x1b[A");
        assert_eq!(key("up", None).encode(true).unwrap(), b"\x1bOA");
        assert_eq!(key("f5", None).encode(false).unwrap(), b"\x1b[15~");
    }

    #[test]
    fn viewport_dimensions_follow_the_terminal_content_bounds() {
        let size = TerminalSize::for_window(856.0, 584.0);
        assert_eq!(size.columns, 100);
        assert_eq!(size.rows, 24);
        assert_eq!(CELL_WIDTH_PX, 8.0);
        assert_eq!(CELL_HEIGHT_PX, 18.0);
    }

    fn luminance(color: u32) -> f32 {
        fn channel(value: u8) -> f32 {
            let value = value as f32 / 255.0;
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        let r = channel((color >> 16) as u8);
        let g = channel((color >> 8) as u8);
        let b = channel(color as u8);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast(first: u32, second: u32) -> f32 {
        let (lighter, darker) = {
            let first = luminance(first);
            let second = luminance(second);
            if first > second {
                (first, second)
            } else {
                (second, first)
            }
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn selection_colors_stay_readable_on_every_terminal_theme() {
        for theme in TerminalTheme::ALL {
            let palette = theme.palette();
            assert!(
                contrast(palette.selection_foreground, palette.selection_background) >= 4.5,
                "{theme:?} selection text contrast is too low"
            );
            assert_ne!(
                palette.selection_background, palette.background,
                "{theme:?} selection must stand out from the terminal background"
            );
        }
    }
}
