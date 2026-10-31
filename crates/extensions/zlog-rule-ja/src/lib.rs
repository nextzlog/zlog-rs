// The zLog_cfgdat collection (github.com/jr8ppg/zLog_cfgdat) carries no licence,
// so none of its samples is embedded.

pub mod acag;
pub mod allja;
pub mod field_day;
pub mod ja0;
pub mod japan;
pub mod jarl;
pub mod nyp;
pub mod six_down;

pub use japan::JapanContest;
pub use jarl::JarlContest;

use zlog_core::{ContestDef, Extension};

zlog_core::texts!();

pub struct Japan;

impl Extension for Japan {
    fn name(&self) -> &str {
        "Japanese contests"
    }

    fn contests(&self) -> Vec<Box<dyn ContestDef>> {
        let mut all: Vec<Box<dyn ContestDef>> = JarlContest::ALL
            .into_iter()
            .map(|c| Box::new(c) as Box<dyn ContestDef>)
            .collect();
        all.extend(
            JapanContest::ALL
                .into_iter()
                .map(|c| Box::new(c) as Box<dyn ContestDef>),
        );
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn extension_lists_all_contests() {
        assert_eq!(Japan.contests().len(), 7);
    }

    #[test]
    fn sent_templates_match_zlog() {
        let zlog = [
            ("allja", "$V$P"),
            ("6d", "$Q$P"),
            ("fd", "$Q$P"),
            ("acag", "$Q$P"),
            ("ja0-in", "$S"),
            ("ja0-out", "$S"),
            ("nyp", "$H"),
        ];
        let station = zlog_core::Station::default();
        for def in Japan.contests() {
            let (_, want) = zlog.iter().find(|(id, _)| *id == def.id()).unwrap();
            let c = def.build(zlog_core::ContestMode::Mix);
            assert_eq!(
                c.sent_template(&station).as_deref(),
                Some(*want),
                "{}",
                def.id()
            );
        }
    }
}
