use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;
use zlog_ui::{ContestList, Date, ListedContest, ListedFile};

zlog_core::texts!();

pub const MARKET_URL: &str = "https://pkg.zlog.org/market.json";

pub const PAGE_URL: &str = "https://use.zlog.org";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sum {
    Hex(String),
    Url(String),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub sum: Sum,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pkg {
    pub tag: String,
    pub msg: String,
    pub web: String,
    pub doc: String,
    pub uses: Vec<String>,
    pub stable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Market {
    pub files: BTreeMap<String, BTreeMap<String, Item>>,
    pub pkg: BTreeMap<String, Pkg>,
}

pub const DLL: &str = "dll";

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn md5_hex(text: &str) -> Option<String> {
    text.trim_start_matches('\u{feff}')
        .split_whitespace()
        .find(|t| t.len() == 32 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

pub fn md5_entries(text: &str) -> Option<Vec<(String, String)>> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim_start().starts_with('<') {
        return None;
    }
    let entries: Vec<(String, String)> = text
        .lines()
        .filter_map(|line| {
            let hex = md5_hex(line)?;
            let name = match line.trim().strip_prefix("MD5 (") {
                Some(rest) => rest.rsplit_once(") =").map_or("", |(n, _)| n),
                None => line
                    .split_whitespace()
                    .find(|t| !t.eq_ignore_ascii_case(&hex))
                    .unwrap_or_default()
                    .trim_start_matches('*'),
            };
            Some((
                name.rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                hex,
            ))
        })
        .collect();
    Some(entries).filter(|e| !e.is_empty())
}

pub fn md5_for<'a>(entries: &'a [(String, String)], url: &str) -> Option<&'a str> {
    let name = url.rsplit('/').next().unwrap_or_default();
    match entries {
        [(_, hex)] => Some(hex),
        _ => entries
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, h)| h.as_str()),
    }
}

fn sum_of(v: &Value) -> Sum {
    let sum = text(v, "sum");
    match md5_hex(&sum) {
        Some(hex) if hex.len() == sum.trim().len() => Sum::Hex(hex),
        _ if sum.starts_with("https://") => Sum::Url(sum),
        _ => Sum::Missing,
    }
}

fn items(map: &serde_json::Map<String, Value>) -> BTreeMap<String, Item> {
    map.iter()
        .filter_map(|(k, v)| {
            let item = Item {
                sum: sum_of(v),
                url: text(v, "url"),
            };
            (item.url.starts_with("https://") && !file_name(&item.url).is_empty())
                .then(|| (k.clone(), item))
        })
        .collect()
}

pub fn parse(json: &str) -> Result<Market, String> {
    let root: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let Some(root) = root.as_object() else {
        return Err("market.json is not an object".into());
    };
    let mut market = Market::default();
    for (section, v) in root {
        let Some(map) = v.as_object() else {
            continue;
        };
        if section != "pkg" {
            market.files.insert(section.clone(), items(map));
            continue;
        }
        for (k, v) in map {
            let uses = v
                .get("use")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let p = Pkg {
                tag: text(v, "tag"),
                msg: text(v, "msg"),
                web: text(v, "web"),
                doc: text(v, "doc"),
                uses,
                stable: text(v, "exp") != "unstable",
            };
            market.pkg.insert(k.clone(), p);
        }
    }
    Ok(market)
}

pub fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or_default();
    path.rsplit('/').next().unwrap_or_default().to_string()
}

pub struct Listing {
    pub contests: Vec<ListedContest>,
    pub sums: HashMap<String, Sum>,
}

impl Market {
    pub fn item(&self, used: &str) -> Option<&Item> {
        let (section, key) = used.split_once('.')?;
        self.files.get(section)?.get(key)
    }

    fn section(&self, name: &str) -> impl Iterator<Item = (&String, &Item)> {
        self.files.get(name).into_iter().flatten()
    }

    fn dat_named(&self, name: &str) -> Option<&Item> {
        self.section("dat")
            .map(|(_, v)| v)
            .find(|v| file_name(&v.url).eq_ignore_ascii_case(name))
    }

    pub fn files_of_pkg(&self, pkg: &Pkg) -> Vec<&Item> {
        let mut out: Vec<&Item> = Vec::new();
        for item in pkg
            .uses
            .iter()
            .filter(|u| u.split_once('.').is_some_and(|(s, _)| s != DLL))
            .filter_map(|u| self.item(u))
        {
            if !out.iter().any(|o| o.url == item.url) {
                out.push(item);
            }
        }
        out
    }

    pub fn files_of(&self, cfg: &str, referenced: Option<&str>) -> Vec<&Item> {
        let mut out: Vec<&Item> = self
            .section("cfg")
            .filter(|(k, _)| *k == cfg)
            .map(|(_, v)| v)
            .collect();
        let same = self
            .files
            .iter()
            .filter(|(s, _)| *s != "cfg" && *s != DLL)
            .flat_map(|(_, m)| m.iter())
            .filter(|(k, _)| k.eq_ignore_ascii_case(cfg))
            .map(|(_, v)| v);
        let named = referenced.into_iter().flat_map(|r| {
            self.section("dat")
                .map(|(_, v)| v)
                .filter(move |v| file_name(&v.url).eq_ignore_ascii_case(r))
        });
        for item in same.chain(named) {
            if !out.iter().any(|o| o.url == item.url) {
                out.push(item);
            }
        }
        out
    }

    pub fn listing(&self, referenced: impl Fn(&str) -> Option<String>, no_sum: &str) -> Listing {
        let mut sums = HashMap::new();
        let mut listed = |items: Vec<&Item>| {
            let mut items = items;
            for cfg in items
                .clone()
                .iter()
                .filter(|i| file_name(&i.url).to_lowercase().ends_with(".cfg"))
            {
                if let Some(dat) = referenced(&file_name(&cfg.url)).and_then(|d| self.dat_named(&d))
                {
                    if !items.iter().any(|o| o.url == dat.url) {
                        items.push(dat);
                    }
                }
            }
            items
                .into_iter()
                .map(|f| {
                    let name = file_name(&f.url);
                    sums.insert(f.url.clone(), f.sum.clone());
                    ListedFile {
                        name,
                        url: f.url.clone(),
                        note: match f.sum {
                            Sum::Missing => no_sum.to_string(),
                            _ => String::new(),
                        },
                    }
                })
                .collect::<Vec<_>>()
        };
        let mut pkgs: Vec<(&String, &Pkg)> = self
            .pkg
            .iter()
            .filter(|(_, p)| {
                p.uses
                    .iter()
                    .any(|u| u.starts_with("cfg.") && self.item(u).is_some())
            })
            .collect();
        pkgs.sort_by_key(|(k, p)| (!p.stable, k.to_lowercase()));
        let mut contests = Vec::new();
        for (key, p) in &pkgs {
            contests.push(ListedContest {
                name: Some(p.tag.clone())
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| key.to_string()),
                rules: Some(p.web.clone()).filter(|w| !w.is_empty()),
                note: p.msg.clone(),
                doc: p.doc.clone(),
                experimental: !p.stable,
                files: listed(self.files_of_pkg(p)),
                ..ListedContest::default()
            });
        }
        let used = |cfg: &str| {
            pkgs.iter()
                .any(|(_, p)| p.uses.iter().any(|u| u == &format!("cfg.{cfg}")))
        };
        for (key, _) in self.section("cfg").filter(|(k, _)| !used(k)) {
            contests.push(ListedContest {
                name: key.clone(),
                files: listed(self.files_of(key, None)),
                ..ListedContest::default()
            });
        }
        Listing { contests, sums }
    }
}

pub fn verify(name: &str, bytes: &[u8], sum: &str, language: &str) -> Result<(), String> {
    match checksum(bytes).eq_ignore_ascii_case(sum) {
        true => Ok(()),
        false => Err(texts().format(language, "sum_mismatch", &[&name])),
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))?;
    zlog_core::file::write_atomic(path, bytes)
}

fn validators_path(cache: &Path) -> PathBuf {
    cache.with_extension("http.json")
}

fn read_validators(path: &Path) -> zlog_net::Validators {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_validators(path: &Path, v: &zlog_net::Validators) -> std::io::Result<()> {
    write_atomic(path, &serde_json::to_vec(v)?)
}

pub fn default_folder() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join(zlog_ui::APP_DIR)
        .join("cfg")
}

fn date_of(t: SystemTime) -> Date {
    use chrono::Datelike;
    let t: chrono::DateTime<chrono::Local> = t.into();
    (t.year(), t.month(), t.day())
}

type Stamp = (SystemTime, u64);

pub const MD5_RETRY: Duration = Duration::from_secs(60);

pub const MIN_REFRESH: Duration = Duration::from_secs(600);

pub struct MarketList {
    get: fn(zlog_net::Request) -> zlog_net::Exchange,
    fetch: Option<zlog_net::Exchange>,
    validators: zlog_net::Validators,
    list: Option<Result<Listing, String>>,
    cache: Option<PathBuf>,
    folder: RefCell<PathBuf>,
    json: Option<String>,
    relist: Cell<bool>,
    requested: Option<Instant>,
    loaded: bool,
    checked: Option<Date>,
    hashes: RefCell<HashMap<PathBuf, (Stamp, String)>>,
    md5: HashMap<String, Vec<(String, String)>>,
    md5_stale: std::collections::HashSet<String>,
    md5_failed: HashMap<String, Instant>,
    md5_fetches: Vec<(String, zlog_net::Exchange)>,
    language: RefCell<String>,
}

impl Default for MarketList {
    fn default() -> Self {
        MarketList::new(
            dirs::cache_dir().map(|d| d.join(zlog_ui::APP_DIR).join("market.json")),
            default_folder(),
            zlog_net::Exchange::send,
        )
    }
}

impl MarketList {
    pub fn new(
        cache: Option<PathBuf>,
        folder: PathBuf,
        get: fn(zlog_net::Request) -> zlog_net::Exchange,
    ) -> Self {
        MarketList {
            get,
            fetch: None,
            validators: zlog_net::Validators::default(),
            list: None,
            cache,
            folder: RefCell::new(folder),
            json: None,
            relist: Cell::new(false),
            requested: None,
            loaded: false,
            checked: None,
            hashes: RefCell::default(),
            md5: HashMap::new(),
            md5_stale: Default::default(),
            md5_failed: HashMap::new(),
            md5_fetches: Vec::new(),
            language: RefCell::new("en".into()),
        }
    }

    fn read(&self, json: &str) -> Result<Listing, String> {
        let folder = self.folder.borrow().clone();
        let market = parse(json)?;
        let no_sum = texts().get(&self.language.borrow(), "no_sum");
        let referenced = move |cfg: &str| {
            let text = zlog_cfg::file::decode(
                &std::fs::read(zlog_cfg::file::find_file(&folder, cfg)?).ok()?,
            );
            Some(zlog_cfg::Config::parse(&text).dat).filter(|d| !d.is_empty())
        };
        Ok(market.listing(referenced, no_sum))
    }

    fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = &self.cache else {
            return;
        };
        let Ok(json) = std::fs::read_to_string(path) else {
            return;
        };
        if let Ok(l) = self.read(&json) {
            self.checked = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .map(date_of);
            self.list = Some(Ok(l));
            self.json = Some(json);
            self.validators = read_validators(&validators_path(path));
        }
    }

    fn fetched(&mut self, r: Result<zlog_net::Reply, String>) {
        if let (Ok(reply), Some(Ok(_))) = (&r, &self.list) {
            if reply.not_modified() {
                if let Some(path) = &self.cache {
                    let _ = std::fs::File::options()
                        .append(true)
                        .open(path)
                        .and_then(|f| f.set_modified(SystemTime::now()));
                }
                self.checked = Some(date_of(SystemTime::now()));
                return;
            }
        }
        let r = r.and_then(|reply| match reply.status {
            200 => Ok((reply.text(), reply.validators())),
            status => Err(format!("HTTP {status}")),
        });
        match r.and_then(|(json, v)| self.read(&json).map(|l| (json, v, l))) {
            Ok((json, validators, l)) => {
                if let Some(path) = &self.cache {
                    if write_atomic(path, json.as_bytes()).is_ok() {
                        let _ = write_validators(&validators_path(path), &validators);
                    }
                }
                self.validators = validators;
                self.checked = Some(date_of(SystemTime::now()));
                self.list = Some(Ok(l));
                self.json = Some(json);
            }
            Err(e) if !matches!(self.list, Some(Ok(_))) => self.list = Some(Err(e)),
            Err(_) => {}
        }
    }

    fn listed_sum(&self, url: &str) -> Option<&Sum> {
        match &self.list {
            Some(Ok(l)) => l.sums.get(url),
            _ => None,
        }
    }

    pub fn sum(&self, url: &str) -> Option<&str> {
        match self.listed_sum(url)? {
            Sum::Hex(h) => Some(h),
            Sum::Url(u) => md5_for(self.md5.get(u)?, url),
            Sum::Missing => None,
        }
    }

    fn fetch_md5(&mut self) {
        let Some(Ok(l)) = &self.list else {
            return;
        };
        let urls: std::collections::BTreeSet<String> = l
            .sums
            .values()
            .filter_map(|s| match s {
                Sum::Url(u) => Some(u.clone()),
                _ => None,
            })
            .filter(|u| {
                (!self.md5.contains_key(u) || self.md5_stale.contains(u))
                    && !self.md5_fetches.iter().any(|(f, _)| f == u)
                    && self
                        .md5_failed
                        .get(u)
                        .is_none_or(|t| t.elapsed() >= MD5_RETRY)
            })
            .collect();
        for url in urls {
            let job = (self.get)(zlog_net::Request::get(&url));
            self.md5_fetches.push((url, job));
        }
    }

    fn poll_md5(&mut self) {
        let mut done = Vec::new();
        self.md5_fetches.retain_mut(|(url, job)| match job.take() {
            Some(r) => {
                let hex = r
                    .ok()
                    .filter(|r| {
                        r.status == 200
                            && !r.content_type.to_ascii_lowercase().contains("text/html")
                    })
                    .and_then(|r| md5_entries(&r.text()));
                done.push((url.clone(), hex));
                false
            }
            None => true,
        });
        for (url, hex) in done {
            self.md5_stale.remove(&url);
            match hex {
                Some(hex) => {
                    self.md5_failed.remove(&url);
                    self.md5.insert(url, hex);
                }
                None => {
                    self.md5.remove(&url);
                    self.md5_failed.insert(url, Instant::now());
                }
            }
        }
    }

    pub fn differs_locally(&self, folder: &Path, file: &ListedFile) -> bool {
        let name = file.name.as_str();
        let Some(sum) = self.sum(&file.url) else {
            return false;
        };
        let path = zlog_cfg::file::find_file(folder, name).unwrap_or_else(|| folder.join(name));
        let Ok(meta) = std::fs::metadata(&path) else {
            return false;
        };
        let stamp = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        let mut hashes = self.hashes.borrow_mut();
        if let Some((s, h)) = hashes.get(&path) {
            if *s == stamp {
                return !h.eq_ignore_ascii_case(sum);
            }
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return false;
        };
        let h = checksum(&bytes);
        let out = !h.eq_ignore_ascii_case(sum);
        hashes.insert(path, (stamp, h));
        out
    }
}

impl ContestList for MarketList {
    fn id(&self) -> &str {
        "market"
    }

    fn title(&self, language: &str) -> String {
        if *self.language.borrow() != language {
            *self.language.borrow_mut() = language.to_string();
        }
        texts().get(language, "title").into()
    }

    fn page(&self) -> &str {
        PAGE_URL
    }

    fn last_checked(&self) -> Option<Date> {
        self.checked
    }

    fn stale(&self, _today: Date) -> bool {
        self.requested.is_none_or(|t| t.elapsed() >= MIN_REFRESH)
    }

    fn refresh(&mut self) {
        self.load();
        self.md5_stale.extend(self.md5.keys().cloned());
        self.md5_failed.clear();
        self.md5_fetches.clear();
        if self.fetch.is_some() {
            return;
        }
        self.requested = Some(Instant::now());
        if !matches!(self.list, Some(Ok(_))) {
            self.list = None;
        }
        let mut request = zlog_net::Request::get(MARKET_URL);
        if matches!(self.list, Some(Ok(_))) {
            request = request.conditional(&self.validators);
        }
        self.fetch = Some((self.get)(request));
    }

    fn outdated(
        &self,
        folder: &Path,
        _contest: &ListedContest,
        file: &ListedFile,
        _changed: Date,
    ) -> bool {
        if *self.folder.borrow() != folder {
            *self.folder.borrow_mut() = folder.to_path_buf();
            self.relist.set(true);
        }
        self.differs_locally(folder, file)
    }

    fn verify(&self, file: &ListedFile, bytes: &[u8], language: &str) -> Result<(), String> {
        match (self.listed_sum(&file.url), self.sum(&file.url)) {
            (_, Some(sum)) => verify(&file.name, bytes, sum, language),
            (Some(Sum::Url(_)), None) => {
                Err(texts().format(language, "sum_unknown", &[&file.name]))
            }
            (Some(_), None) => Ok(()),
            (None, None) => Err(texts().format(language, "sum_unknown", &[&file.name])),
        }
    }

    fn pending(&self) -> bool {
        self.fetch.is_some() || !self.md5_fetches.is_empty()
    }

    fn contests(&mut self) -> Option<Result<&[ListedContest], String>> {
        self.load();
        if let Some(r) = self.fetch.as_mut().and_then(|f| f.take()) {
            self.fetch = None;
            self.fetched(r);
        }
        if self.relist.take() {
            if let (Some(json), Some(Ok(_))) = (&self.json, &self.list) {
                if let Ok(l) = self.read(json) {
                    self.list = Some(Ok(l));
                }
            }
        }
        self.fetch_md5();
        self.poll_md5();
        match self.list.as_ref()? {
            Ok(l) => Some(Ok(&l.contests)),
            Err(e) => Some(Err(e.clone())),
        }
    }
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.contest_list(20, MarketList::default);
}

pub fn checksum(bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    hex::encode(Md5::digest(bytes))
}
