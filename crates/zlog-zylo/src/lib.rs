//! ZyLO plugin support: zLog's plugins, shared libraries implementing the ZyLO API,
//! run by zLog RS (UzLogExtension.pas).
//!
//! Each enabled plugin runs in a helper process of its own, this same executable
//! started with [`SUBCOMMAND`] ([`helper_main`]), so a plugin that crashes or hangs
//! is disabled with a warning and never takes the log or the other plugins down.
//! Plugins start in the background after the window is up; a plugin marked "rules
//! only" starts only while a contest whose CFG names it is open.
//!
//! - [`protocol`]: the frames between zLog RS and a helper.
//! - [`helper`]: the helper, which loads the library and calls it.
//! - [`library`]: loading a `.dll` (Windows), `.dylib` (macOS) or `.so` (Linux).
//! - [`host`]: one helper seen from zLog RS.
//! - [`manager`]: the enabled and running plugins, and their settings.
//! - [`link`]: log changes to the plugins, plugin requests to the book.
//! - [`rules`]: rule plugins named by a CFG `dll` line.
//! - [`format`]: file formats of plugins.
//! - [`panel`]: the plugin window.
//!
//! # The ZyLO contract
//!
//! A plugin is any shared library, in any language, with the C ABI exports below;
//! this is the only test. A library missing a required export is refused, naming
//! every missing one; each optional export is used when present. Nothing is assumed
//! about how the library was built.
//!
//! Types: `int64` is a 64-bit signed integer, `bool` one byte (0 false), `str` a
//! NUL-terminated `char*`, `rec` a pointer to a 256-byte QSO record.
//!
//! Required exports (zLog's `MustGetProc`):
//!
//! | export | signature | called |
//! |---|---|---|
//! | `zylo_launch_event` | `bool()` | once after loading; false refuses to run |
//! | `zylo_finish_event` | `bool()` | before the helper ends |
//! | `zylo_window_event` | `void(uintptr msg)` | Windows only: each message of the helper's loop (`MSG*`) |
//! | `zylo_offset_event` | `void(int64 minutes)` | contest opened: minus the offset from UTC (JST -540) |
//! | `zylo_attach_event` | `void(str contest, str cfg)` | contest opened |
//! | `zylo_assign_event` | `void(str contest, str cfg)` | to the rule plugin of the contest |
//! | `zylo_detach_event` | `void(str contest, str cfg)` | contest closed |
//! | `zylo_insert_event` | `void(rec)` | a QSO was added (an edit: delete, then insert) |
//! | `zylo_delete_event` | `void(rec)` | a QSO was removed |
//! | `zylo_verify_event` | `void(rec)` | rule plugin: set points (byte 159), multipliers; an empty first multiplier is invalid |
//! | `zylo_points_event` | `int64(int64 points, int64 multis)` | rule plugin: the total score |
//! | `zylo_query_cities` | `void(void(*)(str))` | rule plugin: calls back with the multiplier list (DAT text) |
//! | `zylo_import_event` | `bool(str source, str target)` | convert `source` into the ZLO file `target` |
//! | `zylo_export_event` | `bool(str target, str format)` | convert the ZLO file `target` in place |
//! | `zylo_button_event` | `void(int64 id, int64 id)` | a hooked button was clicked |
//! | `zylo_editor_event` | `void(int64 id, int64 key)` | a character was typed into a hooked field |
//!
//! Optional exports, each taking one callback, called once after loading and before
//! `zylo_launch_event`:
//!
//! | export | callback | what zLog RS does |
//! |---|---|---|
//! | `zylo_allow_insert` / `_delete` / `_update` | `void(rec)` | queue the change for the book (by QSO ID) |
//! | `zylo_allow_commit` | `void(rec, int32 id, uint64 mask)` | change the masked fields of QSO `id` (bits: call, sent, rcvd, name, note, mul1, mul2) |
//! | `zylo_allow_dialog` / `_notify` | `void(str)` | show the text (status bar and plugin window); returns at once |
//! | `zylo_allow_access` | `void(char* buf)` | replace the query in `buf` (`{V}` `{F}` `{C}` `{B}` `{M}`) by the answer |
//! | `zylo_allow_button` / `_editor` | `long(str name)` | hook a component; returns the handler id |
//! | `zylo_allow_handle` / `_script` | `long(str)` | not available (Win32 handles, Delphi scripts): returns 0 |
//! | `zylo_query_format` | `void(void(*)(str filter))` | the plugin's file formats (`Name|*.ext;...`) |
//!
//! Records are zLog's `TQSOData` (the 256-byte ZLO record, [`record`]), strings in
//! records Shift_JIS. Strings to the plugin are Shift_JIS, strings from it UTF-8.
//! Ownership: every pointer passed either way is valid only during the call; both
//! sides copy what they keep. The query buffer holds 257 bytes; the answer is at
//! most 256 bytes and a NUL. Exports are called from one thread; callbacks may be
//! called from any thread of the plugin, at any time, and never re-enter zLog RS.
//! A library is never unloaded.

pub mod format;
pub mod helper;
pub mod host;
pub mod library;
pub mod link;
pub mod manager;
pub mod options;
pub mod panel;
pub mod protocol;
pub mod record;
pub mod rules;

use zlog_core::{ContestFile, Extension, Link, LogFormat};

pub use options::ZyloOptions;
pub use panel::ZyloPanel;

zlog_core::texts!();

/// The command-line argument that makes the program a helper.
pub const SUBCOMMAND: &str = "zylo-host";

/// Run as a helper: call from `main` before anything else when the first argument
/// is [`SUBCOMMAND`]. Never returns.
pub fn helper_main() -> ! {
    helper::main()
}

/// The package: the link, the CFG files with a rule plugin and the plugins' formats.
/// Listed before the plain CFG files of `zlog-rule-ja`.
pub struct Zylo;

impl Extension for Zylo {
    fn name(&self) -> &str {
        "ZyLO"
    }

    fn formats(&self) -> Vec<Box<dyn LogFormat>> {
        format::formats(&manager::global())
    }

    fn contest_files(&self) -> Vec<Box<dyn ContestFile>> {
        vec![Box::new(rules::ZyloCfgFile {
            manager: manager::global(),
        })]
    }

    fn links(&mut self) -> Vec<Box<dyn Link>> {
        vec![Box::new(link::ZyloLink::new(manager::global()))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
