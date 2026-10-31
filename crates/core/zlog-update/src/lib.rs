use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use egui::Ui;
use sha2::{Digest, Sha256};
use zlog_net::Fetch;
use zlog_ui::{Panel, View};

const NIGHTLY_TAG: &str = "nightly";

pub const REPOSITORY: &str = zlog_ui::product::REPOSITORY;

pub const VERSION_ASSET: &str = "version.txt";

pub fn current_commit() -> Option<&'static str> {
    Some(env!("ZLOG_COMMIT")).filter(|c| !c.is_empty())
}

pub fn built_at() -> &'static str {
    env!("ZLOG_BUILT")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Other,
}

pub fn appimage() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) && appimage().is_some() {
            Platform::Linux
        } else {
            Platform::Other
        }
    }

    pub fn asset(self) -> Option<String> {
        let asset = zlog_ui::product::ASSET;
        match self {
            Platform::Windows => Some(format!("{asset}.exe")),
            Platform::MacOs => Some(format!("{asset}.dmg")),
            Platform::Linux => Some(format!("{asset}-x86_64.AppImage")),
            Platform::Other => None,
        }
    }
}

pub fn repository_path(url: &str) -> String {
    let url = if url.trim().is_empty() {
        REPOSITORY
    } else {
        url
    };
    url.trim_start_matches("https://github.com/")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .into()
}

pub fn release_api_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/tags/{NIGHTLY_TAG}")
}

pub fn release_page_url(repo: &str) -> String {
    format!("https://github.com/{repo}/releases/tag/{NIGHTLY_TAG}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub updated: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    pub fn binary(&self, platform: Platform) -> Option<(&Asset, &Asset)> {
        let name = platform.asset()?;
        Some((self.asset(&name)?, self.asset(&format!("{name}.sha256"))?))
    }
}

pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let assets = v["assets"].as_array().ok_or("no assets in the response")?;
    let assets = assets
        .iter()
        .filter_map(|a| {
            Some(Asset {
                name: a["name"].as_str()?.into(),
                url: a["browser_download_url"].as_str()?.into(),
                updated: a["updated_at"].as_str().unwrap_or_default().into(),
            })
        })
        .collect();
    Ok(Release {
        page: v["html_url"].as_str().unwrap_or_default().into(),
        assets,
    })
}

pub fn is_update(current: Option<&str>, nightly: &str) -> bool {
    let (Some(a), b) = (current, nightly.trim()) else {
        return false;
    };
    let (a, b) = (a.trim().to_ascii_lowercase(), b.to_ascii_lowercase());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    !(a.starts_with(&b) || b.starts_with(&a))
}

pub fn parse_checksum(text: &str, name: &str) -> Result<[u8; 32], String> {
    let mut first = None;
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(hex) = parts.next() else { continue };
        let file = parts.next().map(|f| f.trim_start_matches('*'));
        let Some(sum) = parse_hex(hex) else { continue };
        if file == Some(name) {
            return Ok(sum);
        }
        first.get_or_insert(sum);
    }
    first.ok_or_else(|| "no SHA-256 in the checksum file".into())
}

fn parse_hex(hex: &str) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    hex::decode_to_slice(hex, &mut out).ok()?;
    Some(out)
}

pub fn sha256(mut reader: impl Read) -> std::io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().into())
}

pub const CHECKSUM_MISMATCH: &str = "checksum_mismatch";

pub fn verify(path: &Path, expected: &[u8; 32]) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let sum = sha256(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    if &sum == expected {
        Ok(())
    } else {
        Err(CHECKSUM_MISMATCH.into())
    }
}

pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let ok = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    ok.then(|| bundle.to_path_buf())
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Replaced(PathBuf),
    Opened(PathBuf),
}

enum Message {
    Progress(u64, Option<u64>),
    Step(&'static str),
    Done(Result<Outcome, String>),
}

struct Update {
    rx: Receiver<Message>,
    progress: (u64, Option<u64>),
    step: &'static str,
}

impl Update {
    fn start(platform: Platform, binary: Asset, checksum: Asset) -> Self {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let report = |m| {
                let _ = tx.send(m);
            };
            let r = install(platform, &binary, &checksum, &report);
            report(Message::Done(r));
        });
        Update {
            rx,
            progress: (0, None),
            step: "step_download",
        }
    }
}

fn install(
    platform: Platform,
    binary: &Asset,
    checksum: &Asset,
    report: &dyn Fn(Message),
) -> Result<Outcome, String> {
    let mut sums = Vec::new();
    zlog_net::download(&checksum.url, &mut sums, |_, _| {})?;
    let expected = parse_checksum(&String::from_utf8_lossy(&sums), &binary.name)?;
    let exe = running_file(platform).ok_or("the running program is not known")?;
    // Windows: next to the exe, so the final rename stays on one volume.
    let dir = match platform {
        Platform::Windows | Platform::Linux => exe.parent().map(Path::to_path_buf),
        _ => None,
    }
    .unwrap_or_else(std::env::temp_dir);
    let file = download_path(&dir, &binary.name);
    let mut out = std::fs::File::create(&file).map_err(|e| e.to_string())?;
    let got = zlog_net::download(&binary.url, &mut out, |n, total| {
        report(Message::Progress(n, total))
    });
    drop(out);
    if let Err(e) = got {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    report(Message::Step("step_verify"));
    if let Err(e) = verify(&file, &expected) {
        let _ = std::fs::remove_file(&file);
        return Err(e);
    }
    report(Message::Step("step_install"));
    match platform {
        Platform::Windows => {
            let r = place_windows(&file, &exe);
            let _ = std::fs::remove_file(&file);
            r.map(Outcome::Replaced)
        }
        Platform::Linux => {
            let r = place_linux(&file, &exe);
            let _ = std::fs::remove_file(&file);
            r.map(Outcome::Replaced)
        }
        Platform::MacOs => {
            let dmg = dmg_path(&dir, &binary.name);
            std::fs::rename(&file, &dmg).map_err(|e| e.to_string())?;
            match bundle_of(&exe) {
                Some(bundle) => match swap_bundle(&dmg, &bundle) {
                    Ok(installed) => {
                        let _ = std::fs::remove_file(&dmg);
                        Ok(Outcome::Replaced(installed))
                    }
                    Err(swap) => open_dmg(dmg).map_err(|open| format!("{swap}; {open}")),
                },
                None => open_dmg(dmg),
            }
        }
        Platform::Other => Err("no binary for this platform".into()),
    }
}

fn place_windows(file: &Path, exe: &Path) -> Result<PathBuf, String> {
    self_replace::self_replace(file).map_err(|e| e.to_string())?;
    Ok(exe.to_path_buf())
}

#[cfg(unix)]
fn place_linux(file: &Path, appimage: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::Permissions::from_mode(0o755);
    std::fs::set_permissions(file, mode).map_err(|e| e.to_string())?;
    std::fs::rename(file, appimage).map_err(|e| e.to_string())?;
    Ok(appimage.to_path_buf())
}

#[cfg(not(unix))]
fn place_linux(_: &Path, _: &Path) -> Result<PathBuf, String> {
    Err("no binary for this platform".into())
}

fn running_file(platform: Platform) -> Option<PathBuf> {
    match platform {
        Platform::Linux => appimage(),
        _ => std::env::current_exe().ok(),
    }
}

const REMOVE_OLD: &str = "ZLOG_UPDATE_REMOVE";

fn old_exe(target: &Path) -> Option<PathBuf> {
    let platform = Platform::current();
    let exe = running_file(platform)?;
    let removable = matches!(platform, Platform::Windows | Platform::Linux);
    (removable && exe != target).then_some(exe)
}

pub fn remove_old_exe() {
    if let Some(old) = std::env::var_os(REMOVE_OLD) {
        std::env::remove_var(REMOVE_OLD);
        std::thread::spawn(move || remove_when_free(Path::new(&old), REMOVE_TRIES));
    }
}

const REMOVE_TRIES: u32 = 1500;

fn remove_when_free(old: &Path, tries: u32) -> bool {
    for _ in 0..tries {
        if std::fs::remove_file(old).is_ok() || !old.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

fn download_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{}.download", std::process::id()))
}

fn dmg_path(dir: &Path, name: &str) -> PathBuf {
    let stem = name.strip_suffix(".dmg").unwrap_or(name);
    dir.join(format!("{stem}.{}.dmg", std::process::id()))
}

fn short_commit(commit: &str) -> String {
    commit.chars().take(7).collect()
}

pub fn installable(current: Option<&str>, nightly: &str) -> bool {
    current.is_some() && is_update(current, nightly)
}

fn can_start(running: bool, outcome: &Option<Result<Outcome, String>>) -> bool {
    !running && matches!(outcome, None | Some(Err(_)))
}

fn open_dmg(dmg: PathBuf) -> Result<Outcome, String> {
    run("open", &[dmg.as_os_str()])?;
    Ok(Outcome::Opened(dmg))
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = zlog_process::external(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program}: {} {}", out.status, err.trim())
            .trim_end()
            .to_string())
    }
}

fn swap_bundle(dmg: &Path, bundle: &Path) -> Result<PathBuf, String> {
    let parent = bundle.parent().ok_or("bundle has no folder")?;
    bundle.file_name().ok_or("bundle has no name")?;
    let fresh = parent.join(".zlog-update.app");
    let old = parent.join(".zlog-old.app");
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&old);
    std::fs::create_dir(&fresh).map_err(|e| e.to_string())?;
    std::fs::remove_dir(&fresh).map_err(|e| e.to_string())?;
    let mount = std::env::temp_dir().join(format!("zlog-update-mount-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&mount);
    run(
        "hdiutil",
        &[
            "attach".as_ref(),
            "-nobrowse".as_ref(),
            "-readonly".as_ref(),
            "-mountpoint".as_ref(),
            mount.as_os_str(),
            dmg.as_os_str(),
        ],
    )?;
    let copied = (|| {
        let app = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .ok_or("no .app in the disk image")?;
        run("ditto", &[app.as_os_str(), fresh.as_os_str()])?;
        app.file_name()
            .map(|n| n.to_os_string())
            .ok_or_else(|| "no .app in the disk image".to_string())
    })();
    let _ = run(
        "hdiutil",
        &["detach".as_ref(), "-quiet".as_ref(), mount.as_os_str()],
    );
    let installed = parent.join(copied?);
    std::fs::rename(bundle, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&fresh, &installed) {
        let _ = std::fs::rename(&old, bundle);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(installed)
}

fn restart(target: &Path, old: Option<&Path>) -> Result<(), String> {
    let started = if Platform::current() == Platform::MacOs {
        zlog_process::external("open").arg("-n").arg(target).spawn()
    } else {
        let mut c = zlog_process::external(target);
        c.env_remove(REMOVE_OLD);
        if let Some(old) = old {
            c.env(REMOVE_OLD, old);
        }
        c.spawn()
    };
    started.map(|_| ()).map_err(|e| e.to_string())
}

enum Check {
    Idle,
    Release(Fetch),
    Version(Release, Fetch),
    Done(Release, String),
    Failed(String),
}

pub struct Updater {
    repository: String,
    check: Check,
    update: Option<Update>,
    outcome: Option<Result<Outcome, String>>,
    restart_on_exit: Option<PathBuf>,
}

impl Drop for Updater {
    fn drop(&mut self) {
        if let Some(target) = self.restart_on_exit.take() {
            let _ = restart(&target, old_exe(&target).as_deref());
        }
    }
}

fn checks_on_opening(updating: bool, outcome: &Option<Result<Outcome, String>>) -> bool {
    !updating && !matches!(outcome, Some(Ok(_)))
}

impl Updater {
    fn new() -> Self {
        Updater {
            repository: repository_path(zlog_ui::product::REPOSITORY),
            check: Check::Idle,
            update: None,
            outcome: None,
            restart_on_exit: None,
        }
    }

    fn poll(&mut self) -> bool {
        let busy = match &mut self.check {
            Check::Release(f) => match f.poll() {
                Some(r) => {
                    let next = r.clone().and_then(|body| parse_release(&body));
                    self.check = match next {
                        Ok(rel) => match rel.asset(VERSION_ASSET) {
                            Some(a) => {
                                let f = Fetch::get(a.url.clone());
                                Check::Version(rel, f)
                            }
                            None => Check::Done(rel, String::new()),
                        },
                        Err(e) => Check::Failed(e),
                    };
                    true
                }
                None => true,
            },
            Check::Version(rel, f) => {
                if let Some(r) = f.poll() {
                    self.check = match r {
                        Ok(v) => Check::Done(rel.clone(), v.trim().into()),
                        Err(e) => Check::Failed(e.clone()),
                    };
                }
                true
            }
            _ => false,
        };
        let mut running = false;
        if let Some(u) = self.update.as_mut() {
            running = true;
            loop {
                match u.rx.try_recv() {
                    Ok(Message::Progress(n, t)) => u.progress = (n, t),
                    Ok(Message::Step(s)) => u.step = s,
                    Ok(Message::Done(r)) => {
                        self.outcome = Some(r);
                        self.update = None;
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.outcome = Some(Err("update thread ended".into()));
                        self.update = None;
                        break;
                    }
                }
            }
        }
        busy || running
    }
}

zlog_core::texts!();

impl Panel for Updater {
    fn window_kind(&self) -> zlog_ui::WindowKind {
        zlog_ui::WindowKind::Dialog
    }

    fn id(&self) -> &str {
        "update"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Help
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn commands(&self, language: &str) -> Vec<zlog_ui::Command> {
        let title = texts().get(language, "title");
        let mut c =
            zlog_ui::Command::window(title, title).menu_label(texts().get(language, "menu"));
        c.toolbar = false;
        vec![c]
    }

    fn default_size(&self) -> [f32; 2] {
        [380.0, 180.0]
    }

    fn opened(&mut self, _view: &View<'_>) {
        if !self.repository.is_empty() && checks_on_opening(self.update.is_some(), &self.outcome) {
            self.outcome = None;
            self.check = Check::Release(Fetch::get(release_api_url(&self.repository)));
        }
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        let t = |k| texts().get(lang, k);
        if self.poll() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let commit = current_commit();
        let current = match commit {
            Some(c) => texts().format(lang, "build", &[&short_commit(c), &built_at()]),
            None => t("development").into(),
        };
        ui.label(texts().format(lang, "current", &[&current]));
        if self.repository.is_empty() {
            ui.weak(t("no_repository"));
            return;
        }
        let page = release_page_url(&self.repository);
        let platform = Platform::current();
        match &self.check {
            Check::Idle => {}
            Check::Failed(e) => {
                ui.colored_label(zlog_ui::palette::error(ui), e);
            }
            Check::Release(_) | Check::Version(..) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t("checking"));
                });
            }
            Check::Done(release, nightly) => {
                let binary = release.binary(platform);
                let updated = binary.map(|(b, _)| b.updated.as_str()).unwrap_or_default();
                let short = short_commit(nightly);
                ui.label(texts().format(lang, "available", &[&short, &updated]));
                if commit.is_none() || binary.is_none() {
                    ui.weak(t(if commit.is_none() {
                        "manual_dev"
                    } else {
                        "manual_platform"
                    }));
                    ui.hyperlink(&page);
                } else if !is_update(commit, nightly) && !nightly.is_empty() {
                    ui.label(t("up_to_date"));
                }
            }
        }
        if let Some(u) = &self.update {
            ui.label(t(u.step));
            let (n, total) = u.progress;
            match total {
                Some(total) if total > 0 => {
                    ui.add(egui::ProgressBar::new(n as f32 / total as f32).show_percentage());
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("{} kB", n / 1024));
                    });
                }
            }
        }
        match &self.outcome {
            Some(Ok(Outcome::Replaced(_))) => {
                ui.colored_label(zlog_ui::palette::good(ui), t("replaced"));
            }
            Some(Ok(Outcome::Opened(dmg))) => {
                ui.label(texts().format(lang, "opened", &[&dmg.display().to_string()]));
            }
            Some(Err(e)) => {
                let text = if e == CHECKSUM_MISMATCH {
                    texts().get(lang, CHECKSUM_MISMATCH)
                } else {
                    e.as_str()
                };
                ui.colored_label(zlog_ui::palette::error(ui), text);
                ui.hyperlink(&page);
            }
            None => {}
        }
    }

    fn quit_cancelled(&mut self) -> bool {
        self.restart_on_exit.take().is_some()
    }

    fn capability_mut(&mut self, want: zlog_ui::WantMut) -> Option<zlog_ui::CapabilityMut<'_>> {
        match want {
            zlog_ui::WantMut::Buttons => Some(zlog_ui::CapabilityMut::Buttons(self)),
            _ => None,
        }
    }
}

impl zlog_ui::ButtonRow for Updater {
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        let platform = Platform::current();
        let commit = current_commit();
        let mut close = false;
        zlog_ui::form::buttons(ui, |ui| {
            if self.repository.is_empty() {
            } else if let Some(Ok(Outcome::Replaced(target))) = &self.outcome {
                if self.restart_on_exit.is_some() {
                    ui.label(t("restarting"));
                } else if ui.button(t("restart")).clicked() {
                    self.restart_on_exit = Some(target.clone());
                    ui.ctx()
                        .send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            } else {
                let mut check = matches!(self.check, Check::Idle | Check::Failed(_))
                    && ui.button(t("check")).clicked();
                if let Check::Done(release, nightly) = &self.check {
                    let startable = installable(commit, nightly)
                        && can_start(self.update.is_some(), &self.outcome);
                    let binary = release.binary(platform);
                    if let Some((b, s)) = binary.filter(|_| startable) {
                        let failed = matches!(self.outcome, Some(Err(_)));
                        let label = if failed { "retry" } else { "update" };
                        if ui.button(t(label)).clicked() {
                            self.outcome = None;
                            self.update = Some(Update::start(platform, b.clone(), s.clone()));
                        }
                        check |= failed && ui.button(t("check")).clicked();
                    }
                }
                if check {
                    self.outcome = None;
                    self.check = Check::Release(Fetch::get(release_api_url(&self.repository)));
                }
            }
            close = ui.button(t("close")).clicked();
        });
        close
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    remove_old_exe();
    b.panel(620, |_| Updater::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_new_build_starts_only_when_the_window_is_dropped() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zlog-update-restart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mark = dir.join("started");
        let script = dir.join("new-build");
        std::fs::write(&script, format!("#!/bin/sh\ntouch '{}'\n", mark.display())).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut u = Updater::new();
        u.restart_on_exit = Some(script);
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!mark.exists());
        drop(u);
        let t = std::time::Instant::now();
        while !mark.exists() && t.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(mark.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_is_replaced_in_place() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zlog-update-appimage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let running = dir.join("zlog-max-x86_64.AppImage");
        std::fs::write(&running, "old").unwrap();
        let file = download_path(&dir, "zlog-max-x86_64.AppImage");
        std::fs::write(&file, "new").unwrap();
        let placed = place_linux(&file, &running).unwrap();
        assert_eq!(placed, running);
        assert_eq!(std::fs::read_to_string(&running).unwrap(), "new");
        let mode = std::fs::metadata(&running).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_renamed_appimage_is_updated_in_place_and_nothing_is_overwritten() {
        let dir = std::env::temp_dir().join(format!("zlog-update-renamed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let running = dir.join("zlog.AppImage");
        std::fs::write(&running, "old").unwrap();
        let other = dir.join("zlog-max-x86_64.AppImage");
        std::fs::write(&other, "unrelated").unwrap();
        let file = download_path(&dir, "zlog-max-x86_64.AppImage");
        std::fs::write(&file, "new").unwrap();
        let placed = place_linux(&file, &running).unwrap();
        assert_eq!(placed, running);
        assert_eq!(std::fs::read_to_string(&running).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "unrelated");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancelled_quit_forgets_the_restart() {
        let mut u = Updater::new();
        assert!(!u.quit_cancelled());
        u.restart_on_exit = Some(PathBuf::from("/nonexistent/zlog-new"));
        assert!(u.quit_cancelled());
        assert!(u.restart_on_exit.is_none());
    }

    #[test]
    fn the_old_exe_variable_is_not_inherited() {
        let dir = std::env::temp_dir().join(format!("zlog-update-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("old.exe");
        std::fs::write(&old, "old").unwrap();
        std::env::set_var(REMOVE_OLD, &old);
        remove_old_exe();
        assert!(std::env::var_os(REMOVE_OLD).is_none());
        let t = std::time::Instant::now();
        while old.exists() && t.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!old.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_checks_unless_updating_or_updated() {
        assert!(checks_on_opening(false, &None));
        assert!(checks_on_opening(false, &Some(Err("x".into()))));
        assert!(!checks_on_opening(true, &None));
        let done = Some(Ok(Outcome::Opened(std::path::PathBuf::from("x.dmg"))));
        assert!(!checks_on_opening(false, &done));
    }

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn retry_after_a_failed_update() {
        assert!(can_start(false, &None));
        assert!(can_start(false, &Some(Err("network".into()))));
        assert!(!can_start(true, &None));
        assert!(!can_start(false, &Some(Ok(Outcome::Opened("x".into())))));
    }

    #[test]
    fn short_commits_and_temp_names() {
        assert_eq!(short_commit("0123456789abcdef"), "0123456");
        assert_eq!(short_commit("ab"), "ab");
        assert_eq!(short_commit("ééééééééé"), "ééééééé");
        let p = download_path(Path::new("/d"), "zlog-max.exe");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
        let dmg = dmg_path(Path::new("/d"), "zlog-max.dmg");
        assert_eq!(dmg.extension().unwrap(), "dmg");
        let name = dmg.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, format!("zlog-max.{}.dmg", std::process::id()));
    }

    #[test]
    fn commits_compare() {
        let full = "0123456789abcdef0123456789abcdef01234567";
        assert!(!is_update(Some(full), full));
        assert!(!is_update(Some(full), "0123456\n"));
        assert!(!is_update(Some("0123456"), &full.to_uppercase()));
        assert!(is_update(Some(full), "fedcba9"));
        assert!(!is_update(None, "fedcba9"), "development builds stay");
        assert!(!is_update(Some(full), ""));
    }

    #[test]
    fn checksums() {
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let empty = sha256(&b""[..]).unwrap();
        assert_eq!(parse_checksum(hex, "x").unwrap(), empty);
        assert_eq!(
            parse_checksum(&format!("{hex}  zlog-max.exe\n"), "zlog-max.exe").unwrap(),
            empty
        );
        let other = "00".repeat(32);
        let two = format!("{other}  zlog-max.dmg\n{hex} *zlog-max.exe\n");
        assert_eq!(parse_checksum(&two, "zlog-max.exe").unwrap(), empty);
        assert!(parse_checksum("not a sum", "x").is_err());
        assert_eq!(
            sha256(&b"abc"[..]).unwrap(),
            parse_checksum(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                "x"
            )
            .unwrap()
        );
    }

    #[test]
    fn files_are_verified() {
        let dir = std::env::temp_dir().join(format!("zlog-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.bin");
        std::fs::write(&file, b"abc").unwrap();
        let good = sha256(&b"abc"[..]).unwrap();
        assert!(verify(&file, &good).is_ok());
        assert!(verify(&file, &[0; 32]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    const RELEASE: &str = r#"{"html_url":"https://github.com/nextzlog/zmax/releases/tag/nightly",
        "assets":[
        {"name":"zlog-max.exe","browser_download_url":"https://e/zlog-max.exe","updated_at":"2026-10-20T01:00:00Z"},
        {"name":"zlog-max.exe.sha256","browser_download_url":"https://e/zlog-max.exe.sha256","updated_at":"x"},
        {"name":"zlog-max.dmg","browser_download_url":"https://e/zlog-max.dmg","updated_at":"y"},
        {"name":"version.txt","browser_download_url":"https://e/version.txt","updated_at":"z"}]}"#;

    #[test]
    fn assets_per_platform() {
        let r = parse_release(RELEASE).unwrap();
        let (b, s) = r.binary(Platform::Windows).unwrap();
        assert_eq!(b.url, "https://e/zlog-max.exe");
        assert_eq!(b.updated, "2026-10-20T01:00:00Z");
        assert_eq!(s.name, "zlog-max.exe.sha256");
        assert!(
            r.binary(Platform::MacOs).is_none(),
            "no checksum, no update"
        );
        assert!(r.binary(Platform::Other).is_none());
        assert!(r.asset(VERSION_ASSET).is_some());
        assert!(parse_release("{}").is_err());
    }

    #[test]
    fn urls() {
        assert_eq!(repository_path(""), "nextzlog/zmax");
        let repo = repository_path("https://github.com/nextzlog/zmax/");
        assert_eq!(repo, "nextzlog/zmax");
        assert_eq!(
            release_api_url(&repo),
            "https://api.github.com/repos/nextzlog/zmax/releases/tags/nightly"
        );
        assert_eq!(
            release_page_url(&repo),
            "https://github.com/nextzlog/zmax/releases/tag/nightly"
        );
    }

    #[test]
    fn the_binaries_and_repository_of_zlog_max() {
        assert_eq!(Platform::Windows.asset().unwrap(), "zlog-max.exe");
        assert_eq!(Platform::MacOs.asset().unwrap(), "zlog-max.dmg");
        assert_eq!(Platform::Linux.asset().unwrap(), "zlog-max-x86_64.AppImage");
        assert_eq!(Platform::Other.asset(), None);
        assert_eq!(Updater::new().repository, "nextzlog/zmax");
    }

    #[test]
    fn only_a_newer_nightly_is_installable() {
        let c = "0123456789abcdef";
        assert!(!installable(Some(c), c));
        assert!(installable(Some(c), "fedcba9"));
        assert!(!installable(Some(c), ""));
        assert!(!installable(Some(c), " "));
        assert!(!installable(None, "fedcba9"));
    }

    #[test]
    fn bundles() {
        let exe = Path::new("/Applications/zLog.app/Contents/MacOS/zlog-gui");
        assert_eq!(
            bundle_of(exe),
            Some(PathBuf::from("/Applications/zLog.app"))
        );
        assert_eq!(bundle_of(Path::new("/usr/local/bin/zlog-gui")), None);
    }
}
