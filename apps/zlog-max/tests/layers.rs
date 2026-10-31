use std::path::{Path, PathBuf};

const LAYERS: [&str; 3] = ["core", "drivers", "extensions"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rank(dir: &Path) -> Option<usize> {
    let dir = dir.canonicalize().ok()?;
    let crates = root().join("crates").canonicalize().ok()?;
    let layer = dir.strip_prefix(&crates).ok()?.components().next()?;
    LAYERS.iter().position(|l| layer.as_os_str() == *l)
}

fn path_deps(manifest: &Path) -> Vec<PathBuf> {
    let text = std::fs::read_to_string(manifest).unwrap();
    let dir = manifest.parent().unwrap();
    text.lines()
        .filter_map(|l| l.split_once("path = \"").map(|(_, rest)| rest))
        .filter_map(|rest| rest.split_once('"').map(|(p, _)| dir.join(p)))
        .collect()
}

#[test]
fn dependencies_point_from_extensions_to_drivers_to_core() {
    let mut checked = 0;
    for (i, layer) in LAYERS.iter().enumerate() {
        let dir = root().join("crates").join(layer);
        for entry in std::fs::read_dir(&dir).unwrap() {
            let manifest = entry.unwrap().path().join("Cargo.toml");
            if !manifest.is_file() {
                continue;
            }
            for dep in path_deps(&manifest) {
                let to = rank(&dep);
                assert!(
                    to.is_some_and(|to| to <= i),
                    "{} depends on {}",
                    manifest.display(),
                    dep.display()
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 60, "{checked}");
}

#[test]
fn every_crate_lives_in_a_layer() {
    for entry in std::fs::read_dir(root().join("crates")).unwrap() {
        let name = entry.unwrap().file_name();
        assert!(LAYERS.iter().any(|l| name == *l), "{name:?}");
    }
}
