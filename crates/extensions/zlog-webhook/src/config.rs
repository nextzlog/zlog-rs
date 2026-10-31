use zlog_ui::Section;

use crate::site::Site;

const URL: &str = "url";
const ENABLED: &str = "enabled";
const EDITS: &str = "edits";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub url: String,
    pub enabled: bool,
}

impl Target {
    pub fn ready(&self) -> bool {
        self.enabled && !self.url.trim().is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    pub targets: [Target; 2],
    pub edits: bool,
}

fn key(site: Site, name: &str) -> String {
    format!("{}.{name}", site.code())
}

impl Config {
    pub fn load(section: &Section) -> Self {
        let on = |key: &str| section.get(key).is_some_and(|v| v == "true");
        let mut c = Config {
            edits: on(EDITS),
            ..Config::default()
        };
        for site in Site::ALL {
            let get = |name: &str| {
                let old = (site == Site::Discord).then(|| section.get(name)).flatten();
                section.get(&key(site, name)).or(old)
            };
            let t = c.target_mut(site);
            t.url = get(URL).cloned().unwrap_or_default();
            t.enabled = get(ENABLED).is_some_and(|v| v == "true");
        }
        c
    }

    pub fn save(&self, section: &mut Section) {
        for site in Site::ALL {
            let t = self.target(site);
            section.insert(key(site, URL), t.url.trim().into());
            section.insert(key(site, ENABLED), t.enabled.to_string());
        }
        section.insert(EDITS.into(), self.edits.to_string());
        section.remove(URL);
        section.remove(ENABLED);
    }

    pub fn target(&self, site: Site) -> &Target {
        &self.targets[site.index()]
    }

    pub fn target_mut(&mut self, site: Site) -> &mut Target {
        &mut self.targets[site.index()]
    }

    pub fn ready(&self, site: Site) -> bool {
        self.target(site).ready()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_its_section() {
        let mut c = Config {
            edits: false,
            ..Config::default()
        };
        *c.target_mut(Site::Discord) = Target {
            url: "https://discord.com/api/webhooks/1/x".into(),
            enabled: true,
        };
        c.target_mut(Site::Slack).url = "https://hooks.slack.com/services/T/B/x".into();
        let mut s = Section::new();
        c.save(&mut s);
        assert_eq!(Config::load(&s), c);
        assert!(c.ready(Site::Discord));
        assert!(!c.ready(Site::Slack));
        assert!(!Config::default().ready(Site::Discord));
    }

    #[test]
    fn the_keys_of_earlier_versions_are_discords() {
        let mut s = Section::new();
        s.insert("url".into(), "https://discord.com/api/webhooks/1/x".into());
        s.insert("enabled".into(), "true".into());
        s.insert("edits".into(), "true".into());
        let c = Config::load(&s);
        assert!(c.ready(Site::Discord) && c.edits);
        assert_eq!(c.target(Site::Slack), &Target::default());
        c.save(&mut s);
        assert!(!s.contains_key("url") && !s.contains_key("enabled"));
        assert_eq!(s["discord.url"], "https://discord.com/api/webhooks/1/x");
        assert_eq!(s["discord.enabled"], "true");
        assert_eq!(s["slack.enabled"], "false");
        assert_eq!(Config::load(&s), c);
    }
}
