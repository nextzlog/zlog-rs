use std::collections::{HashMap, HashSet};
use std::ffi::{c_char, c_long, c_void, CStr};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::library::Library;
use crate::protocol::{
    read_frame, write_frame, HookKind, Record, Reply, ToHelper, ToHost, VERSION,
};
use crate::record::{from_plugin, query_answer, to_plugin, RESPONSE_CAPACITY};

const QUERY_TIMEOUT: Duration = Duration::from_secs(2);

const FINISH_GRACE: Duration = Duration::from_secs(1);

struct Io {
    out: Mutex<Box<dyn Write + Send>>,
    queries: Mutex<HashMap<u32, mpsc::Sender<String>>>,
    seq: AtomicU32,
    depth: AtomicU8,
    hook_id: AtomicI64,
    scratch: Mutex<Option<String>>,
    unsupported: Mutex<HashSet<&'static str>>,
}

impl Io {
    fn send(&self, m: &ToHost) {
        let mut out = self.out.lock().unwrap();
        let _ = write_frame(&mut **out, &m.encode());
    }

    fn unsupported(&self, what: &'static str) {
        if self.unsupported.lock().unwrap().insert(what) {
            self.send(&ToHost::Unsupported { what: what.into() });
        }
    }
}

static IO: Mutex<Option<Arc<Io>>> = Mutex::new(None);

fn io() -> Option<Arc<Io>> {
    IO.lock().unwrap().clone()
}

type Str = *const c_char;
type Ptr = *const c_void;

struct Plugin {
    _lib: Library,
    query_cities: extern "C" fn(extern "C" fn(Str)),
    launch: extern "C" fn() -> u8,
    finish: extern "C" fn() -> u8,
    #[cfg_attr(not(windows), allow(dead_code))]
    window: extern "C" fn(usize),
    import: extern "C" fn(Str, Str) -> u8,
    export: extern "C" fn(Str, Str) -> u8,
    attach: extern "C" fn(Str, Str),
    assign: extern "C" fn(Str, Str),
    detach: extern "C" fn(Str, Str),
    offset: extern "C" fn(i64),
    insert: extern "C" fn(usize),
    delete: extern "C" fn(usize),
    verify: extern "C" fn(usize),
    points: extern "C" fn(i64, i64) -> i64,
    button: extern "C" fn(i64, i64),
    editor: extern "C" fn(i64, i64),
}

fn offer_callbacks(lib: &Library) {
    let allow = |name: &str, cb: Ptr| {
        if let Some(p) = lib.symbol(name) {
            // SAFETY: every `zylo_allow_*` export takes one function pointer.
            let f: extern "C" fn(Ptr) = unsafe { std::mem::transmute(p) };
            f(cb);
        }
    };
    allow("zylo_allow_insert", insert_cb as Ptr);
    allow("zylo_allow_delete", delete_cb as Ptr);
    allow("zylo_allow_update", update_cb as Ptr);
    allow("zylo_allow_dialog", dialog_cb as Ptr);
    allow("zylo_allow_notify", notify_cb as Ptr);
    allow("zylo_allow_access", access_cb as Ptr);
    allow("zylo_allow_handle", handle_cb as Ptr);
    allow("zylo_allow_button", button_cb as Ptr);
    allow("zylo_allow_editor", editor_cb as Ptr);
    allow("zylo_allow_script", script_cb as Ptr);
    allow("zylo_allow_commit", commit_cb as Ptr);
}

pub const REQUIRED: [&str; 16] = [
    "zylo_query_cities",
    "zylo_launch_event",
    "zylo_finish_event",
    "zylo_window_event",
    "zylo_import_event",
    "zylo_export_event",
    "zylo_attach_event",
    "zylo_assign_event",
    "zylo_detach_event",
    "zylo_offset_event",
    "zylo_insert_event",
    "zylo_delete_event",
    "zylo_verify_event",
    "zylo_points_event",
    "zylo_button_event",
    "zylo_editor_event",
];

impl Plugin {
    #[allow(clippy::missing_transmute_annotations)]
    fn load(path: &Path, io: &Io) -> Result<(Plugin, String), String> {
        let lib = Library::open(path)?;
        let missing: Vec<&str> = REQUIRED
            .iter()
            .copied()
            .filter(|n| lib.symbol(n).is_none())
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "{}: not a ZyLO plugin, missing {}",
                path.display(),
                missing.join(", ")
            ));
        }
        macro_rules! need {
            ($name:literal) => {{
                // SAFETY: checked above; the export has the signature of the contract.
                unsafe { std::mem::transmute::<Ptr, _>(lib.symbol($name).unwrap()) }
            }};
        }
        let plugin = Plugin {
            query_cities: need!("zylo_query_cities"),
            launch: need!("zylo_launch_event"),
            finish: need!("zylo_finish_event"),
            window: need!("zylo_window_event"),
            import: need!("zylo_import_event"),
            export: need!("zylo_export_event"),
            attach: need!("zylo_attach_event"),
            assign: need!("zylo_assign_event"),
            detach: need!("zylo_detach_event"),
            offset: need!("zylo_offset_event"),
            insert: need!("zylo_insert_event"),
            delete: need!("zylo_delete_event"),
            verify: need!("zylo_verify_event"),
            points: need!("zylo_points_event"),
            button: need!("zylo_button_event"),
            editor: need!("zylo_editor_event"),
            _lib: lib,
        };
        offer_callbacks(&plugin._lib);
        let mut format = String::new();
        if let Some(p) = plugin._lib.symbol("zylo_query_format") {
            // SAFETY: `zylo_query_format` takes the format callback.
            let f: extern "C" fn(extern "C" fn(Str)) = unsafe { std::mem::transmute(p) };
            f(scratch_cb);
            format = io.scratch.lock().unwrap().take().unwrap_or_default();
        }
        if (plugin.launch)() == 0 {
            return Err(format!("{}: zLog is too old for it", path.display()));
        }
        Ok((plugin, format))
    }

    fn handle(&self, m: ToHelper, io: &Io) {
        let reply = |seq, reply| io.send(&ToHost::Reply { seq, reply });
        match m {
            ToHelper::Offset { minutes } => (self.offset)(minutes as i64),
            ToHelper::Attach { contest, cfg } => {
                let (a, b) = (to_plugin(&contest), to_plugin(&cfg));
                (self.attach)(a.as_ptr(), b.as_ptr());
            }
            ToHelper::Assign { contest, cfg } => {
                let (a, b) = (to_plugin(&contest), to_plugin(&cfg));
                (self.assign)(a.as_ptr(), b.as_ptr());
            }
            ToHelper::Detach { contest, cfg } => {
                let (a, b) = (to_plugin(&contest), to_plugin(&cfg));
                (self.detach)(a.as_ptr(), b.as_ptr());
            }
            ToHelper::Insert { depth, mut record } => {
                io.depth.store(depth.saturating_add(1), Ordering::SeqCst);
                (self.insert)(record.as_mut_ptr() as usize);
                io.depth.store(0, Ordering::SeqCst);
            }
            ToHelper::Delete { depth, mut record } => {
                io.depth.store(depth.saturating_add(1), Ordering::SeqCst);
                (self.delete)(record.as_mut_ptr() as usize);
                io.depth.store(0, Ordering::SeqCst);
            }
            ToHelper::Verify { seq, mut record } => {
                (self.verify)(record.as_mut_ptr() as usize);
                reply(seq, Reply::Record(record));
            }
            ToHelper::Points {
                seq,
                points,
                multis,
            } => reply(seq, Reply::Int((self.points)(points, multis))),
            ToHelper::Cities { seq } => {
                (self.query_cities)(scratch_cb);
                let text = io.scratch.lock().unwrap().take().unwrap_or_default();
                reply(seq, Reply::Text(text));
            }
            ToHelper::Import {
                seq,
                source,
                target,
            } => {
                let (a, b) = (to_plugin(&source), to_plugin(&target));
                reply(seq, Reply::Bool((self.import)(a.as_ptr(), b.as_ptr()) != 0));
            }
            ToHelper::Export {
                seq,
                target,
                format,
            } => {
                let (a, b) = (to_plugin(&target), to_plugin(&format));
                reply(seq, Reply::Bool((self.export)(a.as_ptr(), b.as_ptr()) != 0));
            }
            ToHelper::Button { id } => (self.button)(id, id),
            ToHelper::Editor { id, key } => (self.editor)(id, key),
            ToHelper::Hello { .. }
            | ToHelper::Launch { .. }
            | ToHelper::Answer { .. }
            | ToHelper::Finish => {}
        }
    }
}

fn refuse(m: &ToHelper, io: &Io, why: &str) {
    let seq = match m {
        ToHelper::Verify { seq, .. }
        | ToHelper::Points { seq, .. }
        | ToHelper::Cities { seq }
        | ToHelper::Import { seq, .. }
        | ToHelper::Export { seq, .. } => *seq,
        _ => return,
    };
    io.send(&ToHost::Reply {
        seq,
        reply: Reply::Failed(why.into()),
    });
}

pub fn run(mut input: Box<dyn Read + Send>, output: Box<dyn Write + Send>, exit: bool) {
    let io = Arc::new(Io {
        out: Mutex::new(output),
        queries: Mutex::new(HashMap::new()),
        seq: AtomicU32::new(0),
        depth: AtomicU8::new(0),
        hook_id: AtomicI64::new(0),
        scratch: Mutex::new(None),
        unsupported: Mutex::new(HashSet::new()),
    });
    *IO.lock().unwrap() = Some(io.clone());
    io.send(&ToHost::Hello { version: VERSION });
    let (tx, rx) = mpsc::channel::<ToHelper>();
    let reader_io = io.clone();
    std::thread::spawn(move || {
        while let Ok(Some(frame)) = read_frame(&mut input) {
            match ToHelper::decode(&frame) {
                Ok(ToHelper::Answer { seq, text }) => {
                    if let Some(w) = reader_io.queries.lock().unwrap().remove(&seq) {
                        let _ = w.send(text);
                    }
                }
                Ok(m) => {
                    if tx.send(m).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut plugin: Option<Plugin> = None;
    let mut failed = String::from("not launched");
    let mut hello = false;
    while let Some(m) = next_message(&rx, plugin.as_ref()) {
        match m {
            ToHelper::Hello { version } => {
                if version != VERSION {
                    break;
                }
                hello = true;
            }
            _ if !hello => break,
            ToHelper::Launch { path } if plugin.is_none() => {
                let (error, format) = match Plugin::load(Path::new(&path), &io) {
                    Ok((p, format)) => {
                        plugin = Some(p);
                        (String::new(), format)
                    }
                    Err(e) => {
                        failed = e.clone();
                        (e, String::new())
                    }
                };
                io.send(&ToHost::Launched { error, format });
            }
            ToHelper::Finish => break,
            m => match &plugin {
                Some(p) => p.handle(m, &io),
                None => refuse(&m, &io, &failed),
            },
        }
    }
    if exit {
        std::thread::spawn(|| {
            std::thread::sleep(FINISH_GRACE);
            std::process::exit(0);
        });
    }
    if let Some(p) = &plugin {
        (p.finish)();
    }
    *IO.lock().unwrap() = None;
    if exit {
        std::process::exit(0);
    }
}

#[cfg(windows)]
fn next_message(rx: &mpsc::Receiver<ToHelper>, plugin: Option<&Plugin>) -> Option<ToHelper> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    loop {
        match rx.recv_timeout(Duration::from_millis(10)) {
            Ok(m) => return Some(m),
            Err(mpsc::RecvTimeoutError::Disconnected) => return None,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // SAFETY: a zeroed MSG filled by PeekMessageW, used within this loop only.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if let Some(p) = plugin {
                    (p.window)(&msg as *const MSG as usize);
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

#[cfg(not(windows))]
fn next_message(rx: &mpsc::Receiver<ToHelper>, _plugin: Option<&Plugin>) -> Option<ToHelper> {
    rx.recv().ok()
}

pub fn main() -> ! {
    run(
        Box::new(std::io::stdin()),
        Box::new(std::io::stdout()),
        true,
    );
    std::process::exit(0)
}

fn read_record(p: Ptr) -> Option<Record> {
    if p.is_null() {
        return None;
    }
    // SAFETY: ZyLO passes a pointer to a whole 256-byte `TQSOData`.
    Some(unsafe { std::ptr::read_unaligned(p as *const Record) })
}

fn read_string(p: Str) -> Option<Vec<u8>> {
    if p.is_null() {
        return None;
    }
    // SAFETY: ZyLO passes NUL-terminated strings.
    Some(unsafe { CStr::from_ptr(p) }.to_bytes().to_vec())
}

fn depth(io: &Io) -> u8 {
    io.depth.load(Ordering::SeqCst)
}

extern "C" fn insert_cb(p: Ptr) {
    if let (Some(io), Some(record)) = (io(), read_record(p)) {
        io.send(&ToHost::Insert {
            depth: depth(&io),
            record,
        });
    }
}

extern "C" fn delete_cb(p: Ptr) {
    if let (Some(io), Some(record)) = (io(), read_record(p)) {
        io.send(&ToHost::Delete {
            depth: depth(&io),
            record,
        });
    }
}

extern "C" fn update_cb(p: Ptr) {
    if let (Some(io), Some(record)) = (io(), read_record(p)) {
        io.send(&ToHost::Update {
            depth: depth(&io),
            record,
        });
    }
}

extern "C" fn commit_cb(p: Ptr, id: i32, mask: u64) {
    if let (Some(io), Some(record)) = (io(), read_record(p)) {
        io.send(&ToHost::Commit {
            depth: depth(&io),
            record,
            id,
            mask,
        });
    }
}

extern "C" fn dialog_cb(p: Str) {
    if let (Some(io), Some(s)) = (io(), read_string(p)) {
        io.send(&ToHost::Dialog {
            text: from_plugin(&s),
        });
    }
}

extern "C" fn notify_cb(p: Str) {
    if let (Some(io), Some(s)) = (io(), read_string(p)) {
        io.send(&ToHost::Notify {
            text: from_plugin(&s),
        });
    }
}

extern "C" fn access_cb(p: *mut c_char) {
    let (Some(io), Some(q)) = (io(), read_string(p)) else {
        return;
    };
    let seq = io.seq.fetch_add(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    io.queries.lock().unwrap().insert(seq, tx);
    io.send(&ToHost::Query {
        seq,
        text: from_plugin(&q),
    });
    let answer = rx.recv_timeout(QUERY_TIMEOUT).unwrap_or_default();
    io.queries.lock().unwrap().remove(&seq);
    let bytes = query_answer(&answer);
    debug_assert!(bytes.len() <= RESPONSE_CAPACITY);
    // SAFETY: the contract says the buffer holds `RESPONSE_CAPACITY` + 1 bytes.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
        *(p as *mut u8).add(bytes.len()) = 0;
    }
}

extern "C" fn handle_cb(_: Str) -> c_long {
    if let Some(io) = io() {
        io.unsupported("GetUI");
    }
    0
}

extern "C" fn script_cb(_: Str) -> c_long {
    if let Some(io) = io() {
        io.unsupported("RunDelphi");
    }
    0
}

fn hook(kind: HookKind, p: Str) -> c_long {
    let Some(io) = io() else {
        return 0;
    };
    let id = io.hook_id.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(name) = read_string(p) {
        io.send(&ToHost::Hook {
            kind,
            name: from_plugin(&name),
            id,
        });
    }
    id as c_long
}

extern "C" fn button_cb(p: Str) -> c_long {
    hook(HookKind::Button, p)
}

extern "C" fn editor_cb(p: Str) -> c_long {
    hook(HookKind::Editor, p)
}

extern "C" fn scratch_cb(p: Str) {
    if let (Some(io), Some(s)) = (io(), read_string(p)) {
        *io.scratch.lock().unwrap() = Some(from_plugin(&s));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::record::{from_record, to_record};
    use std::process::Command;
    use zlog_core::Qso;

    const PLUGIN: &str = r#"
#include <string.h>
#include <stdint.h>
typedef void (*rec_cb)(void*);
typedef void (*str_cb)(char*);
typedef long (*hook_cb)(char*);
static rec_cb ins; static str_cb toast; static str_cb acc; static hook_cb btn;
static int inserted;
void zylo_allow_insert(rec_cb f) { ins = f; }
void zylo_allow_notify(str_cb f) { toast = f; }
void zylo_allow_access(str_cb f) { acc = f; }
void zylo_allow_button(hook_cb f) { btn = f; }
void zylo_query_format(str_cb f) { f("Test (*.tst)|*.tst"); }
void zylo_query_cities(str_cb f) { f("100 Sapporo\n101 Asahikawa"); }
_Bool zylo_launch_event(void) { if (btn) btn("MainForm.PluginMenu"); return 1; }
_Bool zylo_finish_event(void) { return 0; }
void zylo_window_event(uintptr_t m) {}
_Bool zylo_import_event(char* s, char* t) { return 0; }
_Bool zylo_export_event(char* t, char* f) { return strcmp(f, "Test") == 0; }
void zylo_attach_event(char* c, char* f) { if (toast) toast(c); }
void zylo_assign_event(char* c, char* f) {}
void zylo_detach_event(char* c, char* f) {}
void zylo_offset_event(int64_t m) {}
void zylo_insert_event(uintptr_t p) {
  if (inserted++ || !ins || !acc) return;
  unsigned char q[256]; char buf[257] = "{C}";
  memcpy(q, (void*)p, 256);
  acc(buf);
  size_t n = strlen(buf); q[175] = (unsigned char)n; memcpy(q + 176, buf, n);
  ins(q);
}
void zylo_delete_event(uintptr_t p) {}
void zylo_verify_event(uintptr_t p) {
  unsigned char* q = (unsigned char*)p;
  q[159] = 2; memcpy(q + 95, q + 52, 31);
}
int64_t zylo_points_event(int64_t p, int64_t m) { return p * m; }
void zylo_button_event(int64_t a, int64_t b) { if (toast) toast("clicked"); }
void zylo_editor_event(int64_t a, int64_t b) {}
"#;

    fn build_plugin() -> Option<std::path::PathBuf> {
        let dir = std::env::temp_dir().join(format!("zlog-zylo-c-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let src = dir.join("plugin.c");
        std::fs::write(&src, PLUGIN).ok()?;
        let lib = dir.join(format!("test.{}", crate::library::EXTENSION));
        let ok = Command::new("cc")
            .args(["-shared", "-fPIC", "-o"])
            .arg(&lib)
            .arg(&src)
            .status()
            .ok()?
            .success();
        ok.then_some(lib)
    }

    fn send(w: &mut impl Write, m: ToHelper) {
        write_frame(w, &m.encode()).unwrap();
    }

    fn recv(r: &mut impl Read) -> ToHost {
        ToHost::decode(&read_frame(r).unwrap().unwrap()).unwrap()
    }

    #[test]
    fn helper_drives_a_c_plugin() {
        let Some(lib) = build_plugin() else {
            eprintln!("no C compiler; skipped");
            return;
        };
        let (host_r, helper_w) = std::io::pipe().unwrap();
        let (helper_r, mut host_w) = std::io::pipe().unwrap();
        let helper = std::thread::spawn(move || run(Box::new(helper_r), Box::new(helper_w), false));
        let mut r = host_r;
        assert_eq!(recv(&mut r), ToHost::Hello { version: VERSION });
        send(&mut host_w, ToHelper::Hello { version: VERSION });
        send(
            &mut host_w,
            ToHelper::Launch {
                path: lib.display().to_string(),
            },
        );
        assert_eq!(
            recv(&mut r),
            ToHost::Hook {
                kind: HookKind::Button,
                name: "MainForm.PluginMenu".into(),
                id: 1
            }
        );
        assert_eq!(
            recv(&mut r),
            ToHost::Launched {
                error: String::new(),
                format: "Test (*.tst)|*.tst".into()
            }
        );
        send(
            &mut host_w,
            ToHelper::Attach {
                contest: "ALL JA".into(),
                cfg: String::new(),
            },
        );
        assert_eq!(
            recv(&mut r),
            ToHost::Notify {
                text: "ALL JA".into()
            }
        );
        let q = Qso {
            callsign: "JA1ZLO".into(),
            nr_rcvd: "100".into(),
            ..Qso::default()
        };
        send(
            &mut host_w,
            ToHelper::Insert {
                depth: 0,
                record: to_record(&q),
            },
        );
        let ToHost::Query { seq, text } = recv(&mut r) else {
            panic!()
        };
        assert_eq!(text, "{C}");
        send(
            &mut host_w,
            ToHelper::Answer {
                seq,
                text: "JA1YAA".into(),
            },
        );
        let ToHost::Insert { depth, record } = recv(&mut r) else {
            panic!()
        };
        assert_eq!(depth, 1);
        let back = from_record(&record);
        assert_eq!(
            (back.callsign.as_str(), back.memo.as_str()),
            ("JA1ZLO", "JA1YAA")
        );
        send(
            &mut host_w,
            ToHelper::Verify {
                seq: 1,
                record: to_record(&q),
            },
        );
        let ToHost::Reply {
            seq: 1,
            reply: Reply::Record(v),
        } = recv(&mut r)
        else {
            panic!()
        };
        let v = from_record(&v);
        assert_eq!((v.points, v.multi1.as_str()), (2, "100"));
        send(
            &mut host_w,
            ToHelper::Points {
                seq: 2,
                points: 6,
                multis: 7,
            },
        );
        assert_eq!(
            recv(&mut r),
            ToHost::Reply {
                seq: 2,
                reply: Reply::Int(42)
            }
        );
        send(&mut host_w, ToHelper::Cities { seq: 3 });
        assert_eq!(
            recv(&mut r),
            ToHost::Reply {
                seq: 3,
                reply: Reply::Text("100 Sapporo\n101 Asahikawa".into())
            }
        );
        send(
            &mut host_w,
            ToHelper::Export {
                seq: 4,
                target: "x".into(),
                format: "Test".into(),
            },
        );
        assert_eq!(
            recv(&mut r),
            ToHost::Reply {
                seq: 4,
                reply: Reply::Bool(true)
            }
        );
        send(&mut host_w, ToHelper::Button { id: 1 });
        assert_eq!(
            recv(&mut r),
            ToHost::Notify {
                text: "clicked".into()
            }
        );
        send(&mut host_w, ToHelper::Finish);
        helper.join().unwrap();
    }

    #[test]
    fn helper_reports_a_plugin_it_cannot_load_and_refuses_requests() {
        let (host_r, helper_w) = std::io::pipe().unwrap();
        let (helper_r, mut host_w) = std::io::pipe().unwrap();
        let helper = std::thread::spawn(move || run(Box::new(helper_r), Box::new(helper_w), false));
        let mut r = host_r;
        assert_eq!(recv(&mut r), ToHost::Hello { version: VERSION });
        send(&mut host_w, ToHelper::Hello { version: VERSION });
        send(
            &mut host_w,
            ToHelper::Launch {
                path: "/no/such.so".into(),
            },
        );
        let ToHost::Launched { error, .. } = recv(&mut r) else {
            panic!()
        };
        assert!(!error.is_empty());
        send(&mut host_w, ToHelper::Cities { seq: 9 });
        assert!(matches!(
            recv(&mut r),
            ToHost::Reply {
                seq: 9,
                reply: Reply::Failed(_)
            }
        ));
        drop(host_w);
        helper.join().unwrap();
    }
}
