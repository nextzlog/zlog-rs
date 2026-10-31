use zlog_core::qso::QslState;
use zlog_core::{Band, DelphiTime, Mode, Power, Qso};
use zlog_link::protocol::{self, Message};
use zlog_link::wire::{
    decode, encode, fill_left, fill_right, join_delimited, qso_to_text, split_delimited,
    text_to_qso,
};

fn full() -> Qso {
    Qso {
        time: DelphiTime(46316.375),
        callsign: "JA1ZLO/1".into(),
        nr_sent: "10M".into(),
        nr_rcvd: "1101H".into(),
        rst_sent: 59,
        rst_rcvd: 579,
        serial: 1234,
        mode: Mode::Ssb,
        band: Band::B430,
        power: Power::P005,
        multi1: "1101".into(),
        multi2: "JA".into(),
        new_multi1: true,
        new_multi2: true,
        points: 2,
        operator: "山田".into(),
        memo: "東京都 千代田区".into(),
        cq: true,
        dupe: true,
        reserve: 3,
        tx: 2,
        power2: 500,
        reserve2: 2_001_230_000,
        reserve3: 2_001_230_007,
        freq: "433020.0".into(),
        qsy_violation: true,
        pc_name: "ＰＣ一".into(),
        forced: true,
        qsl_state: QslState::NoQsl,
        invalid: true,
        ..Qso::default()
    }
}

fn round_trip(q: &Qso) {
    assert_eq!(text_to_qso(&qso_to_text(q)).as_ref(), Some(q));
    let bytes = encode(&qso_to_text(q));
    assert_eq!(text_to_qso(&decode(&bytes)).as_ref(), Some(q));
}

#[test]
fn every_field_round_trips() {
    round_trip(&full());
    round_trip(&Qso::default());
}

#[test]
fn shift_jis_memo_and_operator_round_trip() {
    let mut q = full();
    q.memo = "ソ表能～ｶﾅ".into();
    q.operator = "髙橋".into();
    let line = protocol::put_qso(&q);
    let bytes = encode(&line);
    assert!(bytes.windows(2).any(|w| w == [0x83, 0x5C]));
    let back = decode(&bytes);
    let Some(Message::PutQso(r)) = Message::parse(&back) else {
        panic!("{back}")
    };
    assert_eq!(r.operator, "髙橋");
    assert_eq!(r.memo, q.memo);
    assert_eq!(r, q);
}

#[test]
fn characters_without_shift_jis_become_question_marks() {
    let mut q = full();
    q.memo = "café 😀".into();
    let r = text_to_qso(&decode(&encode(&qso_to_text(&q)))).unwrap();
    assert_eq!(r.memo, "caf? ?");
}

#[test]
fn quoting_edge_cases() {
    for memo in [
        "~",
        "~~",
        "\"",
        "\"\"",
        "\"quoted\"",
        "a\"b",
        "a~\"b\"~c",
        " leading",
        "trailing ",
        "",
        "\"~",
    ] {
        let mut q = full();
        q.memo = memo.into();
        q.operator = memo.into();
        round_trip(&q);
    }
    assert_eq!(join_delimited(&["a\0".into(), "b".into()]), "\"a\0\"~b");
    let mut q = full();
    q.memo = "a\0b".into();
    assert_eq!(text_to_qso(&qso_to_text(&q)).unwrap().memo, "ab");
}

#[test]
fn delimited_text_like_delphi() {
    assert_eq!(split_delimited(""), Vec::<String>::new());
    assert_eq!(split_delimited("~"), vec!["", ""]);
    assert_eq!(split_delimited("a"), vec!["a"]);
    assert_eq!(split_delimited("\"\""), vec![""]);
    assert_eq!(split_delimited("\"a\"\"b\"~c"), vec!["a\"b", "c"]);
    assert_eq!(split_delimited("\"a~b"), vec!["a~b"]);
    assert_eq!(split_delimited("\"a\"b~c"), vec!["a", "b", "c"]);
    assert_eq!(join_delimited(&["".into()]), "\"\"");
    assert_eq!(join_delimited(&["".into(), "".into()]), "~");
    assert_eq!(join_delimited(&["x\"y".into()]), "\"x\"\"y\"");
}

#[test]
fn malformed_qso_texts() {
    assert_eq!(text_to_qso(""), None);
    assert_eq!(text_to_qso("ZLOGQSODATA"), None);
    assert_eq!(text_to_qso("zlogqsodata:~1"), None);
    let q = text_to_qso("ZLOGQSODATA:").unwrap();
    assert_eq!(q.memo, "Convert Error!");
    assert_eq!(q.callsign, "");
    let mut good = full();
    good.memo = "kept?".into();
    let t = qso_to_text(&good).replace("~2001230007~", "~x~");
    let q = text_to_qso(&t).unwrap();
    assert_eq!(q.callsign, good.callsign);
    assert_eq!(q.reserve2, good.reserve2);
    assert_eq!(q.memo, "Convert Error!");
    let q = text_to_qso("ZLOGQSODATA:~46137,5~JA1AAA~~~ 599").unwrap();
    assert_eq!(q.time, DelphiTime(46137.5));
    assert_eq!(q.rst_sent, 599);
    let q = text_to_qso("ZLOGQSODATA:~1~A~~~0~0~0~99~99~99").unwrap();
    assert_eq!((q.mode, q.band), (Mode::Other, Band::Unknown));
    let t = format!("{}~extra~more", qso_to_text(&full()));
    assert_eq!(text_to_qso(&t).unwrap(), full());
}

#[test]
fn every_command_is_parsed() {
    let q = full();
    let t = qso_to_text(&q);
    let cases = [
        ("PUTQSO", Message::PutQso(q.clone())),
        ("DELQSO", Message::DelQso(q.clone())),
        ("EXDELQSO", Message::DelQso(q.clone())),
        ("INSQSO", Message::InsQso(q.clone())),
        ("INSQSOAT", Message::InsQsoAt(q.clone())),
        ("LOCKQSO", Message::LockQso(q.clone())),
        ("UNLOCKQSO", Message::UnlockQso(q.clone())),
        ("EDITQSOTO", Message::EditQsoTo(q.clone())),
        ("PUTLOGEX", Message::PutLogEx(q.clone())),
        ("PUTLOG", Message::PutLog(q.clone())),
    ];
    for (name, m) in cases {
        assert_eq!(
            Message::parse(&format!("#ZLOG# {name} {t}")),
            Some(m),
            "{name}"
        );
    }
    let plain = [
        ("#ZLOG# BEGINMERGE-OK", Message::BeginMergeOk),
        ("#ZLOG# BEGINMERGE-NG", Message::BeginMergeNg),
        ("#ZLOG# ENDQSOIDS", Message::EndQsoIds),
        ("#ZLOG# RENEW", Message::Renew),
        ("#ZLOG# RENEW ", Message::Renew),
        ("#ZLOG# SENDLOG", Message::SendLog),
        ("#ZLOG# QSOIDS ", Message::QsoIds(vec![])),
        ("#ZLOG# QSOIDS 1 x 2", Message::QsoIds(vec![1, 2])),
        ("#ZLOG# FREQ 2  7  [PC]", Message::Freq("2  7  [PC]".into())),
        ("#ZLOG# FREQ", Message::Freq("".into())),
        (
            "#ZLOG# PUTMESSAGE hi",
            Message::PutMessage {
                text: "hi".into(),
                red: false,
            },
        ),
        (
            "#ZLOG# PUTMESSAGE ",
            Message::PutMessage {
                text: "".into(),
                red: false,
            },
        ),
        ("#ZLOG# BSDATA 1", Message::Other("BSDATA 1".into())),
        ("#ZLOG# ", Message::Other("".into())),
        ("#ZLOG#", Message::Other("".into())),
        ("#ZLOG# renew", Message::Other("renew".into())),
    ];
    for (line, m) in plain {
        assert_eq!(Message::parse(line), Some(m), "{line:?}");
    }
    assert_eq!(Message::parse(""), None);
    assert_eq!(Message::parse("#ZLOG"), None);
    assert_eq!(
        Message::parse("#ZLOG# PUTQSO junk"),
        None,
        "malformed is dropped"
    );
    assert_eq!(
        Message::parse("#ZLOG# EDITQSOTO ZLOGQSODATA:~x~JA1AAA"),
        None
    );
}

#[test]
fn every_command_is_built() {
    let q = full();
    let t = qso_to_text(&q);
    assert_eq!(protocol::put_qso(&q), format!("#ZLOG# PUTQSO {t}"));
    assert_eq!(protocol::put_log(&q), format!("#ZLOG# PUTLOG {t}"));
    assert_eq!(protocol::edit_qso_to(&q), format!("#ZLOG# EDITQSOTO {t}"));
    assert_eq!(protocol::del_qso(&q), format!("#ZLOG# DELQSO {t}"));
    assert_eq!(protocol::renew(), "#ZLOG# RENEW ");
    assert_eq!(protocol::renew_after_log(), "#ZLOG# RENEW");
    assert_eq!(protocol::band(Band::B1_9), "#ZLOG# BAND 0");
    assert_eq!(protocol::band(Band::B430), "#ZLOG# BAND 11");
    assert_eq!(protocol::operator(""), "#ZLOG# OPERATOR ");
    assert_eq!(protocol::pc_name("PC1"), "#ZLOG# PCNAME PC1");
    assert_eq!(protocol::begin_merge("\n"), "#ZLOG# BEGINMERGE\n");
    assert_eq!(protocol::get_qso_ids(), "#ZLOG# GETQSOIDS");
    assert_eq!(protocol::send_renew(), "#ZLOG# SENDRENEW");
    assert_eq!(protocol::end_merge(), "#ZLOG# ENDMERGE");
    assert_eq!(protocol::send_log(), "#ZLOG# SENDLOG");
    assert!(protocol::get_log_qso_id(&[]).is_empty());
    let ids: Vec<i32> = (1..=40).collect();
    let lines = protocol::get_log_qso_id(&ids);
    assert_eq!(lines.len(), 2);
    assert!(lines[1].ends_with(" 40 "));
    for (line, m) in [
        (protocol::put_qso(&q), Message::PutQso(q.clone())),
        (protocol::put_log(&q), Message::PutLog(q.clone())),
        (protocol::edit_qso_to(&q), Message::EditQsoTo(q.clone())),
        (protocol::del_qso(&q), Message::DelQso(q.clone())),
    ] {
        assert_eq!(Message::parse(&decode(&encode(&line))), Some(m));
    }
}

#[test]
fn freq_lines() {
    let l = protocol::freq(
        Band::B430,
        Band::B430,
        false,
        433_020_000,
        Mode::Fm,
        false,
        "09:00:00",
        "PC",
    );
    assert_eq!(l, "#ZLOG# FREQ 11 430    433020.0 FM   SP 09:00:00  [PC]");
    let l = protocol::freq(
        Band::B7,
        Band::B7,
        true,
        7_001_050,
        Mode::Cw,
        true,
        "09:00:00",
        "M",
    );
    assert_eq!(l, "#ZLOG# FREQ 30 7        7001.1 CW   CQ 09:00:00  [M]");
    assert_eq!(protocol::freq_pc_name("x [a] [b]"), "a");
    assert_eq!(protocol::freq_pc_name("no brackets"), "");
    assert_eq!(protocol::freq_pc_name("[open"), "");
}

#[test]
fn chat_edge_cases() {
    assert_eq!(
        protocol::chat("12:00", "x", "!"),
        Some((
            "12:00        x> ".into(),
            "#ZLOG# PUTMESSAGE !12:00        x> ".into()
        ))
    );
    assert_eq!(
        protocol::chat("12:00", "x", "¥RENEW").unwrap().1,
        "#ZLOG# RENEW"
    );
    assert_eq!(
        protocol::chat("12:00", "ABCDEFGHIJ", "a").unwrap().0,
        "12:00 CDEFGHIJ> a"
    );
    assert_eq!(fill_left("", 3), "   ");
    assert_eq!(fill_right("", 0), "");
    assert_eq!(fill_right("東京", 3), "東\u{FFFD}");
}
