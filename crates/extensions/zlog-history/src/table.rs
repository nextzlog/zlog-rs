#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Row {
    pub callsign: String,
    pub exchange: String,
    pub name: String,
    pub extra: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    NoHeader,
    NoCallColumn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Column {
    Call,
    Exchange,
    Name,
    Other(String),
}

impl Column {
    fn of(name: &str) -> Column {
        let key: String = name
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_ascii_uppercase();
        match key.as_str() {
            "CALL" | "CALLSIGN" => Column::Call,
            "EXCH" | "NR" | "NUMBER" | "RCVD" => Column::Exchange,
            "NAME" => Column::Name,
            k if k.len() == 5 && k.starts_with("EXCH") && k.as_bytes()[4].is_ascii_digit() => {
                Column::Exchange
            }
            _ => Column::Other(name.trim().to_string()),
        }
    }
}

pub fn decode(bytes: &[u8]) -> String {
    if let Some((enc, _)) = encoding_rs::Encoding::for_bom(bytes) {
        if enc != encoding_rs::UTF_8 {
            return enc.decode(bytes).0.into_owned();
        }
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => {
            let (text, _, bad) = encoding_rs::SHIFT_JIS.decode(bytes);
            if bad {
                String::from_utf8_lossy(bytes).into_owned()
            } else {
                text.into_owned()
            }
        }
    }
}

fn is_comment(line: &str) -> bool {
    let t = line.trim_start();
    t.is_empty() || t.starts_with('#') || t.starts_with(';') || t.starts_with("//")
}

pub fn split(line: &str) -> Vec<String> {
    split_at(line, ',')
}

fn split_at(line: &str, delimiter: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                value.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if value.trim().is_empty() => {
                value.clear();
                quoted = true;
            }
            c if c == delimiter && !quoted => {
                out.push(std::mem::take(&mut value).trim().to_string())
            }
            _ => value.push(c),
        }
    }
    out.push(value.trim().to_string());
    out
}

fn is_report(value: &str) -> bool {
    matches!(value, "59" | "599" | "5NN")
}

pub fn parse(text: &str) -> Result<Vec<Row>, TableError> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = text.lines().filter(|l| !is_comment(l));
    let first = lines.next().ok_or(TableError::NoHeader)?;
    let delimiter = [',', '\t', ';']
        .into_iter()
        .find(|d| first.contains(*d))
        .unwrap_or(',');
    let header: Vec<Column> = split_at(first, delimiter)
        .iter()
        .map(|n| Column::of(n))
        .collect();
    if !header.contains(&Column::Call) {
        return Err(TableError::NoCallColumn);
    }
    let exchanges = header.iter().filter(|c| **c == Column::Exchange).count();
    let mut rows = Vec::new();
    for line in lines {
        let mut row = Row::default();
        let mut exchange = Vec::new();
        for (column, value) in header.iter().zip(split_at(line, delimiter)) {
            if value.is_empty() {
                continue;
            }
            match column {
                Column::Call => row.callsign = value.to_ascii_uppercase(),
                Column::Exchange if exchanges > 1 && is_report(&value.to_ascii_uppercase()) => {}
                Column::Exchange => exchange.push(value.to_ascii_uppercase()),
                Column::Name => row.name = value,
                Column::Other(name) => row.extra.push((name.clone(), value)),
            }
        }
        row.exchange = exchange.join(" ");
        if !row.callsign.is_empty() {
            rows.push(row);
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_header_comments_quotes_and_extra_columns() {
        let text = "\u{FEFF}# comment\n\n!!Order!!,Call,Name,Exch1,UserText\n\
                    1,ja1zlo,Taro,10m,club\n2,JA1YWX,, \"01H\" ,\n3,,Nobody,13M\n";
        let rows = parse(&decode(text.as_bytes())).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].callsign, "JA1ZLO");
        assert_eq!(rows[0].exchange, "10M");
        assert_eq!(rows[0].name, "Taro");
        assert_eq!(
            rows[0].extra,
            [
                ("!!Order!!".to_string(), "1".to_string()),
                ("UserText".to_string(), "club".to_string())
            ]
        );
        assert_eq!(rows[1].exchange, "01H");
        assert!(rows[1].name.is_empty());
    }

    #[test]
    fn several_exchange_columns_are_joined() {
        let rows = parse("CALL,EXCH1,EXCH2\nJA1ZLO,59,10M\nJG1VPP,,13M\nJA1YWX,10,H\n").unwrap();
        assert_eq!(rows[0].exchange, "10M", "the report is no number");
        assert_eq!(rows[2].exchange, "10 H");
        assert_eq!(rows[1].exchange, "13M");
    }

    #[test]
    fn quoted_commas_and_quotes() {
        assert_eq!(split(r#"a,"b,c","d""e""#), ["a", "b,c", "d\"e"]);
        assert_eq!(split(""), [""]);
        assert_eq!(split(r#"5",x"#), ["5\"", "x"]);
    }

    #[test]
    fn tabs_semicolons_and_lone_cr() {
        let rows = parse("CALL\tNAME\rJA1ZLO\tTaro\rJG1VPP\tJiro").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].name, "Jiro");
        assert_eq!(parse("CALL;EXCH\nJA1ZLO;10M").unwrap()[0].exchange, "10M");
    }

    #[test]
    fn utf16_and_a_stray_byte_in_utf8() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "CALL,NAME\nJA1ZLO,太郎\n".encode_utf16() {
            bytes.extend(u.to_le_bytes());
        }
        assert_eq!(parse(&decode(&bytes)).unwrap()[0].name, "太郎");
        let mut bytes = "CALL,NAME\nJA1ZLO,太郎\n".as_bytes().to_vec();
        bytes.push(0xFF);
        assert!(decode(&bytes).contains("太郎"));
    }

    #[test]
    fn files_without_header_or_callsign_column_are_refused() {
        assert_eq!(parse("# only\n\n"), Err(TableError::NoHeader));
        assert_eq!(parse("NAME,EXCH\nTaro,10M"), Err(TableError::NoCallColumn));
    }

    #[test]
    fn shift_jis_is_read() {
        let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("CALL,NAME\nJA1ZLO,太郎\n");
        assert_eq!(parse(&decode(&bytes)).unwrap()[0].name, "太郎");
    }

    #[test]
    fn only_ascii_letters_are_upper_cased_like_zlog() {
        let rows = parse("CALL,EXCH\nja1zlo,ｔｏｋｙｏ straße\n").unwrap();
        assert_eq!(rows[0].callsign, "JA1ZLO");
        assert_eq!(rows[0].exchange, "ｔｏｋｙｏ STRAßE");
    }
}
