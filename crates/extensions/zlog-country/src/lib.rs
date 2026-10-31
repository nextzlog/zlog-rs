use std::sync::Arc;

use zlog_rule_dx::cty::Cty;
use zlog_ui::{Command, Panel, Section, StatusItem, View};

zlog_core::texts!();

const SHOWN: &str = "shown";

pub struct CountryStatus {
    shown: bool,
    cty: Option<Arc<Cty>>,
}

impl Default for CountryStatus {
    fn default() -> Self {
        CountryStatus {
            shown: true,
            cty: None,
        }
    }
}

impl CountryStatus {
    pub fn with_cty(cty: Cty) -> Self {
        CountryStatus {
            shown: true,
            cty: Some(Arc::new(cty)),
        }
    }

    fn cty(&self) -> Arc<Cty> {
        self.cty.clone().unwrap_or_else(Cty::global)
    }
}

pub fn item(cty: &Cty, callsign: &str, language: &str) -> Option<StatusItem> {
    let call = callsign.trim();
    if call.is_empty() {
        return None;
    }
    let t = texts();
    if !cty.is_loaded() {
        return Some(
            StatusItem::new(t.get(language, "no_file")).tooltip(t.format(
                language,
                "no_file.tip",
                &[&zlog_ui::product::NAME],
            )),
        );
    }
    let found = cty.lookup(call, false);
    if found.is_unknown() {
        return Some(StatusItem::new(t.get(language, "unknown")).warning(true));
    }
    let (cq, itu) = (
        cty.guess_cq_zone(call, false),
        cty.guess_itu_zone(call, false),
    );
    let args: [&dyn std::fmt::Display; 4] = [&found.country.name, &found.continent(), &cq, &itu];
    Some(
        StatusItem::new(zlog_core::text::substitute(t.get(language, "item"), &args)).tooltip(
            zlog_core::text::substitute(t.get(language, "item.tip"), &args),
        ),
    )
}

impl Panel for CountryStatus {
    fn migrations(&self) -> Vec<zlog_ui::migration::Migration> {
        vec![zlog_ui::migration::Migration::RenamedPanel(
            "country", "qth",
        )]
    }

    fn id(&self) -> &str {
        "qth"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::None
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Settings
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        let mut c = Command::new(SHOWN, self.title(language), texts().get(language, "tip"))
            .selected(self.shown);
        c.checkable = true;
        vec![c]
    }

    fn run_command(&mut self, id: &str) {
        if id == SHOWN {
            self.shown = !self.shown;
        }
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        if !self.shown {
            return Vec::new();
        }
        item(&self.cty(), view.callsign, view.language)
            .into_iter()
            .collect()
    }

    fn load(&mut self, section: &Section) {
        self.shown = section.get(SHOWN).is_none_or(|v| v != "false");
    }

    fn save(&self, section: &mut Section) {
        section.insert(SHOWN.into(), self.shown.to_string());
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(600, |_| CountryStatus::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTY: &str = "Japan:                    25:  45:  AS:   36.40:  -138.38:    -9.0:  JA:\n    JA,JE,JF,JG,JH,JI,JJ,JK,JL,JM,JN,JO,JP,JQ,JR,JS,7J,7K,7L,7M,7N,8J,8K,8L,8M,8N;\n";

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn shows_the_country_and_zones_of_the_callsign_being_typed() {
        let cty = Cty::parse(CTY);
        let i = item(&cty, "JG1VPP", "en").unwrap();
        assert_eq!(i.text, "Japan · AS · CQ 25 · ITU 45");
        assert!(!i.warning);
        assert!(item(&cty, "  ", "en").is_none());
        assert!(item(&cty, "W1AW", "en").unwrap().warning);
        let none = item(&Cty::default(), "JG1VPP", "ja").unwrap();
        assert_eq!(none.text, "CTY.DAT がありません");
    }

    #[test]
    fn the_toggle_hides_it_and_is_remembered() {
        let mut p = CountryStatus::with_cty(Cty::parse(CTY));
        assert!(p.commands("en")[0].selected);
        p.run_command(SHOWN);
        let mut s = Section::new();
        p.save(&mut s);
        let mut q = CountryStatus::default();
        q.load(&s);
        assert!(!q.shown);
    }
}
