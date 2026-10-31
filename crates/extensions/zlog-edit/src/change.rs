use zlog_core::{Command, Qso, QsoId};
use zlog_ui::View;

pub fn insertion_from(original: &Qso) -> Qso {
    Qso {
        callsign: String::new(),
        nr_rcvd: String::new(),
        memo: String::new(),
        ..original.clone()
    }
}

pub fn finish(view: &View<'_>, qso: &mut Qso) {
    if view.out_of_period(qso) {
        qso.invalid = true;
    }
    qso.callsign = qso.callsign.trim().to_ascii_uppercase();
    qso.nr_rcvd = qso.nr_rcvd.trim().to_ascii_uppercase();
    if qso.nr_sent.trim().is_empty() {
        qso.nr_sent = view.contest.sent_number(qso.band, qso.power, view.station);
    }
}

pub fn find(view: &View<'_>, id: QsoId) -> Option<usize> {
    view.log.qsos.iter().position(|q| QsoId::of(q) == id)
}

pub fn insert_before(original: &Qso, mut qso: Qso) -> Command {
    qso.reserve2 = original.reserve3;
    qso.reserve3 = original.reserve3;
    Command::InsertBefore(QsoId::of(original), qso)
}

pub fn toggle_invalid(qso: &Qso) -> Command {
    let mut q = qso.clone();
    q.invalid = !q.invalid;
    Command::Update(QsoId::of(qso), q)
}

pub fn after_delete(qsos: &[Qso], index: usize) -> Option<QsoId> {
    let next = if index + 1 < qsos.len() {
        index + 1
    } else {
        index.checked_sub(1)?
    };
    qsos.get(next).map(QsoId::of)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qso(call: &str, id: i32) -> Qso {
        Qso {
            callsign: call.into(),
            nr_rcvd: "20H".into(),
            memo: "QSB".into(),
            reserve3: id * 100,
            ..Qso::default()
        }
    }

    #[test]
    fn insert_before_links_the_original() {
        let original = qso("JA2BBB", 7);
        let mut q = insertion_from(&original);
        assert_eq!(
            (q.callsign.as_str(), q.nr_rcvd.as_str(), q.memo.as_str()),
            ("", "", "")
        );
        assert_eq!(q.time, original.time);
        q.callsign = "JA2CCC".into();
        let Command::InsertBefore(before, new) = insert_before(&original, q) else {
            panic!("not an insertion");
        };
        assert_eq!(before, QsoId::of(&original));
        assert_eq!(new.reserve2, original.reserve3);
        assert_eq!(new.callsign, "JA2CCC");
    }

    struct Before(f64);

    impl zlog_ui::PeriodCheck for Before {
        fn outside(&self, q: &Qso) -> bool {
            q.time.0 < self.0
        }
    }

    struct Prefecture;

    impl zlog_core::Contest for Prefecture {
        fn name(&self) -> &str {
            "prefecture"
        }
        fn band_range(&self) -> (zlog_core::Band, zlog_core::Band) {
            (zlog_core::Band::B1_9, zlog_core::Band::HIGHEST)
        }
        fn contest_mode(&self) -> zlog_core::ContestMode {
            zlog_core::ContestMode::Mix
        }
        fn sent_number(
            &self,
            _band: zlog_core::Band,
            _power: zlog_core::Power,
            station: &zlog_core::Station,
        ) -> String {
            format!("{}H", station.prov)
        }
        fn valid_number(&self, _q: &Qso) -> bool {
            true
        }
        fn score(&mut self, _log: &zlog_core::Log) -> zlog_core::Scoring {
            zlog_core::Scoring {
                qsos: Vec::new(),
                sheet: zlog_core::ScoreSheet::default(),
            }
        }
    }

    fn finished(q: &mut Qso, start: f64) {
        let contest = Prefecture;
        let (log, sheet) = (zlog_core::Log::default(), zlog_core::ScoreSheet::default());
        let station = zlog_core::Station {
            prov: "10".into(),
            ..zlog_core::Station::default()
        };
        let view = View {
            log: &log,
            contest: &contest,
            sheet: &sheet,
            callsign: "",
            station: &station,
            number: "",
            memo: "",
            rst_sent: "",
            power: zlog_core::Power::P010,
            band: zlog_core::Band::B7,
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
            period: &Before(start),
            tab: Default::default(),
            changes: 0,
        };
        finish(&view, q);
    }

    #[test]
    fn edit_refills_an_empty_sent_number() {
        let mut q = Qso {
            time: zlog_core::DelphiTime(1.1),
            callsign: " ja2aaa".into(),
            nr_rcvd: "20h".into(),
            band: zlog_core::Band::B7,
            power: zlog_core::Power::P010,
            nr_sent: "  ".into(),
            ..Qso::default()
        };
        finished(&mut q, 1.0);
        assert_eq!(
            (q.callsign.as_str(), q.nr_rcvd.as_str(), q.nr_sent.as_str()),
            ("JA2AAA", "20H", "10H")
        );
        assert!(!q.invalid);
        q.nr_sent = "11M".into();
        finished(&mut q, 1.0);
        assert_eq!(q.nr_sent, "11M");
    }

    #[test]
    fn edit_outside_the_period_marks_invalid() {
        let mut q = Qso {
            time: zlog_core::DelphiTime(0.5),
            ..Qso::default()
        };
        finished(&mut q, 1.0);
        assert!(q.invalid);
        q.time = zlog_core::DelphiTime(1.1);
        finished(&mut q, 1.0);
        assert!(q.invalid);
    }

    #[test]
    fn invalid_turns_over() {
        let q = qso("JA1ZLO", 1);
        let Command::Update(id, new) = toggle_invalid(&q) else {
            panic!("not an update");
        };
        assert_eq!((id, new.invalid), (QsoId::of(&q), true));
    }

    #[test]
    fn after_a_delete_the_next_qso_is_selected() {
        let qsos = [qso("A", 1), qso("B", 2), qso("C", 3)];
        assert_eq!(after_delete(&qsos, 0), Some(QsoId(2)));
        assert_eq!(after_delete(&qsos, 2), Some(QsoId(2)));
        assert_eq!(after_delete(&qsos[..1], 0), None);
    }
}
