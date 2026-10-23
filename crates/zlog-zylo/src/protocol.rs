//! The pipe protocol between zLog RS (the host) and a helper process.
//!
//! Each message is one frame: a little-endian `u32` length, then that many bytes.
//! The payload starts with a tag byte; fields follow in order: integers little
//! endian, strings as a `u32` length and UTF-8 bytes, QSO records as their 256 raw
//! bytes (`TQSOData`, see [`crate::record`]). Both sides first send
//! [`ToHelper::Hello`] / [`ToHost::Hello`] with [`VERSION`]; a side that sees another
//! version stops.
//!
//! Requests that need an answer carry a sequence number, repeated in the answer
//! ([`ToHost::Reply`], [`ToHelper::Answer`]); everything else is one way.

use std::io::{self, Read, Write};

/// Version of this protocol; raised on any incompatible change.
pub const VERSION: u16 = 1;

/// Largest frame accepted, so a broken peer cannot make the other allocate gigabytes.
pub const MAX_FRAME: usize = 16 << 20;

/// A QSO record as ZyLO passes it.
pub type Record = [u8; 256];

/// From the host to the helper; one per ZyLO export the host calls.
#[derive(Debug, Clone, PartialEq)]
pub enum ToHelper {
    Hello {
        version: u16,
    },
    /// Load the plugin at `path` and call `zylo_launch_event`.
    Launch {
        path: String,
    },
    /// `zylo_offset_event`: zLog's `UTCOffset` (minutes, JST is -540).
    Offset {
        minutes: i32,
    },
    /// `zylo_attach_event` (contest name, CFG path).
    Attach {
        contest: String,
        cfg: String,
    },
    /// `zylo_assign_event`, only to the rule plugin of the contest.
    Assign {
        contest: String,
        cfg: String,
    },
    /// `zylo_detach_event`.
    Detach {
        contest: String,
        cfg: String,
    },
    /// `zylo_insert_event`; `depth` counts how many plugin changes led to this one.
    Insert {
        depth: u8,
        record: Record,
    },
    /// `zylo_delete_event`.
    Delete {
        depth: u8,
        record: Record,
    },
    /// `zylo_verify_event`; answered with [`Reply::Record`].
    Verify {
        seq: u32,
        record: Record,
    },
    /// `zylo_points_event`; answered with [`Reply::Int`].
    Points {
        seq: u32,
        points: i64,
        multis: i64,
    },
    /// `zylo_query_cities`; answered with [`Reply::Text`].
    Cities {
        seq: u32,
    },
    /// `zylo_import_event`; answered with [`Reply::Bool`].
    Import {
        seq: u32,
        source: String,
        target: String,
    },
    /// `zylo_export_event`; answered with [`Reply::Bool`].
    Export {
        seq: u32,
        target: String,
        format: String,
    },
    /// `zylo_button_event` (handler id, as zLog passes it twice).
    Button {
        id: i64,
    },
    /// `zylo_editor_event` (handler id, character code).
    Editor {
        id: i64,
        key: i64,
    },
    /// The host's answer to [`ToHost::Query`].
    Answer {
        seq: u32,
        text: String,
    },
    /// `zylo_finish_event`, then the helper exits.
    Finish,
}

/// What a plugin hooked with `zylo_allow_button` or `zylo_allow_editor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    Button,
    Editor,
}

/// The answer to a request of the host.
// Records are the usual answer; boxing them would only add allocations.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Record(Record),
    Int(i64),
    Text(String),
    Bool(bool),
    /// The plugin is not loaded, or the export is missing.
    Failed(String),
}

/// From the helper to the host.
#[derive(Debug, Clone, PartialEq)]
pub enum ToHost {
    Hello {
        version: u16,
    },
    /// Result of [`ToHelper::Launch`]: `error` is empty on success; `format` is the
    /// file-dialog filter from `zylo_query_format` (empty for none).
    Launched {
        error: String,
        format: String,
    },
    /// The plugin's insert, delete and update callbacks; `depth` is that of the event
    /// being handled when the plugin called (0 outside an event).
    Insert {
        depth: u8,
        record: Record,
    },
    Delete {
        depth: u8,
        record: Record,
    },
    Update {
        depth: u8,
        record: Record,
    },
    /// `zylo_allow_commit`: change the fields in `mask` of the QSO with this ID.
    Commit {
        depth: u8,
        record: Record,
        id: i32,
        mask: u64,
    },
    /// A modal message (`DisplayModal`).
    Dialog {
        text: String,
    },
    /// A toast (`DisplayToast`).
    Notify {
        text: String,
    },
    /// `zylo_allow_access`: the plugin asks for `{V}`, `{F}`, ...; answered with
    /// [`ToHelper::Answer`].
    Query {
        seq: u32,
        text: String,
    },
    /// The plugin hooked a component; events for it come back with this id.
    Hook {
        kind: HookKind,
        name: String,
        id: i64,
    },
    /// The plugin used a callback this host cannot provide (`handle`, `script`).
    Unsupported {
        what: String,
    },
    Reply {
        seq: u32,
        reply: Reply,
    },
}

/// Bytes being built into a payload.
#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i64(&mut self, v: i64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn str(&mut self, v: &str) -> &mut Self {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v.as_bytes());
        self
    }
    fn rec(&mut self, v: &Record) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
}

/// A payload being read.
struct In<'a>(&'a [u8]);

fn bad(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

impl In<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(bad("frame too short"));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| bad("string not UTF-8"))
    }
    fn rec(&mut self) -> io::Result<Record> {
        Ok(self.take(256)?.try_into().unwrap())
    }
    fn end(&self) -> io::Result<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(bad("frame too long"))
        }
    }
}

impl ToHelper {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out::default();
        match self {
            ToHelper::Hello { version } => o.u8(1).u16(*version),
            ToHelper::Launch { path } => o.u8(2).str(path),
            ToHelper::Offset { minutes } => o.u8(3).i32(*minutes),
            ToHelper::Attach { contest, cfg } => o.u8(4).str(contest).str(cfg),
            ToHelper::Assign { contest, cfg } => o.u8(5).str(contest).str(cfg),
            ToHelper::Detach { contest, cfg } => o.u8(6).str(contest).str(cfg),
            ToHelper::Insert { depth, record } => o.u8(7).u8(*depth).rec(record),
            ToHelper::Delete { depth, record } => o.u8(8).u8(*depth).rec(record),
            ToHelper::Verify { seq, record } => o.u8(9).u32(*seq).rec(record),
            ToHelper::Points {
                seq,
                points,
                multis,
            } => o.u8(10).u32(*seq).i64(*points).i64(*multis),
            ToHelper::Cities { seq } => o.u8(11).u32(*seq),
            ToHelper::Import {
                seq,
                source,
                target,
            } => o.u8(12).u32(*seq).str(source).str(target),
            ToHelper::Export {
                seq,
                target,
                format,
            } => o.u8(13).u32(*seq).str(target).str(format),
            ToHelper::Button { id } => o.u8(14).i64(*id),
            ToHelper::Editor { id, key } => o.u8(15).i64(*id).i64(*key),
            ToHelper::Answer { seq, text } => o.u8(16).u32(*seq).str(text),
            ToHelper::Finish => o.u8(17),
        };
        o.0
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut i = In(bytes);
        let m = match i.u8()? {
            1 => ToHelper::Hello { version: i.u16()? },
            2 => ToHelper::Launch { path: i.str()? },
            3 => ToHelper::Offset { minutes: i.i32()? },
            4 => ToHelper::Attach {
                contest: i.str()?,
                cfg: i.str()?,
            },
            5 => ToHelper::Assign {
                contest: i.str()?,
                cfg: i.str()?,
            },
            6 => ToHelper::Detach {
                contest: i.str()?,
                cfg: i.str()?,
            },
            7 => ToHelper::Insert {
                depth: i.u8()?,
                record: i.rec()?,
            },
            8 => ToHelper::Delete {
                depth: i.u8()?,
                record: i.rec()?,
            },
            9 => ToHelper::Verify {
                seq: i.u32()?,
                record: i.rec()?,
            },
            10 => ToHelper::Points {
                seq: i.u32()?,
                points: i.i64()?,
                multis: i.i64()?,
            },
            11 => ToHelper::Cities { seq: i.u32()? },
            12 => ToHelper::Import {
                seq: i.u32()?,
                source: i.str()?,
                target: i.str()?,
            },
            13 => ToHelper::Export {
                seq: i.u32()?,
                target: i.str()?,
                format: i.str()?,
            },
            14 => ToHelper::Button { id: i.i64()? },
            15 => ToHelper::Editor {
                id: i.i64()?,
                key: i.i64()?,
            },
            16 => ToHelper::Answer {
                seq: i.u32()?,
                text: i.str()?,
            },
            17 => ToHelper::Finish,
            _ => return Err(bad("unknown message")),
        };
        i.end()?;
        Ok(m)
    }
}

impl ToHost {
    pub fn encode(&self) -> Vec<u8> {
        let mut o = Out::default();
        match self {
            ToHost::Hello { version } => o.u8(1).u16(*version),
            ToHost::Launched { error, format } => o.u8(2).str(error).str(format),
            ToHost::Insert { depth, record } => o.u8(3).u8(*depth).rec(record),
            ToHost::Delete { depth, record } => o.u8(4).u8(*depth).rec(record),
            ToHost::Update { depth, record } => o.u8(5).u8(*depth).rec(record),
            ToHost::Commit {
                depth,
                record,
                id,
                mask,
            } => o.u8(6).u8(*depth).rec(record).i32(*id).u64(*mask),
            ToHost::Dialog { text } => o.u8(7).str(text),
            ToHost::Notify { text } => o.u8(8).str(text),
            ToHost::Query { seq, text } => o.u8(9).u32(*seq).str(text),
            ToHost::Hook { kind, name, id } => o
                .u8(10)
                .u8(matches!(kind, HookKind::Editor) as u8)
                .str(name)
                .i64(*id),
            ToHost::Unsupported { what } => o.u8(11).str(what),
            ToHost::Reply { seq, reply } => {
                o.u8(12).u32(*seq);
                match reply {
                    Reply::Record(r) => o.u8(0).rec(r),
                    Reply::Int(v) => o.u8(1).i64(*v),
                    Reply::Text(t) => o.u8(2).str(t),
                    Reply::Bool(b) => o.u8(3).u8(*b as u8),
                    Reply::Failed(e) => o.u8(4).str(e),
                }
            }
        };
        o.0
    }

    pub fn decode(bytes: &[u8]) -> io::Result<Self> {
        let mut i = In(bytes);
        let m = match i.u8()? {
            1 => ToHost::Hello { version: i.u16()? },
            2 => ToHost::Launched {
                error: i.str()?,
                format: i.str()?,
            },
            3 => ToHost::Insert {
                depth: i.u8()?,
                record: i.rec()?,
            },
            4 => ToHost::Delete {
                depth: i.u8()?,
                record: i.rec()?,
            },
            5 => ToHost::Update {
                depth: i.u8()?,
                record: i.rec()?,
            },
            6 => ToHost::Commit {
                depth: i.u8()?,
                record: i.rec()?,
                id: i.i32()?,
                mask: i.u64()?,
            },
            7 => ToHost::Dialog { text: i.str()? },
            8 => ToHost::Notify { text: i.str()? },
            9 => ToHost::Query {
                seq: i.u32()?,
                text: i.str()?,
            },
            10 => ToHost::Hook {
                kind: if i.u8()? == 0 {
                    HookKind::Button
                } else {
                    HookKind::Editor
                },
                name: i.str()?,
                id: i.i64()?,
            },
            11 => ToHost::Unsupported { what: i.str()? },
            12 => {
                let seq = i.u32()?;
                let reply = match i.u8()? {
                    0 => Reply::Record(i.rec()?),
                    1 => Reply::Int(i.i64()?),
                    2 => Reply::Text(i.str()?),
                    3 => Reply::Bool(i.u8()? != 0),
                    4 => Reply::Failed(i.str()?),
                    _ => return Err(bad("unknown reply")),
                };
                ToHost::Reply { seq, reply }
            }
            _ => return Err(bad("unknown message")),
        };
        i.end()?;
        Ok(m)
    }
}

/// Write one frame.
pub fn write_frame(w: &mut dyn Write, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Read one frame; `Ok(None)` at the end of the stream between frames.
pub fn read_frame(r: &mut dyn Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(bad("frame too large"));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(b: u8) -> Record {
        let mut r = [0u8; 256];
        r[0] = b;
        r[255] = b.wrapping_add(1);
        r
    }

    #[test]
    fn every_message_round_trips() {
        let to_helper = [
            ToHelper::Hello { version: VERSION },
            ToHelper::Launch {
                path: "C:\\プラグイン\\a.dll".into(),
            },
            ToHelper::Offset { minutes: -540 },
            ToHelper::Attach {
                contest: "ALL JA".into(),
                cfg: "a.cfg".into(),
            },
            ToHelper::Assign {
                contest: "x".into(),
                cfg: String::new(),
            },
            ToHelper::Detach {
                contest: "y".into(),
                cfg: "z".into(),
            },
            ToHelper::Insert {
                depth: 3,
                record: rec(1),
            },
            ToHelper::Delete {
                depth: 0,
                record: rec(2),
            },
            ToHelper::Verify {
                seq: 7,
                record: rec(3),
            },
            ToHelper::Points {
                seq: 8,
                points: -1,
                multis: i64::MAX,
            },
            ToHelper::Cities { seq: 9 },
            ToHelper::Import {
                seq: 10,
                source: "s".into(),
                target: "t".into(),
            },
            ToHelper::Export {
                seq: 11,
                target: "t".into(),
                format: "ADIF".into(),
            },
            ToHelper::Button { id: 4 },
            ToHelper::Editor { id: 5, key: 65 },
            ToHelper::Answer {
                seq: 12,
                text: "JA1ZLO".into(),
            },
            ToHelper::Finish,
        ];
        for m in to_helper {
            assert_eq!(ToHelper::decode(&m.encode()).unwrap(), m);
        }
        let to_host = [
            ToHost::Hello { version: VERSION },
            ToHost::Launched {
                error: String::new(),
                format: "ADIF (*.adi)|*.adi".into(),
            },
            ToHost::Insert {
                depth: 1,
                record: rec(4),
            },
            ToHost::Delete {
                depth: 2,
                record: rec(5),
            },
            ToHost::Update {
                depth: 0,
                record: rec(6),
            },
            ToHost::Commit {
                depth: 0,
                record: rec(7),
                id: 1234,
                mask: 0x7f,
            },
            ToHost::Dialog {
                text: "日本".into(),
            },
            ToHost::Notify { text: "n".into() },
            ToHost::Query {
                seq: 1,
                text: "{V}".into(),
            },
            ToHost::Hook {
                kind: HookKind::Editor,
                name: "MainForm.CallsignEdit".into(),
                id: 2,
            },
            ToHost::Hook {
                kind: HookKind::Button,
                name: "b".into(),
                id: 3,
            },
            ToHost::Unsupported {
                what: "script".into(),
            },
            ToHost::Reply {
                seq: 2,
                reply: Reply::Record(rec(8)),
            },
            ToHost::Reply {
                seq: 3,
                reply: Reply::Int(-5),
            },
            ToHost::Reply {
                seq: 4,
                reply: Reply::Text("100 札幌".into()),
            },
            ToHost::Reply {
                seq: 5,
                reply: Reply::Bool(true),
            },
            ToHost::Reply {
                seq: 6,
                reply: Reply::Failed("no".into()),
            },
        ];
        for m in to_host {
            assert_eq!(ToHost::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn broken_payloads_are_errors_not_panics() {
        assert!(ToHelper::decode(&[]).is_err());
        assert!(ToHelper::decode(&[99]).is_err());
        assert!(ToHelper::decode(&[7, 0, 1, 2]).is_err());
        let mut long = ToHelper::Finish.encode();
        long.push(0);
        assert!(ToHelper::decode(&long).is_err());
        // A string length past the end.
        assert!(ToHost::decode(&[7, 255, 255, 255, 255]).is_err());
        assert!(ToHost::decode(&[8, 2, 0, 0, 0, 0xff, 0xfe]).is_err());
    }

    #[test]
    fn frames_round_trip_and_end_cleanly() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"abc").unwrap();
        write_frame(&mut buf, b"").unwrap();
        let mut r = &buf[..];
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), b"abc");
        assert_eq!(read_frame(&mut r).unwrap().unwrap(), b"");
        assert!(read_frame(&mut r).unwrap().is_none());
        // Cut inside a frame, and an oversized length.
        let mut r = &buf[..5];
        assert!(read_frame(&mut r).is_err());
        let huge = (MAX_FRAME as u32 + 1).to_le_bytes();
        assert!(read_frame(&mut &huge[..]).is_err());
    }
}
