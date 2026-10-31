use zlog_cfg::file::decode;
use zlog_cfg::Config;
use zlog_core::{Band, Mode};

fn pt(c: &Config, band: Band, mode: Mode) -> i32 {
    Config::at(&c.points, band, mode)
}

#[test]
fn commands_need_a_separator_like_zlog() {
    let c = Config::parse("#T\nCTY\nCTYENTITYONLY ON\n");
    assert!(!c.use_cty);
    assert!(c.cty_entity_only);
    assert!(Config::parse("cty ;").use_cty);
    assert!(Config::parse("cty\t;\tcountry type").use_cty);
}

#[test]
fn comments_names_and_exit() {
    let c = Config::parse(
        "; JA contest\n#ALL JA1 Contest ; not a comment\n  ;indented comment\nPROV 10 ; tokyo\nEXIT\nPROV 11\n",
    );
    assert_eq!(c.contest_name, "ALL JA1 Contest ; not a comment");
    assert_eq!(c.prov, "11");
    assert_eq!(Config::parse("PROV 10\nexit ;\nPROV 11\n").prov, "10");
}

#[test]
fn trim_and_upper_case_are_ascii_only() {
    let c = Config::parse("\u{3000}PROV\t\u{3000}ab\u{3000}\nCITY\tstraße\nCONTESTID\tｊａ\n");
    assert_eq!(c.prov, "");
    assert!(c.ignored.iter().any(|i| i == "\u{3000}PROV"));
    assert_eq!(c.city, "STRAßE");
    assert_eq!(c.contest_id, "ｊａ");
    let c = Config::parse("\x01\x1fPROV\x0b\x0c 10 \x07\r\n");
    assert_eq!(c.prov, "10");
}

#[test]
fn defaultpt_items_follow_str_to_int_def() {
    let c = Config::parse("DEFAULTPT 5, 3,2 ,$0A,+4,x1,-1\n");
    assert_eq!(pt(&c, Band::B1_9, Mode::Cw), 5);
    assert_eq!(pt(&c, Band::B1_9, Mode::Ssb), 3);
    assert_eq!(pt(&c, Band::B3_5, Mode::Cw), 0);
    assert_eq!(pt(&c, Band::B3_5, Mode::Ssb), 10);
    assert_eq!(pt(&c, Band::B7, Mode::Cw), 4);
    assert_eq!(pt(&c, Band::B7, Mode::Fm), 1);
    assert_eq!(pt(&c, Band::B14, Mode::Cw), -1);
}

#[test]
fn line_endings_and_tabs() {
    let c = Config::parse("PROV 10\rCITY 1001\r\nPOWER\tHHH\nCOEFF ON");
    assert_eq!((c.prov.as_str(), c.city.as_str()), ("10", "1001"));
    assert!(c.power.starts_with("HHH-"));
    assert!(c.coeff && c.use_coeff);
}

#[test]
fn coeff_scan_reads_the_whole_file_as_zlog() {
    let c = Config::parse("EXIT\ncoeff on\n");
    assert!(c.coeff);
    assert!(c.use_coeff);
    let c = Config::parse(" COEFF ON\n");
    assert!(!c.coeff);
    let c = Config::parse("COEFF OFF ; CONTEST\n");
    assert!(c.coeff);
    assert!(!c.use_coeff);
}

#[test]
fn comma_lists_split_on_tabs_and_quotes_end_items() {
    let c = Config::parse("LOCAL 99\tX\nLOCAL\t10\t11,\"12 3\"45, 13\n");
    let keys: Vec<_> = c.local.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["10", "11", "12 3", "45", "13"]);
    let c = Config::parse("EXPT7\t2,\t3,4\n");
    assert_eq!(pt(&c, Band::B7, Mode::Ssb), 3);
    assert_eq!(pt(&c, Band::B7, Mode::Fm), 4);
    assert_eq!(pt(&c, Band::B7, Mode::Am), 1);
}

#[test]
fn shift_jis_utf8_bom_and_utf16_files() {
    let text = "#全市全郡\r\nPROV 10\r\n";
    let sjis = encoding_rs::SHIFT_JIS.encode(text).0.into_owned();
    let mut bom = b"\xEF\xBB\xBF".to_vec();
    bom.extend(text.as_bytes());
    let mut le = vec![0xFF, 0xFE];
    le.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    for bytes in [sjis, bom, le, text.as_bytes().to_vec()] {
        let c = Config::parse(&decode(&bytes));
        assert_eq!(c.contest_name, "全市全郡");
        assert_eq!(c.prov, "10");
    }
}
