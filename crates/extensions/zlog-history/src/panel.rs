use egui::{RichText, Ui};
use zlog_core::{Band, Contest, Qso};
use zlog_ui::{Agreement, Command, EntryField, Panel, Section, StatusItem, Suggestion, View};

use crate::config::{Config, Source};
use crate::index::{Index, Pick};
use crate::loader::{Built, Formats, Loading, Problem};
use crate::texts;

#[derive(Default)]
pub struct History {
    config: Config,
    formats: Option<Formats>,
    built_of: Option<Vec<Source>>,
    loading: Option<Loading>,
    built: Option<Built>,
}

impl History {
    pub fn new(formats: Formats) -> Self {
        History {
            formats: Some(formats),
            ..History::default()
        }
    }

    fn want(&mut self, wanted: Vec<Source>) {
        if self.built_of.as_ref() != Some(&wanted) {
            if self.built_of.is_some() {
                self.built = None;
            }
            self.loading = Some(Loading::start(wanted.clone(), self.formats));
            self.built_of = Some(wanted);
        }
    }

    fn index(&self) -> Option<&Index> {
        self.built.as_ref().map(|b| &b.index)
    }
}

fn source_text(index: &Index, pick: &Pick, lang: &str) -> String {
    let file = index.sources.get(pick.source).map_or("", String::as_str);
    let text = if pick.agree == pick.of {
        file.to_string()
    } else {
        texts().format(lang, "source.split", &[&file, &pick.agree, &pick.of])
    };
    match &pick.via {
        Some(via) => texts().format(lang, "source.via", &[&text, via]),
        None => text,
    }
}

pub fn suggest(
    index: &Index,
    name_memo: bool,
    call: &str,
    contest: &dyn Contest,
    band: Band,
    lang: &str,
) -> Vec<Suggestion> {
    let call = call.trim();
    let mut out = Vec::new();
    if call.is_empty() {
        return out;
    }
    if let Some(p) = index.exchange(call) {
        let past = Qso {
            callsign: call.to_string(),
            nr_rcvd: p.text.clone(),
            band,
            ..Qso::default()
        };
        if let Some(text) = contest.exchange_from(band, &past).filter(|t| !t.is_empty()) {
            let source = source_text(index, &p, lang);
            let s = Suggestion::new(EntryField::Number, text, call, source);
            out.push(s.from_call(p.via.clone().unwrap_or_default()));
        }
    }
    if let Some(p) = index.name(call).filter(|_| name_memo) {
        let source = source_text(index, &p, lang);
        let s = Suggestion::new(EntryField::Memo, p.text, call, source);
        out.push(s.from_call(p.via.unwrap_or_default()));
    }
    out
}

pub fn status_of(s: &Suggestion, typed: &str, lang: &str) -> Option<StatusItem> {
    let t = texts();
    let field = t.get(
        lang,
        if s.field == EntryField::Memo {
            "field.name"
        } else {
            "field.number"
        },
    );
    let field = if s.from_call.is_empty() {
        field.to_string()
    } else {
        t.format(lang, "field.via", &[&field, &s.from_call])
    };
    match s.agreement(typed) {
        Agreement::Same => None,
        Agreement::Empty => {
            Some(
                StatusItem::new(t.format(lang, "status.offer", &[&field, &s.text]))
                    .tooltip(t.format(lang, "status.offer.tip", &[&s.source])),
            )
        }
        Agreement::Differs => Some(
            StatusItem::new(t.format(lang, "status.differs", &[&field, &s.text]))
                .tooltip(t.format(lang, "status.differs.tip", &[&s.source]))
                .warning(true),
        ),
    }
}

fn problem_text(p: &Problem, lang: &str) -> String {
    let t = texts();
    match p {
        Problem::Missing(e) => t.format(lang, "problem.missing", &[e]),
        Problem::Unknown => t.get(lang, "problem.unknown").into(),
        Problem::Broken(e) => t.format(lang, "problem.broken", &[e]),
    }
}

impl Panel for History {
    fn id(&self) -> &str {
        "hist"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [520.0, 340.0]
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Search
    }

    fn section(&self) -> (zlog_ui::MenuSection, i32) {
        (zlog_ui::MenuSection::Check, 50)
    }

    fn commands(&self, language: &str) -> Vec<Command> {
        vec![Command::window(
            texts().get(language, "toolbar"),
            texts().get(language, "toolbar.tip"),
        )
        .icon(zlog_ui::icon::CLOCK_COUNTER_CLOCKWISE)
        .role("history")]
    }

    fn toolbar_default_shown(&self) -> bool {
        false
    }

    fn tick_after(&self) -> Option<std::time::Duration> {
        self.loading
            .as_ref()
            .map(|_| std::time::Duration::from_millis(250))
    }

    fn tick(&mut self, view: &View<'_>) {
        if !self.config.enabled {
            (self.built_of, self.loading, self.built) = (None, None, None);
            return;
        }
        self.want(self.config.sources_for(view.contest.name()));
        if let Some(result) = self.loading.as_ref().and_then(Loading::poll) {
            self.loading = None;
            self.built = result.ok();
        }
    }

    fn suggestions(&self, view: &View<'_>) -> Vec<Suggestion> {
        match self.index() {
            Some(ix) if self.config.enabled => suggest(
                ix,
                self.config.name_memo,
                view.callsign,
                view.contest,
                view.band,
                view.language,
            ),
            _ => Vec::new(),
        }
    }

    fn status(&self, view: &View<'_>) -> Vec<StatusItem> {
        let t = texts();
        let lang = view.language;
        let mut items = Vec::new();
        if !self.config.enabled {
            return items;
        }
        if self.loading.is_some() {
            items.push(StatusItem::new(t.get(lang, "status.loading")));
        }
        if let Some(b) = self.built.as_ref().filter(|b| !b.problems.is_empty()) {
            let tip: Vec<String> = b
                .problems
                .iter()
                .map(|(path, p)| format!("{path}: {}", problem_text(p, lang)))
                .collect();
            items.push(
                StatusItem::new(t.format(lang, "status.problems", &[&b.problems.len()]))
                    .tooltip(tip.join("\n"))
                    .warning(true),
            );
        }
        for s in self.suggestions(view) {
            let typed = if s.field == EntryField::Memo {
                view.memo
            } else {
                view.number
            };
            items.extend(status_of(&s, typed, lang));
        }
        items
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let t = texts();
        let lang = view.language;
        if !self.config.enabled {
            ui.label(t.get(lang, "off"));
            return;
        }
        let state = match (&self.loading, &self.built) {
            (Some(_), _) => t.get(lang, "status.loading").to_string(),
            (None, Some(b)) => t.format(lang, "loaded", &[&b.index.len(), &b.index.sources.len()]),
            (None, None) => t.get(lang, "none").to_string(),
        };
        zlog_ui::layout::toolbar(ui, &[(&state, false)], |ui| {
            if zlog_ui::layout::tool(ui, zlog_ui::icon::ARROW_CLOCKWISE, t.get(lang, "reload"))
                .on_hover_text(t.get(lang, "reload.tip"))
                .clicked()
            {
                self.built_of = None;
            }
        });
        if let Some(b) = self.built.as_ref().filter(|b| !b.problems.is_empty()) {
            for (path, p) in &b.problems {
                let line = format!("{path}: {}", problem_text(p, lang));
                zlog_ui::layout::status(ui, "", Some(&line));
            }
        }
        let call = view.callsign.trim();
        let Some(ix) = self.index() else {
            return;
        };
        if call.is_empty() {
            ui.label(t.get(lang, "type_call"));
            return;
        }
        let entries = ix.lookup(call).entries;
        if entries.is_empty() {
            ui.label(t.format(lang, "unknown", &[&call]));
            return;
        }
        zlog_ui::layout::status(ui, &t.format(lang, "count", &[&call, &entries.len()]), None);
        egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
            egui::Grid::new("hist_entries")
                .striped(true)
                .num_columns(5)
                .min_col_width(ui.available_width() / 6.0)
                .show(ui, |ui| {
                    for key in [
                        "col.source",
                        "col.when",
                        "col.number",
                        "col.name",
                        "col.other",
                    ] {
                        ui.label(RichText::new(t.get(lang, key)).strong());
                    }
                    ui.end_row();
                    for e in entries.iter().rev() {
                        ui.label(ix.sources.get(e.source).map_or("", String::as_str));
                        let when = match (e.time, e.band) {
                            (Some(time), Some(band)) => {
                                let (y, mo, d, _, _, _) = time.to_ymd_hms();
                                format!("{y:04}-{mo:02}-{d:02} {}", band.label())
                            }
                            _ => String::new(),
                        };
                        ui.label(when);
                        ui.label(RichText::new(&e.exchange).monospace());
                        ui.label(&e.name);
                        let other: Vec<String> =
                            e.extra.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                        ui.label(other.join(", "));
                        ui.end_row();
                    }
                });
        });
    }

    fn load(&mut self, section: &Section) {
        self.config = Config::load(section);
    }

    fn save(&self, section: &mut Section) {
        self.config.save(section);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table::Row;
    use zlog_core::{ContestMode, Log, Power, ScoreSheet, Scoring, Station};

    struct Test {
        serial: bool,
    }

    impl Contest for Test {
        fn name(&self) -> &str {
            "test"
        }
        fn band_range(&self) -> (Band, Band) {
            (Band::B1_9, Band::B50)
        }
        fn contest_mode(&self) -> ContestMode {
            ContestMode::Mix
        }
        fn sent_number(&self, _: Band, _: Power, _: &Station) -> String {
            String::new()
        }
        fn valid_number(&self, _: &Qso) -> bool {
            true
        }
        fn score(&mut self, _: &Log) -> Scoring {
            Scoring::default()
        }
        fn exchange_from(&self, _: Band, past: &Qso) -> Option<String> {
            (!self.serial).then(|| past.nr_rcvd.clone())
        }
    }

    fn index() -> Index {
        let mut ix = Index::default();
        let s = ix.add_source("hist.txt");
        let row = |call: &str, exch: &str, name: &str| Row {
            callsign: call.into(),
            exchange: exch.into(),
            name: name.into(),
            extra: Vec::new(),
        };
        ix.add_rows(
            s,
            vec![row("JA1ZLO", "10M", "Taro"), row("JG1VPP", "13M", "")],
        );
        ix
    }

    #[test]
    fn suggests_number_and_name_for_the_callsign_typed() {
        let ix = index();
        let contest = Test { serial: false };
        let s = suggest(&ix, true, "ja1zlo ", &contest, Band::B7, "en");
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].field, s[0].text.as_str()),
            (EntryField::Number, "10M")
        );
        assert_eq!((s[1].field, s[1].text.as_str()), (EntryField::Memo, "Taro"));
        assert!(s
            .iter()
            .all(|s| s.is_for("JA1ZLO") && s.source == "hist.txt"));
        assert_eq!(
            suggest(&ix, false, "JA1ZLO", &contest, Band::B7, "en").len(),
            1
        );
        assert!(suggest(&ix, true, "", &contest, Band::B7, "en").is_empty());
        assert!(suggest(&ix, true, "JA1YWX", &contest, Band::B7, "en").is_empty());
    }

    #[test]
    fn a_suggestion_found_via_the_base_call_says_so() {
        let ix = index();
        let contest = Test { serial: false };
        let s = suggest(&ix, true, "JA1ZLO/1", &contest, Band::B7, "en");
        assert_eq!(s[0].from_call, "JA1ZLO");
        assert_eq!(s[0].source, "hist.txt, of JA1ZLO");
        let st = status_of(&s[0], "", "en").unwrap();
        assert!(st.text.contains("of JA1ZLO"), "{}", st.text);
        let exact = suggest(&ix, true, "JA1ZLO", &contest, Band::B7, "en");
        assert!(exact[0].from_call.is_empty());
    }

    #[test]
    fn other_files_drop_the_old_index_and_unchanged_settings_keep_it() {
        let built = || Built {
            index: index(),
            ..Built::default()
        };
        let mut h = History::default();
        h.want(Vec::new());
        h.built = Some(built());
        h.load(&Section::default());
        h.want(Vec::new());
        assert!(h.built.is_some(), "unchanged settings keep the index");
        h.built_of = None;
        h.want(Vec::new());
        assert!(h.built.is_some());
        let other = Source {
            path: "other.txt".into(),
            contest: String::new(),
        };
        h.want(vec![other]);
        assert!(h.built.is_none(), "another contest's files are not offered");
    }

    #[test]
    fn no_number_in_contests_of_serial_numbers() {
        let s = suggest(
            &index(),
            true,
            "JA1ZLO",
            &Test { serial: true },
            Band::B7,
            "en",
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].field, EntryField::Memo);
    }

    #[test]
    fn status_offers_while_empty_and_warns_when_typed_differs() {
        let s = Suggestion::new(EntryField::Number, "10M", "JA1ZLO", "hist.txt");
        let offer = status_of(&s, "", "en").unwrap();
        assert!(!offer.warning && offer.text.contains("10M"));
        assert!(status_of(&s, "10m", "en").is_none());
        let differs = status_of(&s, "11M", "en").unwrap();
        assert!(differs.warning && differs.text.contains("10M"));
    }

    #[test]
    fn memo_status_follows_the_memo_typed() {
        let h = History {
            built: Some(Built {
                index: index(),
                ..Built::default()
            }),
            ..History::default()
        };
        let contest = Test { serial: true };
        let (log, sheet, station) = (Log::default(), ScoreSheet::default(), Station::default());
        let status = |memo: &str| {
            let view = View {
                log: &log,
                contest: &contest,
                sheet: &sheet,
                callsign: "JA1ZLO",
                station: &station,
                number: "",
                memo,
                rst_sent: "",
                power: Power::P010,
                band: Band::B7,
                mode: zlog_core::Mode::Cw,
                language: "en",
                spots: &[],
                hz: None,
                rig: 1,
                cq: false,
                operator: "",
                period_hours: 0,
                pc_name: "",
                log_path: "",
                focus: None,
                editing: None,
                selected: None,
                period: &(),
                tab: Default::default(),
                changes: 0,
            };
            h.status(&view)
        };
        let offer = status("");
        assert!(offer.len() == 1 && !offer[0].warning && offer[0].text.contains("Taro"));
        assert!(status("taro").is_empty());
        let differs = status("Jiro");
        assert!(differs.len() == 1 && differs[0].warning);
    }

    #[test]
    fn disagreeing_entries_are_counted_in_the_source() {
        let mut ix = index();
        let s = ix.add_source("old.zlox");
        ix.add_qsos(
            s,
            &[Qso {
                callsign: "JA1ZLO".into(),
                nr_rcvd: "11M".into(),
                ..Qso::default()
            }],
        );
        let p = ix.exchange("JA1ZLO").unwrap();
        assert_eq!(source_text(&ix, &p, "en"), "old.zlox (1 of 2)");
    }
}
