use std::path::{Path, PathBuf};

use zlog_core::{Format, Log};

use crate::texts;

#[derive(Debug)]
pub enum BackupError {
    NoLog,
    SameFolder(PathBuf),
    CreateFolder(PathBuf, std::io::Error),
    Write(PathBuf, std::io::Error),
}

impl BackupError {
    pub fn message(&self, lang: &str) -> String {
        let t = texts();
        match self {
            BackupError::NoLog => t.get(lang, "no_log").into(),
            BackupError::SameFolder(d) => t.format(lang, "same_folder", &[&d.display()]),
            BackupError::CreateFolder(d, e) => t.format(lang, "create_failed", &[&d.display(), e]),
            BackupError::Write(d, e) => t.format(lang, "write_failed", &[&d.display(), e]),
        }
    }
}

pub fn zlog_folder() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
}

pub fn expand_variables(text: &str, root: &Path) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(len) = after.find('%') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = &after[..len];
        let tail = &after[len + 1..];
        if name.eq_ignore_ascii_case("ZLOG_ROOT") {
            out.push_str(&root.display().to_string());
            if !tail.is_empty() && !tail.starts_with(['/', '\\']) {
                out.push(std::path::MAIN_SEPARATOR);
            }
            rest = tail;
        } else if let Some(value) = std::env::var_os(name).filter(|_| !name.is_empty()) {
            out.push_str(&value.to_string_lossy());
            rest = tail;
        } else {
            out.push('%');
            out.push_str(name);
            rest = &after[len..];
        }
    }
    out.push_str(rest);
    out
}

pub fn folder_of(setting: &str, root: &Path) -> PathBuf {
    let folder = PathBuf::from(expand_variables(setting.trim(), root));
    match folder.is_relative() {
        true => root.join(folder),
        false => folder,
    }
}

pub fn backup_name(log_path: &Path) -> Option<String> {
    let name = log_path.file_name()?.to_string_lossy();
    Some(format!("{name}.BAK"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn format_of(log_path: &Path) -> Format {
    match log_path.extension() {
        Some(e) if e.eq_ignore_ascii_case("zlo") => Format::Zlo,
        _ => Format::Zlox,
    }
}

pub fn backup(
    log_path: &Path,
    setting: &str,
    root: &Path,
    log: &Log,
) -> Result<PathBuf, BackupError> {
    let name = backup_name(log_path).ok_or(BackupError::NoLog)?;
    let folder = folder_of(setting, root);
    std::fs::create_dir_all(&folder).map_err(|e| BackupError::CreateFolder(folder.clone(), e))?;
    let log_dir = match log_path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    if same_dir(&folder, log_dir) {
        return Err(BackupError::SameFolder(folder));
    }
    let bytes = log.to_bytes(format_of(log_path));
    let target = folder.join(&name);
    zlog_core::file::write_atomic(&target, &bytes).map_err(|e| BackupError::Write(folder, e))?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("zlog-backup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("log")).unwrap();
        dir
    }

    #[test]
    fn the_name_is_the_log_name_with_bak() {
        assert_eq!(
            backup_name(Path::new("/a/ALLJA1.zlox")).as_deref(),
            Some("ALLJA1.zlox.BAK")
        );
        assert_eq!(backup_name(Path::new("")), None);
    }

    #[test]
    fn a_folder_is_expanded_and_taken_from_the_zlog_folder_like_zlog() {
        let root = Path::new("/zlog");
        assert_eq!(folder_of("bak", root), Path::new("/zlog/bak"));
        assert_eq!(folder_of("/x/y", root), Path::new("/x/y"));
        std::env::set_var("ZLOG_BACKUP_TEST", "/env/dir");
        assert_eq!(
            folder_of("%ZLOG_BACKUP_TEST%/b", root),
            Path::new("/env/dir/b")
        );
        assert_eq!(folder_of("%ZLOG_ROOT%b", root), Path::new("/zlog/b"));
        assert_eq!(expand_variables("%ZLOG_ROOT%/b", root), "/zlog/b");
        assert_eq!(expand_variables("%ZLOG_ROOT%", root), "/zlog");
        assert_eq!(
            folder_of("%ZLOG_NOT_SET_HERE%b", root),
            Path::new("/zlog/%ZLOG_NOT_SET_HERE%b")
        );
        assert_eq!(expand_variables("50%", root), "50%");
    }

    #[test]
    fn variables_expand_like_expand_environment_strings() {
        let root = Path::new("/zlog");
        std::env::set_var("ZLOG_BACKUP_VAR", "v");
        for (text, want) in [
            ("D:\\%ZLOG_NO_SUCH%\\bak", "D:\\%ZLOG_NO_SUCH%\\bak"),
            ("%%", "%%"),
            ("a%%b", "a%%b"),
            ("%ZLOG_NO_SUCH%ZLOG_BACKUP_VAR%", "%ZLOG_NO_SUCHv"),
            ("%ZLOG_BACKUP_VAR%%ZLOG_BACKUP_VAR%", "vv"),
            ("x%ZLOG_BACKUP_VAR", "x%ZLOG_BACKUP_VAR"),
            ("%zlog_root%", "/zlog"),
        ] {
            assert_eq!(expand_variables(text, root), want, "{text}");
        }
    }

    fn log_of(calls: &[&str]) -> Log {
        Log {
            qsos: calls
                .iter()
                .map(|c| zlog_core::Qso {
                    callsign: c.to_string(),
                    ..zlog_core::Qso::default()
                })
                .collect(),
            ..Log::default()
        }
    }

    #[test]
    fn writes_the_log_in_memory_and_replaces_the_old_copy() {
        let dir = scratch("copy");
        let log_path = dir.join("log/test.zlox");
        std::fs::write(&log_path, b"stale file after a failed save").unwrap();
        let folder = dir.join("new/bak");
        let root = dir.join("log");
        let first = log_of(&["JA1ZLO"]);
        let target = backup(&log_path, folder.to_str().unwrap(), &root, &first).unwrap();
        assert_eq!(target, folder.join("test.zlox.BAK"));
        assert_eq!(
            std::fs::read(&target).unwrap(),
            first.to_bytes(Format::Zlox)
        );
        let second = log_of(&["JA1ZLO", "JA1YWX"]);
        backup(&log_path, folder.to_str().unwrap(), &root, &second).unwrap();
        assert_eq!(
            std::fs::read(&target).unwrap(),
            second.to_bytes(Format::Zlox)
        );
        let names: Vec<_> = std::fs::read_dir(&folder).unwrap().collect();
        assert_eq!(names.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_relative_folder_is_under_the_zlog_folder() {
        let dir = scratch("memory");
        let log_path = dir.join("elsewhere/new.zlo");
        let log = log_of(&["JA1ZLO"]);
        let target = backup(&log_path, "bak", &dir.join("log"), &log).unwrap();
        assert_eq!(target, dir.join("log/bak/new.zlo.BAK"));
        assert_eq!(std::fs::read(target).unwrap(), log.to_bytes(Format::Zlo));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let dir = scratch("errors");
        let log_path = dir.join("log/a.zlox");
        std::fs::write(&log_path, b"x").unwrap();
        let root = dir.join("log");
        let same = backup(&log_path, ".", &root, &Log::default()).unwrap_err();
        assert!(matches!(same, BackupError::SameFolder(_)));
        assert!(same.message("en").contains("the log's folder"));
        let file = dir.join("file");
        std::fs::write(&file, b"").unwrap();
        let blocked =
            backup(&log_path, file.to_str().unwrap(), &root, &Log::default()).unwrap_err();
        assert!(matches!(blocked, BackupError::CreateFolder(..)));
        let none = backup(Path::new(""), "bak", &root, &Log::default()).unwrap_err();
        assert!(matches!(none, BackupError::NoLog));
        let dir_as_log = backup(&root, file.to_str().unwrap(), &root, &Log::default());
        assert!(dir_as_log.is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unwritable_folder_keeps_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("readonly");
        let log_path = dir.join("log/a.zlox");
        std::fs::write(&log_path, b"x").unwrap();
        let folder = dir.join("ro");
        std::fs::create_dir(&folder).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let result = backup(&log_path, folder.to_str().unwrap(), &dir, &Log::default());
        let writable = std::fs::File::create(folder.join("probe")).is_ok();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !writable {
            assert!(matches!(result, Err(BackupError::Write(..))));
            assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
