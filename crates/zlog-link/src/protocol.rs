//! Z-Link commands: lines `#ZLOG# <COMMAND> <argument>` (UZLinkForm.pas).
//!
//! [`Message::parse`] follows `TZLinkForm.ProcessCommand`; the functions below it build
//! the lines zLog sends, byte for byte (without the line break, which
//! [`crate::session::Session::write`] adds).

use zlog_core::{Band, Qso};

use crate::wire::{fill_left, fill_right, qso_to_text, text_to_qso};

/// `ZLinkHeader` (UzlogConst.pas).
pub const HEADER: &str = "#ZLOG#";

/// One command received from Z-Server.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// `FREQ`: a station's rig status (`TRigControl.StatusSummaryFreqHz`).
    Freq(String),
    BeginMergeOk,
    BeginMergeNg,
    /// `QSOIDS`: QSO IDs with edit counters held by the server.
    QsoIds(Vec<i32>),
    EndQsoIds,
    /// `PUTMESSAGE`: a chat line; `red` when it started with `!`.
    PutMessage {
        text: String,
        red: bool,
    },
    /// `PUTQSO`: a QSO just logged at another station.
    PutQso(Qso),
    /// `DELQSO` (then rescore) and `EXDELQSO` (one of a multiple delete).
    DelQso(Qso),
    /// `INSQSO`: insert before the QSO whose ID is in `reserve2`.
    InsQso(Qso),
    /// `INSQSOAT`: as `INSQSO`, but zLog only queues it (`Log.AddQue` without
    /// `ProcessQue`, `TZLinkForm.ProcessCommand`) until a later command processes the
    /// queue (RENEW, or DELQSO, EXDELQSO, INSQSO, LOCKQSO, UNLOCKQSO, EDITQSOTO).
    InsQsoAt(Qso),
    LockQso(Qso),
    UnlockQso(Qso),
    /// `EDITQSOTO`: replace the QSO with the same ID.
    EditQsoTo(Qso),
    /// `PUTLOGEX`: edit, or add if missing.
    PutLogEx(Qso),
    /// `PUTLOG`: add if missing (log download and merge).
    PutLog(Qso),
    Renew,
    /// `SENDLOG`: the server asks for the whole log.
    SendLog,
    /// Any other command (`SPOT`, `BSDATA`, `SENDSCRATCH`, `POSTWANTED`, `PUTFILE`, ...),
    /// which this client shows in its console only.
    Other(String),
}

/// The argument after a command name (zLog: `pos(name, temp) = 1`, then `copy`).
fn after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)
}

impl Message {
    /// The command in a received line, `None` without `#ZLOG#` (zLog writes such
    /// lines to its error log). As zLog, the header may appear anywhere in the line.
    ///
    /// zLog keeps only 255 characters of a line; this client keeps all of them.
    pub fn parse(line: &str) -> Option<Message> {
        let at = line.find(HEADER)?;
        // `copy(temp, length(ZLinkHeader) + 2, 255)`: the header and one blank.
        let mut rest = line[at + HEADER.len()..].chars();
        rest.next();
        let t = rest.as_str();
        let qso = |s: &str| text_to_qso(s).unwrap_or_default();
        // The order of zLog's tests matters only where one name begins another
        // (`PUTLOGEX ` before `PUTLOG `, `UNLOCKQSO` is not `LOCKQSO`).
        let m = if let Some(s) = after(t, "FREQ") {
            Message::Freq(s.get(1..).unwrap_or("").to_string())
        } else if t.starts_with("BEGINMERGE-OK") {
            Message::BeginMergeOk
        } else if t.starts_with("BEGINMERGE-NG") {
            Message::BeginMergeNg
        } else if let Some(s) = after(t, "QSOIDS") {
            // zLog takes a number only when a blank follows it; this client takes the
            // last one too.
            Message::QsoIds(
                s.split_whitespace()
                    .filter_map(|n| n.parse().ok())
                    .collect(),
            )
        } else if t.starts_with("ENDQSOIDS") {
            Message::EndQsoIds
        } else if let Some(s) = after(t, "PUTMESSAGE") {
            let s = s.get(1..).unwrap_or("");
            match s.strip_prefix('!') {
                Some(r) => Message::PutMessage {
                    text: r.into(),
                    red: true,
                },
                None => Message::PutMessage {
                    text: s.into(),
                    red: false,
                },
            }
        } else if let Some(s) = after(t, "PUTQSO ") {
            Message::PutQso(qso(s))
        } else if let Some(s) = after(t, "DELQSO ").or_else(|| after(t, "EXDELQSO ")) {
            Message::DelQso(qso(s))
        } else if let Some(s) = after(t, "INSQSOAT ") {
            Message::InsQsoAt(qso(s))
        } else if let Some(s) = after(t, "INSQSO ") {
            Message::InsQso(qso(s))
        } else if let Some(s) = after(t, "LOCKQSO") {
            Message::LockQso(qso(s.get(1..).unwrap_or("")))
        } else if let Some(s) = after(t, "UNLOCKQSO") {
            Message::UnlockQso(qso(s.get(1..).unwrap_or("")))
        } else if let Some(s) = after(t, "EDITQSOTO ") {
            Message::EditQsoTo(qso(s))
        } else if let Some(s) = after(t, "PUTLOGEX ") {
            Message::PutLogEx(qso(s))
        } else if let Some(s) = after(t, "PUTLOG ") {
            Message::PutLog(qso(s))
        } else if t.starts_with("RENEW") {
            Message::Renew
        } else if t.starts_with("SENDLOG") {
            Message::SendLog
        } else {
            Message::Other(t.to_string())
        };
        Some(m)
    }
}

fn line(command: &str, argument: &str) -> String {
    format!("{HEADER} {command} {argument}")
}

/// `SendQSO`: a QSO just logged here.
pub fn put_qso(q: &Qso) -> String {
    line("PUTQSO", &qso_to_text(q))
}

/// `SendQSO_PUTLOG`: a QSO of the whole log (`SENDLOG`, merge).
pub fn put_log(q: &Qso) -> String {
    line("PUTLOG", &qso_to_text(q))
}

/// `EditQSObyID`: a QSO edited here, with its edit counter advanced.
pub fn edit_qso_to(q: &Qso) -> String {
    line("EDITQSOTO", &qso_to_text(q))
}

/// `DeleteQSO`: a QSO deleted here.
pub fn del_qso(q: &Qso) -> String {
    line("DELQSO", &qso_to_text(q))
}

/// `Renew`: note the trailing blank, as zLog sends it.
pub fn renew() -> String {
    format!("{HEADER} RENEW ")
}

/// `RENEW` as sent after `SENDLOG` and during a merge (no trailing blank).
pub fn renew_after_log() -> String {
    format!("{HEADER} RENEW")
}

/// `SendBand`: the band of the entry row, by ordinal (`#ZLOG# BAND 3`).
pub fn band(b: Band) -> String {
    line("BAND", &(b as u8).to_string())
}

/// `SendOperator`.
pub fn operator(op: &str) -> String {
    line("OPERATOR", op)
}

/// `SendPcName`.
pub fn pc_name(name: &str) -> String {
    line("PCNAME", name)
}

/// `MergeLogWithZServer`: zLog appends a line break here and `WriteData` another.
pub fn begin_merge(line_break: &str) -> String {
    format!("{HEADER} BEGINMERGE{line_break}")
}

/// Answer to `BEGINMERGE-OK`.
pub fn get_qso_ids() -> String {
    format!("{HEADER} GETQSOIDS")
}

/// `SendMergeTempList`: requests for the QSOs only the server has, 20 IDs a line,
/// each followed by a blank.
pub fn get_log_qso_id(ids: &[i32]) -> Vec<String> {
    ids.chunks(20)
        .map(|c| {
            let list: String = c.iter().map(|id| format!("{id} ")).collect();
            format!("{HEADER} GETLOGQSOID {list}")
        })
        .collect()
}

pub fn send_renew() -> String {
    format!("{HEADER} SENDRENEW")
}

pub fn end_merge() -> String {
    format!("{HEADER} ENDMERGE")
}

/// `LoadLogFromZLink`: ask for the whole server log.
pub fn send_log() -> String {
    format!("{HEADER} SENDLOG")
}

/// `TChatForm.SendMessage`: the line shown here and the line sent.
///
/// The message is `hh:nn ` (local time), the prompt right-aligned in 10 bytes with
/// `> `, then the text. A text starting with `!` is shown in red at the other
/// stations; one starting with `\` (`¥` on Japanese Windows) is a raw command.
pub fn chat(time_hhnn: &str, prompt: &str, text: &str) -> Option<(String, String)> {
    if text.is_empty() {
        return None;
    }
    if let Some(raw) = text.strip_prefix(['\\', '¥']) {
        return Some((text.to_string(), format!("{HEADER} {raw}")));
    }
    let (body, red) = match text.strip_prefix('!') {
        Some(r) => (r, "!"),
        None => (text, ""),
    };
    let shown = format!(
        "{time_hhnn} {}{body}",
        fill_left(&format!("{prompt}> "), 10)
    );
    Some((shown.clone(), format!("{HEADER} PUTMESSAGE {red}{shown}")))
}

/// `TRigControl.StatusSummaryFreqHz` for a `FREQ` line: band ordinal of the entry
/// row (`Main.CurrentQSO.Band`; `30` for the multiplier station, `IsMultiStation`),
/// the rig's band, kHz, mode, CQ or SP, time, PC name.
#[allow(clippy::too_many_arguments)]
pub fn freq(
    entry_band: Band,
    rig_band: Band,
    multi: bool,
    hz: u64,
    mode: zlog_core::Mode,
    cq: bool,
    time_hhnnss: &str,
    pc: &str,
) -> String {
    let khz = format!("{:.1}", hz as f64 / 1000.0);
    let run = if cq { "CQ" } else { "SP" };
    let ordinal = if multi {
        "30".to_string()
    } else {
        (entry_band as u8).to_string()
    };
    let summary = format!(
        "{}{}{} {}{run} {time_hhnnss}  [{pc}]",
        fill_right(&ordinal, 3),
        fill_right(rig_band.label(), 5),
        fill_left(&khz, 10),
        fill_right(mode.label(), 5),
    );
    line("FREQ", &summary)
}

/// The PC name in a `FREQ` line (`TFreqList.GetPCName`): the text between the first
/// `[` and the first `]`, or empty when either is missing or `]` comes first.
pub fn freq_pc_name(summary: &str) -> &str {
    match (summary.find('['), summary.find(']')) {
        (Some(i), Some(j)) if j > i => &summary[i + 1..j],
        _ => "",
    }
}

/// `TZLinkForm.LockQSO`: a QSO is being edited here (`TEditDialog.Init`).
pub fn lock_qso(q: &Qso) -> String {
    line("LOCKQSO", &qso_to_text(q))
}

/// `TZLinkForm.UnLockQSO`: the edit of a QSO here has ended.
pub fn unlock_qso(q: &Qso) -> String {
    line("UNLOCKQSO", &qso_to_text(q))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Mode;

    #[test]
    fn parse_commands() {
        let q = Qso {
            callsign: "JA1ZLO".into(),
            reserve3: 1_000_123_401,
            ..Qso::default()
        };
        let t = qso_to_text(&q);
        assert_eq!(
            Message::parse(&format!("#ZLOG# PUTQSO {t}")),
            Some(Message::PutQso(q.clone()))
        );
        assert_eq!(
            Message::parse(&format!("#ZLOG# EXDELQSO {t}")),
            Some(Message::DelQso(q.clone()))
        );
        assert_eq!(
            Message::parse(&format!("#ZLOG# PUTLOGEX {t}")),
            Some(Message::PutLogEx(q.clone()))
        );
        assert_eq!(
            Message::parse(&format!("#ZLOG# UNLOCKQSO {t}")),
            Some(Message::UnlockQso(q))
        );
        assert_eq!(
            Message::parse("#ZLOG# QSOIDS 100012300 100012401 "),
            Some(Message::QsoIds(vec![100012300, 100012401]))
        );
        assert_eq!(
            Message::parse("junk #ZLOG# PUTMESSAGE !12:00     14> hi"),
            Some(Message::PutMessage {
                text: "12:00     14> hi".into(),
                red: true
            })
        );
        assert_eq!(Message::parse("#ZLOG# RENEW "), Some(Message::Renew));
        assert_eq!(
            Message::parse("#ZLOG# SPOT x"),
            Some(Message::Other("SPOT x".into()))
        );
        assert_eq!(Message::parse("Login:"), None);
    }

    #[test]
    fn built_lines() {
        assert_eq!(band(Band::B14), "#ZLOG# BAND 4");
        assert_eq!(operator("JR8PPG"), "#ZLOG# OPERATOR JR8PPG");
        assert_eq!(renew(), "#ZLOG# RENEW ");
        assert_eq!(begin_merge("\r\n"), "#ZLOG# BEGINMERGE\r\n");
        let ids: Vec<i32> = (1..=21).collect();
        let lines = get_log_qso_id(&ids);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" 19 20 "));
        assert_eq!(lines[1], "#ZLOG# GETLOGQSOID 21 ");
    }

    #[test]
    fn chat_lines() {
        assert_eq!(
            chat("12:34", "7", "hello"),
            Some((
                "12:34        7> hello".into(),
                "#ZLOG# PUTMESSAGE 12:34        7> hello".into()
            ))
        );
        assert_eq!(
            chat("12:34", "7", "!qrv?").unwrap().1,
            "#ZLOG# PUTMESSAGE !12:34        7> qrv?"
        );
        assert_eq!(chat("12:34", "7", "\\SENDLOG").unwrap().1, "#ZLOG# SENDLOG");
        assert_eq!(chat("12:34", "7", ""), None);
    }

    #[test]
    fn freq_line() {
        let l = freq(
            Band::B7,
            Band::B7,
            false,
            7_025_000,
            Mode::Cw,
            true,
            "12:00:00",
            "PC1",
        );
        assert_eq!(l, "#ZLOG# FREQ 2  7        7025.0 CW   CQ 12:00:00  [PC1]");
        let Some(Message::Freq(s)) = Message::parse(&l) else {
            panic!()
        };
        assert_eq!(freq_pc_name(&s), "PC1");
    }

    /// N3: the ordinal is the entry row's band, the label the rig's.
    #[test]
    fn freq_line_entry_and_rig_band() {
        let l = freq(
            Band::B7,
            Band::B14,
            false,
            14_025_000,
            Mode::Cw,
            false,
            "03:00:00",
            "PC1",
        );
        assert_eq!(l, "#ZLOG# FREQ 2  14      14025.0 CW   SP 03:00:00  [PC1]");
        let l = freq(
            Band::B7,
            Band::B14,
            true,
            14_025_000,
            Mode::Cw,
            false,
            "t",
            "",
        );
        assert!(l.starts_with("#ZLOG# FREQ 30 14 "));
        assert!(l.ends_with("[]"));
    }

    /// N7: `GetPCName` takes the first `[` up to the next `]`; nothing without both.
    #[test]
    fn pc_name_of_freq_lines() {
        assert_eq!(freq_pc_name("a [PC1] b [PC2]"), "PC1");
        assert_eq!(freq_pc_name("a [PC[1] b"), "PC[1");
        assert_eq!(freq_pc_name("no brackets"), "");
        assert_eq!(freq_pc_name(""), "");
        assert_eq!(freq_pc_name("open [only"), "");
        assert_eq!(freq_pc_name("close only]"), "");
        assert_eq!(freq_pc_name("] before [PC1]"), "");
        assert_eq!(freq_pc_name("[]"), "");
    }

    /// N8: lock lines carry the QSO as text, and parse back.
    #[test]
    fn lock_lines() {
        let q = Qso {
            callsign: "JA1ZLO".into(),
            reserve3: 1_000_123_401,
            ..Qso::default()
        };
        let l = lock_qso(&q);
        assert!(l.starts_with("#ZLOG# LOCKQSO "));
        assert_eq!(Message::parse(&l), Some(Message::LockQso(q.clone())));
        let l = unlock_qso(&q);
        assert!(l.starts_with("#ZLOG# UNLOCKQSO "));
        assert_eq!(Message::parse(&l), Some(Message::UnlockQso(q)));
    }
}
