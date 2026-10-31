use std::path::{Path, PathBuf};

use zlog_core::{ContestFile, OpenedContest};

use crate::user::UserDefinedDef;
use crate::{texts, Config};

pub fn decode(bytes: &[u8]) -> String {
    for bom in [encoding_rs::UTF_16LE, encoding_rs::UTF_16BE] {
        if let Some((enc, n)) = encoding_rs::Encoding::for_bom(bytes).filter(|(e, _)| *e == bom) {
            return enc.decode_without_bom_handling(&bytes[n..]).0.into_owned();
        }
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned(),
    }
}

pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = crate::cfg::delphi_trim(name).replace('\\', "/");
    let rel = Path::new(&name);
    let exact = dir.join(rel);
    if exact.is_file() {
        return Some(exact);
    }
    let folder = exact.parent()?.to_path_buf();
    let wanted = rel.file_name()?.to_str()?.to_lowercase();
    std::fs::read_dir(&folder)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_lowercase() == wanted)
        })
}

pub fn load_cfg(path: &Path) -> Result<(UserDefinedDef, Option<String>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let cfg = decode(&bytes);
    let dir = path.parent().unwrap_or(Path::new("."));
    let def = UserDefinedDef::from_cfg(&cfg, |name| {
        let p = find_file(dir, name)?;
        std::fs::read(p).ok().map(|b| decode(&b))
    });
    let dat = &def.config().dat;
    let missing = (!dat.is_empty() && !def.dat_loaded()).then(|| dat.clone());
    Ok((def, missing))
}

pub fn cfg_title(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(Config::parse(&decode(&bytes)).contest_name)
}

pub struct CfgFile;

impl ContestFile for CfgFile {
    fn name(&self, language: &str) -> String {
        texts().get(language, "file.cfg").into()
    }

    fn extensions(&self) -> &[&str] {
        &["cfg"]
    }

    fn title(&self, path: &Path) -> Option<String> {
        cfg_title(path)
    }

    fn open(&self, path: &Path, language: &str) -> Result<OpenedContest, String> {
        let (def, missing) = load_cfg(path)?;
        let script = def
            .script_error()
            .map(|e| texts().format(language, "file.script_failed", &[&e]));
        let warnings = missing
            .map(|d| texts().format(language, "file.dat_missing", &[&d]))
            .into_iter()
            .chain(script)
            .collect();
        Ok(OpenedContest {
            def: Box::new(def),
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zlog_core::Registry;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("zlog-rule-ja-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sjis(s: &str) -> Vec<u8> {
        encoding_rs::SHIFT_JIS.encode(s).0.into_owned()
    }

    #[test]
    fn decodes_utf8_bom_and_shift_jis() {
        assert_eq!(decode("#テスト".as_bytes()), "#テスト");
        assert_eq!(decode(b"\xEF\xBB\xBF#A"), "#A");
        assert_eq!(decode(&sjis("#テスト")), "#テスト");
        let mut le = vec![0xFF, 0xFE];
        le.extend("#テスト".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(decode(&le), "#テスト");
        let mut be = vec![0xFE, 0xFF];
        be.extend("#テスト".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode(&be), "#テスト");
    }

    #[test]
    fn loads_shift_jis_cfg_and_dat_with_other_case() {
        let dir = scratch("cfg");
        std::fs::write(
            dir.join("Test.CFG"),
            sjis("#テストコンテスト\ndat TEST.dat\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("test.DAT"),
            sjis("見出し\n01 北海道\n02 青森\nend of file\n"),
        )
        .unwrap();
        let path = dir.join("Test.CFG");
        assert_eq!(cfg_title(&path).as_deref(), Some("テストコンテスト"));
        let (def, missing) = load_cfg(&path).unwrap();
        assert_eq!(missing, None);
        assert!(def.dat_loaded());
        assert_eq!(find_file(&dir, "TEST.dat"), Some(dir.join("test.DAT")));

        let r = Registry::new().with(crate::Cfg);
        assert_eq!(r.contest_files().len(), 1);
        let opened = r.open_contest_file(&path, "en").unwrap();
        assert!(opened.warnings.is_empty());
        assert_eq!(
            zlog_core::ContestDef::caption(&*opened.def, "en"),
            "テストコンテスト"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_dat_is_a_warning() {
        let dir = scratch("nodat");
        let path = dir.join("x.cfg");
        std::fs::write(&path, "#X\ndat none.dat\n").unwrap();
        let opened = CfgFile.open(&path, "en").unwrap();
        assert_eq!(opened.warnings.len(), 1);
        assert!(opened.warnings[0].contains("none.dat"));
        assert!(CfgFile.open(&dir.join("gone.cfg"), "en").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
