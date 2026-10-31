use std::io::Write;
use std::path::Path;

pub fn stage(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<tempfile::NamedTempFile> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let old = std::fs::metadata(path).ok();
    if old.as_ref().is_some_and(|m| m.permissions().readonly()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("{} is read-only", path.display()),
        ));
    }
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mode = old.as_ref().map_or(0o666, |m| m.mode() & 0o777);
        builder.permissions(std::fs::Permissions::from_mode(mode));
        mode
    };
    let mut file = builder.tempfile_in(dir)?;
    #[cfg(unix)]
    if let Some(old) = &old {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let _ = std::os::unix::fs::fchown(file.as_file(), Some(old.uid()), Some(old.gid()));
        file.as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    write(file.as_file_mut())?;
    file.as_file().sync_all()?;
    Ok(file)
}

const MAX_LINKS: usize = 40;

fn through_links(path: &Path) -> std::io::Result<std::path::PathBuf> {
    let mut path = path.to_path_buf();
    for _ in 0..MAX_LINKS {
        match std::fs::read_link(&path) {
            Ok(to) => path = path.parent().map_or(to.clone(), |d| d.join(&to)),
            Err(_) => return Ok(path),
        }
    }
    Err(std::io::Error::other(format!(
        "{}: too many levels of symbolic links",
        path.display()
    )))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let path = through_links(path)?;
    stage(&path, |f| f.write_all(bytes))?
        .persist(&path)
        .map(drop)
        .map_err(|e| e.error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert!(stage(&path, |_| Err(std::io::Error::other("x"))).is_err());
        assert!(write_atomic(dir.path(), b"x").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_save_keeps_the_mode_and_writes_through_a_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("real")).unwrap();
        let target = dir.path().join("real").join("a.zlo");
        write_atomic(&target, b"one").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o664)).unwrap();
        let link = dir.path().join("link.zlo");
        std::os::unix::fs::symlink("real/a.zlo", &link).unwrap();
        write_atomic(&link, b"two").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&target).unwrap(), b"two");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o664);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_loop_is_an_error_and_replaces_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::os::unix::fs::symlink("b", &a).unwrap();
        std::os::unix::fs::symlink("a", &b).unwrap();
        assert!(write_atomic(&a, b"x").is_err());
        for p in [&a, &b] {
            assert!(std::fs::symlink_metadata(p)
                .unwrap()
                .file_type()
                .is_symlink());
        }
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_read_only_file_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.zlo");
        write_atomic(&path, b"one").unwrap();
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&path, p).unwrap();
        let e = write_atomic(&path, b"two").unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(std::fs::read(&path).unwrap(), b"one");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn setuid_and_setgid_bits_are_not_copied_and_the_owner_is_kept() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.zlo");
        write_atomic(&path, b"one").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o6640)).unwrap();
        let before = std::fs::metadata(&path).unwrap();
        write_atomic(&path, b"two").unwrap();
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!(after.mode() & 0o7777, 0o640);
        assert_eq!((after.uid(), after.gid()), (before.uid(), before.gid()));
    }
}
