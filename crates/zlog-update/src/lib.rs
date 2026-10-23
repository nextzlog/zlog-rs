//! Update zLog to the nightly build of its GitHub repository.
//!
//! Nothing happens until the operator opens
//! "Update" in the Help menu and presses a button, so the
//! program never contacts the network on its own.
//!
//! The running build knows the commit it was built from (embedded by `build.rs` from
//! `GITHUB_SHA`, set in the CI workflow); a local build has none and is a
//! development build, which is never replaced. The nightly release carries
//! `version.txt` (the commit of its binaries) and `<asset>.sha256` for each binary,
//! uploaded by `.github/workflows/build.yaml`. An update downloads the binary of
//! this platform, checks its SHA-256 against the published one and then:
//!
//! - Windows: replaces the running `zlog-rs.exe` (crate `self-replace`).
//! - macOS: mounts `zlog-rs.dmg` and swaps the `zLog.app` bundle holding the
//!   running program for the one in the image; when the program is not in a bundle
//!   or the bundle's folder is not writable, opens the image in Finder instead.
//! - Elsewhere: shows the link to the nightly release.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use egui::Ui;
use sha2::{Digest, Sha256};
use zlog_net::Fetch;
use zlog_ui::{Panel, View};

/// Tag of the release the CI workflow replaces on every push to master.
pub const NIGHTLY_TAG: &str = "nightly";

/// The repository whose nightly is followed when none is given.
pub const REPOSITORY: &str = "https://github.com/nextzlog/zlog-rs";

/// Asset holding the commit the nightly binaries were built from.
pub const VERSION_ASSET: &str = "version.txt";

/// The commit this program was built from, `None` for a development build.
pub fn current_commit() -> Option<&'static str> {
    Some(env!("ZLOG_COMMIT")).filter(|c| !c.is_empty())
}

/// When this program was built (`YYYY-MM-DD hh:mm UTC`).
pub fn built_at() -> &'static str {
    env!("ZLOG_BUILT")
}

/// The platforms the nightly release has a binary for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Other,
}

impl Platform {
    /// The platform this program was built for.
    pub fn current() -> Self {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Other
        }
    }

    /// Name of this platform's binary in the nightly release.
    pub fn asset(self) -> Option<&'static str> {
        match self {
            Platform::Windows => Some("zlog-rs.exe"),
            Platform::MacOs => Some("zlog-rs.dmg"),
            Platform::Other => None,
        }
    }
}

/// `owner/name` from a repository URL such as `https://github.com/owner/name/`.
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

/// The REST API address of the nightly release of `repo` (`owner/name`).
pub fn release_api_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases/tags/{NIGHTLY_TAG}")
}

/// The web page of the nightly release of `repo` (`owner/name`).
pub fn release_page_url(repo: &str) -> String {
    format!("https://github.com/{repo}/releases/tag/{NIGHTLY_TAG}")
}

/// One file attached to a release.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// When it was last uploaded (ISO 8601).
    pub updated: String,
}

/// The nightly release as the GitHub API describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// The asset called `name`.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The binary for `platform` and its checksum file, when both are published.
    pub fn binary(&self, platform: Platform) -> Option<(&Asset, &Asset)> {
        let name = platform.asset()?;
        Some((self.asset(name)?, self.asset(&format!("{name}.sha256"))?))
    }
}

/// The release in a GitHub `releases/tags/...` response.
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

/// Whether the nightly built from `nightly` differs from the running build
/// `current`; a development build (`None`) is never updated. Abbreviated commits
/// match their full form.
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

/// The SHA-256 in a checksum file (`<hex>` or `<hex>  <name>` as `sha256sum` and
/// `shasum -a 256` write), for the file called `name` when several are listed.
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
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// The SHA-256 of everything `reader` yields.
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

/// Error of [`verify`]; a text key, shown in the user's language by the window.
pub const CHECKSUM_MISMATCH: &str = "checksum_mismatch";

/// Check that the file at `path` has the SHA-256 `expected`.
pub fn verify(path: &Path, expected: &[u8; 32]) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let sum = sha256(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    if &sum == expected {
        Ok(())
    } else {
        Err(CHECKSUM_MISMATCH.into())
    }
}

/// The `.app` bundle holding the program at `exe` (`X.app/Contents/MacOS/prog`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let ok = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    ok.then(|| bundle.to_path_buf())
}

/// How an update ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The program was replaced; start this (exe or bundle) to run the new one.
    Replaced(PathBuf),
    /// The disk image could not be installed in place and was opened in Finder.
    Opened(PathBuf),
}

/// What the update thread reports.
enum Message {
    Progress(u64, Option<u64>),
    /// Language key of the step now running.
    Step(&'static str),
    Done(Result<Outcome, String>),
}

/// An update running on its own thread.
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

/// Download, verify and install `binary`; runs on the update thread.
fn install(
    platform: Platform,
    binary: &Asset,
    checksum: &Asset,
    report: &dyn Fn(Message),
) -> Result<Outcome, String> {
    let mut sums = Vec::new();
    zlog_net::download(&checksum.url, &mut sums, |_, _| {})?;
    let expected = parse_checksum(&String::from_utf8_lossy(&sums), &binary.name)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Windows: next to the exe, so the final rename stays on one volume.
    let dir = match platform {
        Platform::Windows => exe.parent().map(Path::to_path_buf),
        _ => None,
    }
    .unwrap_or_else(std::env::temp_dir);
    let file = download_path(&dir, &binary.name);
    let mut out = std::fs::File::create(&file).map_err(|e| e.to_string())?;
    let got = zlog_net::download(&binary.url, &mut out, |n, total| {
        report(Message::Progress(n, total))
    });
    drop(out);
    // A failed download (network, timeout, disk full) leaves no partial file.
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
            let r = self_replace::self_replace(&file).map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&file);
            r.map(|()| Outcome::Replaced(exe))
        }
        Platform::MacOs => {
            let dmg = dmg_path(&dir, &binary.name);
            std::fs::rename(&file, &dmg).map_err(|e| e.to_string())?;
            match bundle_of(&exe) {
                Some(bundle) => match swap_bundle(&dmg, &bundle) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(&dmg);
                        Ok(Outcome::Replaced(bundle))
                    }
                    // Say why the swap failed if the fallback fails too.
                    Err(swap) => open_dmg(dmg).map_err(|open| format!("{swap}; {open}")),
                },
                None => open_dmg(dmg),
            }
        }
        Platform::Other => Err("no binary for this platform".into()),
    }
}

/// Where the binary is downloaded: named per process, so two zLogs updating at once
/// never write the same file.
fn download_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.{}.download", std::process::id()))
}

/// The downloaded disk image, renamed to end in `.dmg`: `hdiutil` and `open` go by
/// the extension, and `zlog-rs.dmg.<pid>` was refused with exit status 1.
fn dmg_path(dir: &Path, name: &str) -> PathBuf {
    let stem = name.strip_suffix(".dmg").unwrap_or(name);
    dir.join(format!("{stem}.{}.dmg", std::process::id()))
}

/// The first seven characters of a commit (characters, not bytes: never splits one).
fn short_commit(commit: &str) -> String {
    commit.chars().take(7).collect()
}

/// Whether the Update button is offered: no update running and none done, or the
/// last one failed (then it reads Retry).
fn can_start(running: bool, outcome: &Option<Result<Outcome, String>>) -> bool {
    !running && matches!(outcome, None | Some(Err(_)))
}

/// Open the disk image in Finder, for the operator to copy zLog by hand.
fn open_dmg(dmg: PathBuf) -> Result<Outcome, String> {
    run("open", &[dmg.as_os_str()])?;
    Ok(Outcome::Opened(dmg))
}

/// Run `program` with `args`, failing unless it exits successfully; the error names
/// the program and carries what it wrote to stderr, so the operator sees why.
fn run(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    let out = std::process::Command::new(program)
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

/// Replace `bundle` by the `.app` in the disk image `dmg`: copy it next to
/// `bundle` first, then swap the two by renaming, so a failure leaves the old one.
fn swap_bundle(dmg: &Path, bundle: &Path) -> Result<(), String> {
    let parent = bundle.parent().ok_or("bundle has no folder")?;
    let name = bundle.file_name().ok_or("bundle has no name")?;
    let fresh = parent.join(".zlog-update.app");
    let old = parent.join(".zlog-old.app");
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&old);
    // Fails here when the folder is not writable (say /Applications without rights).
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
        // ditto keeps the code signature and extended attributes.
        run("ditto", &[app.as_os_str(), fresh.as_os_str()])
    })();
    let _ = run(
        "hdiutil",
        &["detach".as_ref(), "-quiet".as_ref(), mount.as_os_str()],
    );
    copied?;
    std::fs::rename(bundle, &old).map_err(|e| e.to_string())?;
    if let Err(e) = std::fs::rename(&fresh, parent.join(name)) {
        let _ = std::fs::rename(&old, bundle);
        return Err(e.to_string());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Start the new program `target`; run when this one has ended (see
/// [`Updater`]'s `Drop`), so that two never run at once and this one's settings,
/// saved on exit, never overwrite those of the new one.
fn restart(target: &Path) -> Result<(), String> {
    let started = if Platform::current() == Platform::MacOs {
        std::process::Command::new("open")
            .arg("-n")
            .arg(target)
            .spawn()
    } else {
        std::process::Command::new(target).spawn()
    };
    started.map(|_| ()).map_err(|e| e.to_string())
}

/// Where the check for the nightly stands.
enum Check {
    Idle,
    Release(Fetch),
    Version(Release, Fetch),
    Done(Release, String),
    Failed(String),
}

/// The window of "Update".
pub struct Updater {
    /// `owner/name` of the GitHub repository; empty when the build has none.
    repository: String,
    check: Check,
    update: Option<Update>,
    outcome: Option<Result<Outcome, String>>,
    /// The new build to start when this program ends: the window is dropped after
    /// the settings are saved on exit. Should the operator cancel the close (a log
    /// not saved), it starts at the next exit instead.
    restart_on_exit: Option<PathBuf>,
}

impl Drop for Updater {
    fn drop(&mut self) {
        if let Some(target) = self.restart_on_exit.take() {
            let _ = restart(&target);
        }
    }
}

/// Whether opening the window starts a check: not while updating or after an
/// update waiting for the restart.
fn checks_on_opening(updating: bool, outcome: &Option<Result<Outcome, String>>) -> bool {
    !updating && !matches!(outcome, Some(Ok(_)))
}

impl Updater {
    /// The updater for the repository at `url` (`https://github.com/owner/name`).
    pub fn new(url: &str) -> Self {
        Updater {
            repository: repository_path(url),
            check: Check::Idle,
            update: None,
            outcome: None,
            restart_on_exit: None,
        }
    }

    /// Advance the check and the update by what has arrived.
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
    fn id(&self) -> &str {
        "update"
    }

    fn menu(&self) -> zlog_ui::MenuGroup {
        zlog_ui::MenuGroup::Help
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [380.0, 180.0]
    }

    /// Every opening checks again: a new nightly may be out since the last look.
    fn opened(&mut self, _view: &View<'_>) {
        if !self.repository.is_empty() && checks_on_opening(self.update.is_some(), &self.outcome) {
            self.outcome = None;
            self.check = Check::Release(Fetch::get(release_api_url(&self.repository)));
        }
    }

    fn has_buttons(&self) -> bool {
        true
    }

    /// The step the window is at: check, update (or retry and check again after a
    /// failure), or restart into the new build; then Close.
    fn buttons(&mut self, ui: &mut Ui, view: &View<'_>) -> bool {
        let t = |k| texts().get(view.language, k);
        let platform = Platform::current();
        let commit = current_commit();
        let mut close = false;
        ui.horizontal(|ui| {
            if self.repository.is_empty() {
            } else if let Some(Ok(Outcome::Replaced(target))) = &self.outcome {
                if self.restart_on_exit.is_some() {
                    ui.label(t("restarting"));
                } else if ui.button(t("restart")).clicked() {
                    self.restart_on_exit = Some(target.clone());
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else {
                let mut check = matches!(self.check, Check::Idle | Check::Failed(_))
                    && ui.button(t("check")).clicked();
                if let Check::Done(release, nightly) = &self.check {
                    let startable = commit.is_some()
                        && (is_update(commit, nightly) || nightly.is_empty())
                        && can_start(self.update.is_some(), &self.outcome);
                    if let Some((b, s)) = release.binary(platform).filter(|_| startable) {
                        let failed = matches!(self.outcome, Some(Err(_)));
                        if ui
                            .button(t(if failed { "retry" } else { "update" }))
                            .clicked()
                        {
                            self.outcome = None;
                            self.update = Some(Update::start(platform, b.clone(), s.clone()));
                        }
                        // After a failure the nightly may have changed: check again.
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
                    // Development builds and other platforms update by hand.
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
        let mut u = Updater::new("https://github.com/o/n");
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
        // Never splits a character (a byte slice at 7 would panic here).
        assert_eq!(short_commit("ééééééééé"), "ééééééé");
        let p = download_path(Path::new("/d"), "zlog-rs.exe");
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
        // hdiutil and open need the image to end in .dmg.
        let dmg = dmg_path(Path::new("/d"), "zlog-rs.dmg");
        assert_eq!(dmg.extension().unwrap(), "dmg");
        let name = dmg.file_name().unwrap().to_string_lossy().to_string();
        assert_eq!(name, format!("zlog-rs.{}.dmg", std::process::id()));
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
            parse_checksum(&format!("{hex}  zlog-rs.exe\n"), "zlog-rs.exe").unwrap(),
            empty
        );
        let other = "00".repeat(32);
        let two = format!("{other}  zlog-rs.dmg\n{hex} *zlog-rs.exe\n");
        assert_eq!(parse_checksum(&two, "zlog-rs.exe").unwrap(), empty);
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

    const RELEASE: &str = r#"{"html_url":"https://github.com/nextzlog/zlog-rs/releases/tag/nightly",
        "assets":[
        {"name":"zlog-rs.exe","browser_download_url":"https://e/zlog-rs.exe","updated_at":"2026-10-20T01:00:00Z"},
        {"name":"zlog-rs.exe.sha256","browser_download_url":"https://e/zlog-rs.exe.sha256","updated_at":"x"},
        {"name":"zlog-rs.dmg","browser_download_url":"https://e/zlog-rs.dmg","updated_at":"y"},
        {"name":"version.txt","browser_download_url":"https://e/version.txt","updated_at":"z"}]}"#;

    #[test]
    fn assets_per_platform() {
        let r = parse_release(RELEASE).unwrap();
        let (b, s) = r.binary(Platform::Windows).unwrap();
        assert_eq!(b.url, "https://e/zlog-rs.exe");
        assert_eq!(b.updated, "2026-10-20T01:00:00Z");
        assert_eq!(s.name, "zlog-rs.exe.sha256");
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
        assert_eq!(repository_path(""), "nextzlog/zlog-rs");
        let repo = repository_path("https://github.com/nextzlog/zlog-rs/");
        assert_eq!(repo, "nextzlog/zlog-rs");
        assert_eq!(
            release_api_url(&repo),
            "https://api.github.com/repos/nextzlog/zlog-rs/releases/tags/nightly"
        );
        assert_eq!(
            release_page_url(&repo),
            "https://github.com/nextzlog/zlog-rs/releases/tag/nightly"
        );
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
