use egui::ThemePreference;
use zlog_ui::{Section, SettingsPage};

zlog_core::texts!();

pub const SECTION: &str = "display";

const THEMES: [(ThemePreference, &str); 3] = [
    (ThemePreference::System, "system"),
    (ThemePreference::Light, "light"),
    (ThemePreference::Dark, "dark"),
];

pub fn theme(section: &Section) -> ThemePreference {
    let saved = section.get("theme").map(String::as_str);
    THEMES
        .iter()
        .find(|(_, code)| Some(*code) == saved)
        .map_or(ThemePreference::System, |(t, _)| *t)
}

pub fn apply(ctx: &egui::Context, section: &Section) {
    ctx.set_theme(theme(section));
}

#[derive(Default)]
pub struct DisplayPage;

impl SettingsPage for DisplayPage {
    fn id(&self) -> &str {
        SECTION
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, section: &mut Section, language: &str) {
        let t = texts();
        let mut chosen = theme(section);
        ui.horizontal(|ui| {
            ui.label(t.get(language, "theme"));
            for (theme, code) in THEMES {
                let key = format!("theme.{code}");
                ui.radio_value(&mut chosen, theme, t.get(language, &key));
            }
        });
        if chosen != theme(section) {
            let code = THEMES.iter().find(|(t, _)| *t == chosen).map(|(_, c)| *c);
            section.insert("theme".into(), code.unwrap_or("system").into());
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.style(SECTION, apply);
    b.page(120, |_| DisplayPage);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn the_system_theme_unless_another_is_kept() {
        let mut s = Section::new();
        assert_eq!(theme(&s), ThemePreference::System);
        for (t, code) in THEMES {
            s.insert("theme".into(), code.into());
            assert_eq!(theme(&s), t);
        }
        s.insert("theme".into(), "purple".into());
        assert_eq!(theme(&s), ThemePreference::System);
    }
}
