use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn run(bin: &str, args: &[&str]) -> Output {
    Command::new(bin).args(args).output().unwrap()
}

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zlog-max/tests/regress")
}

pub fn fixture(path: &str) -> String {
    fixtures().join(path).display().to_string()
}

pub fn scratch(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("zlog-max-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name).display().to_string()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_and_scratch_paths_exist() {
        assert!(Path::new(&fixture("cfg/good.cfg")).is_file());
        let s = scratch("x");
        assert!(Path::new(&s).parent().unwrap().is_dir());
    }
}
