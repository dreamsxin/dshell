use crate::terminal::{TerminalFrame, TerminalRun};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffKind {
    Equal,
    Changed,
    LeftOnly,
    RightOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffLine {
    pub left: Option<String>,
    pub right: Option<String>,
    pub kind: DiffKind,
}

pub fn visible_terminal_lines(frame: &TerminalFrame) -> Vec<String> {
    let mut lines = frame
        .rows
        .iter()
        .map(|row| row_text(row))
        .collect::<Vec<_>>();
    let Some(first) = lines.iter().position(|line| !line.is_empty()) else {
        return Vec::new();
    };
    let last = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .unwrap_or(first);
    lines.drain(last + 1..);
    lines.drain(..first);
    lines
}

fn row_text(row: &[TerminalRun]) -> String {
    row.iter()
        .map(|run| run.text.as_str())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

pub fn compare_lines(left: &[String], right: &[String]) -> Vec<DiffLine> {
    let columns = right.len() + 1;
    let mut lcs = vec![0usize; (left.len() + 1) * columns];

    for left_index in (0..left.len()).rev() {
        for right_index in (0..right.len()).rev() {
            let index = left_index * columns + right_index;
            lcs[index] = if left[left_index] == right[right_index] {
                1 + lcs[(left_index + 1) * columns + right_index + 1]
            } else {
                lcs[(left_index + 1) * columns + right_index]
                    .max(lcs[left_index * columns + right_index + 1])
            };
        }
    }

    let mut rows = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut left_index = 0;
    let mut right_index = 0;

    while left_index < left.len() && right_index < right.len() {
        if left[left_index] == right[right_index] {
            append_unmatched(&mut rows, &mut removed, &mut added);
            rows.push(DiffLine {
                left: Some(left[left_index].clone()),
                right: Some(right[right_index].clone()),
                kind: DiffKind::Equal,
            });
            left_index += 1;
            right_index += 1;
        } else if lcs[(left_index + 1) * columns + right_index]
            >= lcs[left_index * columns + right_index + 1]
        {
            removed.push(left[left_index].clone());
            left_index += 1;
        } else {
            added.push(right[right_index].clone());
            right_index += 1;
        }
    }
    removed.extend(left[left_index..].iter().cloned());
    added.extend(right[right_index..].iter().cloned());
    append_unmatched(&mut rows, &mut removed, &mut added);
    rows
}

fn append_unmatched(rows: &mut Vec<DiffLine>, removed: &mut Vec<String>, added: &mut Vec<String>) {
    let paired = removed.len().min(added.len());
    for (left, right) in removed.drain(..paired).zip(added.drain(..paired)) {
        rows.push(DiffLine {
            left: Some(left),
            right: Some(right),
            kind: DiffKind::Changed,
        });
    }
    rows.extend(removed.drain(..).map(|left| DiffLine {
        left: Some(left),
        right: None,
        kind: DiffKind::LeftOnly,
    }));
    rows.extend(added.drain(..).map(|right| DiffLine {
        left: None,
        right: Some(right),
        kind: DiffKind::RightOnly,
    }));
}

#[cfg(test)]
mod tests {
    use super::{DiffKind, compare_lines, visible_terminal_lines};
    use crate::terminal::{MouseTrackingMode, TerminalFrame, TerminalRun};

    fn lines(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn comparison_aligns_equal_and_replaced_lines() {
        let diff = compare_lines(
            &lines(&["host", "old value", "ready"]),
            &lines(&["host", "new value", "ready"]),
        );

        assert_eq!(diff.len(), 3);
        assert_eq!(diff[0].kind, DiffKind::Equal);
        assert_eq!(diff[1].kind, DiffKind::Changed);
        assert_eq!(diff[1].left.as_deref(), Some("old value"));
        assert_eq!(diff[1].right.as_deref(), Some("new value"));
        assert_eq!(diff[2].kind, DiffKind::Equal);
    }

    #[test]
    fn comparison_keeps_insertions_aligned_to_context() {
        let diff = compare_lines(&lines(&["a", "b", "d"]), &lines(&["a", "x", "b", "d", "e"]));

        assert_eq!(diff.len(), 5);
        assert_eq!(diff[1].kind, DiffKind::RightOnly);
        assert_eq!(diff[1].right.as_deref(), Some("x"));
        assert_eq!(diff[2].kind, DiffKind::Equal);
        assert_eq!(diff[4].kind, DiffKind::RightOnly);
        assert_eq!(diff[4].right.as_deref(), Some("e"));
    }

    #[test]
    fn comparison_marks_deleted_lines() {
        let diff = compare_lines(&lines(&["a", "obsolete", "b"]), &lines(&["a", "b"]));

        assert_eq!(diff.len(), 3);
        assert_eq!(diff[1].kind, DiffKind::LeftOnly);
        assert_eq!(diff[1].left.as_deref(), Some("obsolete"));
        assert_eq!(diff[2].kind, DiffKind::Equal);
    }

    #[test]
    fn visible_terminal_lines_keep_indentation_and_drop_blank_padding() {
        let frame = TerminalFrame {
            rows: vec![
                Vec::new(),
                vec![TerminalRun {
                    text: "  host  ".to_owned(),
                    width_cells: 8,
                    foreground: 0,
                    background: 0,
                    bold: false,
                    italic: false,
                    underline: false,
                    selected: false,
                }],
                vec![TerminalRun {
                    text: "   ".to_owned(),
                    width_cells: 3,
                    foreground: 0,
                    background: 0,
                    bold: false,
                    italic: false,
                    underline: false,
                    selected: false,
                }],
                Vec::new(),
            ],
            selection_text: None,
            mouse_tracking_mode: MouseTrackingMode::Disabled,
            foreground: 0,
            background: 0,
            cursor: None,
        };

        assert_eq!(visible_terminal_lines(&frame), vec!["  host".to_owned()]);
    }

    #[test]
    fn comparison_handles_empty_and_identical_screens() {
        assert!(compare_lines(&[], &[]).is_empty());

        let identical = lines(&["same", "same"]);
        let diff = compare_lines(&identical, &identical);
        assert_eq!(diff.len(), 2);
        assert!(diff.iter().all(|line| line.kind == DiffKind::Equal));
    }
}
