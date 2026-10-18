//! Check whether a newer release is published on GitHub.
//!
//! Port of ZyLO's `latest` (MIT, JA1ZLO), which checked at start-up and showed a
//! notification. Here the check runs only when the operator presses the button in
//! this window, so the program never contacts the network on its own.

use egui::{Color32, Ui};
use zlog_net::Fetch;
use zlog_ui::{Panel, View};

pub struct Latest {
    /// `owner/name` of the GitHub repository; empty when the build has none.
    repository: String,
    current: String,
    fetch: Option<Fetch>,
    result: Option<Result<Release, String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub url: String,
}

impl Latest {
    pub fn new(repository: &str, current: &str) -> Self {
        Latest {
            repository: repository
                .trim_start_matches("https://github.com/")
                .trim_end_matches('/')
                .into(),
            current: current.into(),
            fetch: None,
            result: None,
        }
    }
}

/// The latest release from a GitHub `releases/latest` response.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let tag = v["tag_name"]
        .as_str()
        .ok_or("no tag_name in the response")?;
    let url = v["html_url"].as_str().unwrap_or_default();
    Ok(Release {
        tag: tag.into(),
        url: url.into(),
    })
}

/// Whether version `a` is newer than `b`, comparing the numbers in them (`v1.2.10` > `1.2.9`).
pub fn is_newer(a: &str, b: &str) -> bool {
    let nums = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    nums(a) > nums(b)
}

zlog_core::texts!();

impl Panel for Latest {
    fn id(&self) -> &str {
        "latest"
    }

    fn title(&self, language: &str) -> String {
        texts().get(language, "title").into()
    }

    fn default_size(&self) -> [f32; 2] {
        [320.0, 120.0]
    }

    fn ui(&mut self, ui: &mut Ui, view: &View<'_>) {
        let lang = view.language;
        ui.label(texts().format(lang, "current", &[&self.current]));
        if self.repository.is_empty() {
            ui.weak(texts().get(lang, "no_repository"));
            return;
        }
        if let Some(f) = self.fetch.as_mut() {
            match f.poll() {
                Some(r) => {
                    self.result = Some(r.clone().and_then(|body| parse_release(&body)));
                    self.fetch = None;
                }
                None => {
                    ui.spinner();
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(200));
                }
            }
        }
        if self.fetch.is_none() && ui.button(texts().get(lang, "check")).clicked() {
            let url = format!(
                "https://api.github.com/repos/{}/releases/latest",
                self.repository
            );
            self.fetch = Some(Fetch::get(url));
        }
        match &self.result {
            Some(Ok(r)) if is_newer(&r.tag, &self.current) => {
                ui.colored_label(
                    Color32::from_rgb(0, 120, 0),
                    texts().format(lang, "newer", &[&r.tag]),
                );
                ui.hyperlink(&r.url);
            }
            Some(Ok(r)) => {
                ui.label(texts().format(lang, "up_to_date", &[&r.tag]));
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::RED, e);
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }

    #[test]
    fn versions() {
        assert!(is_newer("v1.2.10", "1.2.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(is_newer("3.0.5.0", "3.0.4.0"));
    }

    #[test]
    fn release_json() {
        let r = parse_release(
            r#"{"tag_name":"v3.0.4.0","html_url":"https://github.com/x/y/releases/tag/v3.0.4.0"}"#,
        )
        .unwrap();
        assert_eq!(r.tag, "v3.0.4.0");
        assert!(parse_release("{}").is_err());
    }
}
