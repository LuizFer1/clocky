//! Project chips wrapped in rows. Slint has no flex-wrap, so the rows are computed here from an
//! estimate of each chip's width (12px medium text, dot, padding).

use slint::{Color, ModelRc, VecModel};

use crate::ui;

/// Gap between chips (`space-1-5`).
const GAP: f32 = 6.0;
/// Dot (8) + gap (6) + padding (2 × 10) + border (2).
const CHROME: f32 = 36.0;
/// Average advance of a 12px Geist Medium character, rounded up.
const CHAR: f32 = 7.0;

/// Estimated width of a chip with this label.
pub fn chip_width(label: &str) -> f32 {
    // Labels are short (project names), so the count fits an f32 exactly.
    #[expect(clippy::cast_precision_loss, reason = "short labels")]
    let chars = label.chars().count() as f32;
    CHROME + chars * CHAR
}

/// Splits the labels into rows no wider than `max_width` (a chip wider than that gets a row of
/// its own). Returns the indices of each row.
pub fn wrap(labels: &[&str], max_width: f32) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (index, label) in labels.iter().enumerate() {
        let width = chip_width(label);
        match rows.last_mut() {
            Some(row) if used + GAP + width <= max_width => {
                row.push(index);
                used += GAP + width;
            }
            _ => {
                rows.push(vec![index]);
                used = width;
            }
        }
    }
    rows
}

/// A chip for the "Sem projeto" choice (id -1).
pub fn no_project_chip(selected: bool) -> ui::ProjectItem {
    ui::ProjectItem {
        id: -1,
        name: crate::manage_view::NO_PROJECT.into(),
        color: Color::default(),
        selected,
        ..ui::ProjectItem::default()
    }
}

/// The chips as wrapped rows for Slint.
pub fn rows(chips: &[ui::ProjectItem], max_width: f32) -> ModelRc<ui::ChipRow> {
    let labels: Vec<String> = chips.iter().map(|c| c.name.to_string()).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let rows: Vec<ui::ChipRow> = wrap(&labels, max_width)
        .into_iter()
        .map(|row| {
            let items: Vec<ui::ProjectItem> = row
                .into_iter()
                .filter_map(|i| chips.get(i).cloned())
                .collect();
            ui::ChipRow {
                chips: ModelRc::new(VecModel::from(items)),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(rows))
}

#[cfg(test)]
mod tests {
    use slint::Model;

    use super::*;

    #[test]
    fn short_labels_share_a_row() {
        assert_eq!(wrap(&["a", "b", "c"], 200.0), vec![vec![0, 1, 2]]);
    }

    #[test]
    fn handoff_chips_wrap_like_screen_61() {
        // 308 = picker width minus paddings: three projects, then "Sem projeto".
        let rows = wrap(&["connect", "faculdade", "pessoal", "Sem projeto"], 308.0);
        assert_eq!(rows, vec![vec![0, 1, 2], vec![3]]);
    }

    #[test]
    fn a_wide_chip_gets_its_own_row() {
        let long = "x".repeat(100);
        assert_eq!(
            wrap(&["a", &long, "b"], 120.0),
            vec![vec![0], vec![1], vec![2]]
        );
        assert_eq!(wrap(&[], 120.0), Vec::<Vec<usize>>::new());
    }

    #[test]
    fn rows_keep_the_chip_data() {
        let chips = vec![no_project_chip(true), no_project_chip(false)];
        let model = rows(&chips, 1000.0);
        assert_eq!(model.row_count(), 1);
        let row = model.row_data(0).unwrap();
        assert_eq!(row.chips.row_count(), 2);
        assert!(row.chips.row_data(0).unwrap().selected);
    }
}
