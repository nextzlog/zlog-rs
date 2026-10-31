use zlog_core::{Band, Qso};

use crate::wire::{complete_qso, fill_left, fill_right, parse_qso, qso_to_text};

pub const HEADER: &str = "#ZLOG#";

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Freq(String),
    BeginMergeOk,
    BeginMergeNg,
    QsoIds(Vec<i32>),
    EndQsoIds,
    PutMessage { text: String, red: bool },
    PutQso(Qso),
    DelQso(Qso),
    InsQso(Qso),
    InsQsoAt(Qso),
    LockQso(Qso),
    UnlockQso(Qso),
    EditQsoTo(Qso),
    PutLogEx(Qso),
    PutLog(Qso),
    Renew,
    SendLog,
    Other(String),
}

fn after<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)
}

impl Message {
    pub fn parse(line: &str) -> Option<Message> {
        let at = line.find(HEADER)?;
        let mut rest = line[at + HEADER.len()..].chars();
        rest.next();
        let t = rest.as_str();
        let parsed = complete_qso;
        if t.split_once(' ').is_some_and(|(v, a)| {
            matches!(
                v,
                "PUTQSO"
                    | "PUTLOG"
                    | "PUTLOGEX"
                    | "EDITQSOTO"
                    | "DELQSO"
                    | "EXDELQSO"
                    | "INSQSO"
                    | "INSQSOAT"
                    | "LOCKQSO"
                    | "UNLOCKQSO"
            ) && parsed(a).is_none()
        }) {
            return None;
        }
        let qso = |s: &str| parsed(s).unwrap_or_default();
        let m = if let Some(s) = after(t, "FREQ") {
            Message::Freq(s.get(1..).unwrap_or("").to_string())
        } else if t.starts_with("BEGINMERGE-OK") {
            Message::BeginMergeOk
        } else if t.starts_with("BEGINMERGE-NG") {
            Message::BeginMergeNg
        } else if let Some(s) = after(t, "QSOIDS") {
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

pub const MAX_ARGUMENT: usize = 255;

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Freq(String),
    Band(String),
    Operator(String),
    PcName(String),
    Chat(String),
    PutQso(Qso),
    PutLog(Qso),
    EditQsoTo(Qso),
    DelQso(Qso),
    InsQso(Qso),
    Renew,
    SendRenew,
    SendLog,
    GetQsoIds,
    GetLogQsoId(Vec<i32>),
    BeginMerge,
    EndMerge,
    ExGetQsoIds(Vec<i32>),
    ExSendLog(Vec<i32>),
    NewBeginMerge,
    CheckQsoIds(Vec<i32>),
    GetUnregQsos,
    Who,
    GetConsole,
    Ignored,
    Relay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    Empty,
    Header,
    Long,
    Control,
    Malformed,
    File,
}

fn ids(s: &str) -> Result<Vec<i32>, Refused> {
    s.split_whitespace()
        .map(|n| n.parse().map_err(|_| Refused::Malformed))
        .collect()
}

pub fn clip(line: &str) -> Option<String> {
    let rest = line.strip_prefix(HEADER)?.strip_prefix(' ')?;
    let (verb, arg) = rest.split_once(' ')?;
    if arg.chars().count() <= MAX_ARGUMENT {
        return None;
    }
    let arg: String = arg.chars().take(MAX_ARGUMENT).collect();
    Some(format!("{HEADER} {verb} {arg}"))
}

impl Request {
    pub fn parse(line: &str) -> Result<Request, Refused> {
        if line.trim().is_empty() {
            return Err(Refused::Empty);
        }
        if line.chars().any(|c| c.is_control() && c != '\t') {
            return Err(Refused::Control);
        }
        let rest = line.strip_prefix(HEADER).ok_or(Refused::Header)?;
        let rest = match rest.strip_prefix(' ') {
            Some(r) => r,
            None if rest.is_empty() => return Err(Refused::Empty),
            None => return Err(Refused::Header),
        };
        if rest.trim().is_empty() {
            return Err(Refused::Empty);
        }
        let (verb, arg) = rest.split_once(' ').unwrap_or((rest, ""));
        if arg.chars().count() > MAX_ARGUMENT {
            return Err(Refused::Long);
        }
        let qso = |s: &str| parse_qso(s).ok_or(Refused::Malformed);
        Ok(match verb {
            "FREQ" => Request::Freq(arg.into()),
            "BAND" => Request::Band(arg.trim().into()),
            "OPERATOR" => Request::Operator(arg.into()),
            "PCNAME" => Request::PcName(arg.into()),
            "PUTMESSAGE" => Request::Chat(arg.into()),
            "PUTQSO" => Request::PutQso(qso(arg)?),
            "PUTLOG" => Request::PutLog(qso(arg)?),
            "EDITQSOTO" => Request::EditQsoTo(qso(arg)?),
            "DELQSO" | "EXDELQSO" => Request::DelQso(qso(arg)?),
            "INSQSO" => Request::InsQso(qso(arg)?),
            "LOCKQSO" | "UNLOCKQSO" => {
                qso(arg)?;
                Request::Relay
            }
            "RENEW" => Request::Renew,
            "SENDRENEW" => Request::SendRenew,
            "SENDLOG" => Request::SendLog,
            "GETQSOIDS" => Request::GetQsoIds,
            "GETLOGQSOID" => Request::GetLogQsoId(ids(arg)?),
            "BEGINMERGE" => Request::BeginMerge,
            "ENDMERGE" => Request::EndMerge,
            "EXGETQSOIDS" => Request::ExGetQsoIds(ids(arg)?),
            "EXSENDLOG" => Request::ExSendLog(ids(arg)?),
            "NEWBEGINMERGE" => Request::NewBeginMerge,
            "CHECKQSOIDS" => Request::CheckQsoIds(ids(arg)?),
            "GETUNREGQSOS" => Request::GetUnregQsos,
            "WHO" => Request::Who,
            "GETCONSOLE" => Request::GetConsole,
            "GETFILE" | "PUTFILE" | "FILEDATA" | "PUTFILE-OK" => return Err(Refused::File),
            "EDITQSOFROM" | "INSQSOAT" | "RESET" | "ENDLOG" | "SENDCURRENT" => Request::Ignored,
            v if v.eq_ignore_ascii_case("WHO") => Request::Who,
            _ => Request::Relay,
        })
    }

    pub fn reads_log(&self) -> bool {
        matches!(
            self,
            Request::SendLog
                | Request::GetQsoIds
                | Request::GetLogQsoId(_)
                | Request::ExGetQsoIds(_)
                | Request::ExSendLog(_)
                | Request::CheckQsoIds(_)
                | Request::GetUnregQsos
        )
    }
}

pub fn qso_ids(ids: &[i32]) -> Vec<String> {
    let mut lines: Vec<String> = ids
        .chunks(20)
        .map(|c| {
            let list: String = c.iter().map(|id| format!("{id} ")).collect();
            format!("{HEADER} QSOIDS {list}")
        })
        .collect();
    lines.push(format!("{HEADER} ENDQSOIDS"));
    lines
}

pub fn check_qso_ids(ids: &[i32]) -> String {
    let list: String = ids.iter().map(|id| format!("{id} ")).collect();
    format!("{HEADER} CHECKQSOIDS {list}")
}

pub fn put_log_ex(q: &Qso) -> String {
    line("PUTLOGEX", &qso_to_text(q))
}

pub fn begin_merge_ok() -> String {
    format!("{HEADER} BEGINMERGE-OK")
}

pub fn begin_merge_ng() -> String {
    format!("{HEADER} BEGINMERGE-NG")
}

pub fn new_end_merge() -> String {
    format!("{HEADER} NEWENDMERGE")
}

pub fn notice(text: &str) -> String {
    line("PUTMESSAGE", text)
}

pub fn band_string(b: Band) -> String {
    match b {
        Band::B10G => "10 GHz & up".into(),
        Band::Unknown | Band::Target => b.label().into(),
        b => format!("{} MHz", b.label()),
    }
}

fn line(command: &str, argument: &str) -> String {
    format!("{HEADER} {command} {argument}")
}

pub fn put_qso(q: &Qso) -> String {
    line("PUTQSO", &qso_to_text(q))
}

pub fn put_log(q: &Qso) -> String {
    line("PUTLOG", &qso_to_text(q))
}

pub fn edit_qso_to(q: &Qso) -> String {
    line("EDITQSOTO", &qso_to_text(q))
}

pub fn del_qso(q: &Qso) -> String {
    line("DELQSO", &qso_to_text(q))
}

pub fn renew() -> String {
    format!("{HEADER} RENEW ")
}

pub fn renew_after_log() -> String {
    format!("{HEADER} RENEW")
}

pub fn band(b: Band) -> String {
    line("BAND", &(b as u8).to_string())
}

pub fn operator(op: &str) -> String {
    line("OPERATOR", op)
}

pub fn pc_name(name: &str) -> String {
    line("PCNAME", name)
}

pub fn begin_merge(line_break: &str) -> String {
    format!("{HEADER} BEGINMERGE{line_break}")
}

pub fn get_qso_ids() -> String {
    format!("{HEADER} GETQSOIDS")
}

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

pub fn send_log() -> String {
    format!("{HEADER} SENDLOG")
}

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

pub fn freq_pc_name(summary: &str) -> &str {
    match (summary.find('['), summary.find(']')) {
        (Some(i), Some(j)) if j > i => &summary[i + 1..j],
        _ => "",
    }
}

pub fn lock_qso(q: &Qso) -> String {
    line("LOCKQSO", &qso_to_text(q))
}

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
        assert_eq!(operator("JA1ZLO"), "#ZLOG# OPERATOR JA1ZLO");
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
