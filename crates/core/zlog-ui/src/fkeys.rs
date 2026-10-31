use egui::{RichText, Ui};

use crate::Section;

pub const SHOW: &str = "fkeys";

pub const MIN_WIDTH: f32 = 84.0;

pub fn shown(section: &Section) -> bool {
    section.get(SHOW).is_none_or(|v| v != "false")
}

pub fn set_shown(section: &mut Section, on: bool) {
    section.insert(SHOW.into(), on.to_string());
}

fn clip(text: &str, chars: usize) -> String {
    let text = text.trim();
    match text.chars().count() > chars {
        true => {
            let head: String = text.chars().take(chars.saturating_sub(1)).collect();
            format!("{head}…")
        }
        false => text.to_string(),
    }
}

pub fn message_keys(ui: &mut Ui, messages: &[String]) -> Option<u8> {
    let n = messages.len().min(12);
    if n == 0 {
        return None;
    }
    let spacing = ui.spacing().item_spacing.x;
    let room = ui.available_width();
    let cols = crate::layout::columns(room, 6, MIN_WIDTH, spacing).min(n);
    let width = ((room - spacing * (cols as f32 - 1.0)) / cols as f32).floor();
    let font = egui::TextStyle::Button.resolve(ui.style());
    let glyph = ui.fonts_mut(|f| f.glyph_width(&font, '0')).max(1.0);
    let chars = ((width - 2.0 * ui.spacing().button_padding.x) / glyph)
        .floor()
        .max(3.0) as usize;
    let height =
        2.0 * ui.text_style_height(&egui::TextStyle::Button) + 2.0 * ui.spacing().button_padding.y;
    let mut clicked = None;
    ui.vertical(|ui| {
        for row in (0..n).collect::<Vec<_>>().chunks(cols) {
            ui.horizontal(|ui| {
                for &i in row {
                    let text = messages[i].trim();
                    let label = format!("F{}\n{}", i + 1, clip(text, chars));
                    let rich = match text.is_empty() {
                        true => RichText::new(label).weak(),
                        false => RichText::new(label),
                    };
                    let r = ui.add_sized(
                        [width, height],
                        egui::Button::new(rich).wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    let r = match text.is_empty() {
                        true => r,
                        false => r.on_hover_text(text),
                    };
                    if r.clicked() {
                        clicked = Some(i as u8 + 1);
                    }
                }
            });
        }
    });
    clicked
}

pub fn message_table(ui: &mut Ui, id: &str, banks: &mut [(&str, &mut [String; 12])]) {
    if banks.is_empty() {
        return;
    }
    let key = egui::Id::unique(("fkey_bank", id));
    let mut bank = ui
        .data(|d| d.get_temp::<usize>(key))
        .unwrap_or(0)
        .min(banks.len() - 1);
    if banks.len() > 1 {
        let options: Vec<(usize, &str)> = banks.iter().enumerate().map(|(i, b)| (i, b.0)).collect();
        crate::layout::segmented(ui, &mut bank, &options);
        ui.data_mut(|d| d.insert_temp(key, bank));
    }
    crate::form(ui, (id, bank), &["F12"], |ui, form| {
        for (n, m) in banks[bank].1.iter_mut().enumerate() {
            let key = ui.label(format!("F{}", n + 1));
            form.text(ui, egui::TextEdit::singleline(m))
                .labelled_by(key.id);
            ui.end_row();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_edits_the_chosen_bank() {
        use egui_kittest::kittest::Queryable;
        let mut a: [String; 12] = std::array::from_fn(|n| format!("A{n}"));
        let mut b: [String; 12] = std::array::from_fn(|n| format!("B{n}"));
        {
            let mut h = egui_kittest::Harness::builder()
                .with_size([280.0, 600.0])
                .build_ui(|ui| {
                    message_table(ui, "t", &mut [("Bank A", &mut a), ("Bank B", &mut b)])
                });
            h.run();
            assert!(h.query_all_by_value("A0").next().is_some());
            h.query_all_by_label("Bank B").next().unwrap().click();
            h.run();
            assert!(
                h.query_all_by_value("B11").next().is_some()
                    && h.query_all_by_value("A0").next().is_none()
            );
            assert!(h.query_all_by_value("B0").next().unwrap().rect().max.x <= 280.0);
            h.query_all_by_value("B0").next().unwrap().focus();
            h.run();
            h.query_all_by_value("B0").next().unwrap().type_text("X");
            h.run();
        }
        assert!(b[0].contains('X'), "{:?}", b[0]);
        assert_eq!(a[0], "A0");
    }

    #[test]
    fn long_messages_are_clipped_with_an_ellipsis() {
        assert_eq!(clip("CQ TEST JA1ZLO", 8), "CQ TEST…");
        assert_eq!(clip(" TU ", 8), "TU");
    }

    #[test]
    fn the_buttons_fit_a_narrow_window_and_name_the_message() {
        use egui_kittest::kittest::Queryable;
        let messages: Vec<String> = (1..=12)
            .map(|n| match n {
                4 => String::new(),
                _ => format!("CQ TEST JA1ZLO MESSAGE {n}"),
            })
            .collect();
        for width in [280.0, 900.0] {
            let mut clicked = None;
            let mut h = egui_kittest::Harness::builder()
                .with_size([width, 300.0])
                .build_ui(|ui| {
                    if let Some(n) = message_keys(ui, &messages) {
                        clicked = Some(n);
                    }
                });
            h.run();
            let key = h.get_by(|n| n.label().is_some_and(|l| l.starts_with("F2\nCQ")));
            assert!(key.rect().max.x <= width, "{width}");
            let rows: std::collections::BTreeSet<i32> = h
                .query_all_by(|n| {
                    n.label()
                        .is_some_and(|l| l.starts_with('F') && l.contains('\n'))
                })
                .map(|n| n.rect().min.y as i32)
                .collect();
            match width < 300.0 {
                true => assert!((4..=6).contains(&rows.len()), "{width}: {rows:?}"),
                false => assert_eq!(rows.len(), 2, "{width}"),
            }
            assert!(h
                .query_by(|n| n.label().as_deref() == Some("F4\n"))
                .is_some());
            h.get_by(|n| n.label().is_some_and(|l| l.starts_with("F2\n")))
                .click();
            h.run();
            drop(h);
            assert_eq!(clicked, Some(2));
        }
    }
}
