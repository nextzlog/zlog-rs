use egui::{Key, KeyboardShortcut};
use keyboard_types::{Code, Modifiers};

#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
pub use native::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    Native(Modifiers, Code),
    Hidden,
}

pub fn shown(k: &KeyboardShortcut) -> Shown {
    match accelerator(k) {
        Some((mods, code)) if safe(k) => Shown::Native(mods, code),
        _ => Shown::Hidden,
    }
}

pub fn parse(text: &str) -> Option<(Modifiers, Code)> {
    accelerator(&zlog_ui::keys::parse(text)?)
}

pub fn accelerator(k: &KeyboardShortcut) -> Option<(Modifiers, Code)> {
    let m = k.modifiers;
    let mut mods = Modifiers::empty();
    if m.command || m.mac_cmd {
        mods |= Modifiers::META;
    }
    if m.ctrl && !m.command {
        mods |= Modifiers::CONTROL;
    }
    if m.alt {
        mods |= Modifiers::ALT;
    }
    if m.shift {
        mods |= Modifiers::SHIFT;
    }
    Some((mods, code(k.logical_key)?))
}

pub fn safe(k: &KeyboardShortcut) -> bool {
    let m = k.modifiers;
    m.command || m.mac_cmd || m.ctrl || m.alt || function(k.logical_key)
}

fn function(key: Key) -> bool {
    key.name()
        .strip_prefix('F')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn code(key: Key) -> Option<Code> {
    let name = key.name();
    let text = match key {
        Key::A | Key::B | Key::C | Key::D | Key::E | Key::F | Key::G | Key::H | Key::I => {
            format!("Key{name}")
        }
        Key::J | Key::K | Key::L | Key::M | Key::N | Key::O | Key::P | Key::Q | Key::R => {
            format!("Key{name}")
        }
        Key::S | Key::T | Key::U | Key::V | Key::W | Key::X | Key::Y | Key::Z => {
            format!("Key{name}")
        }
        Key::Num0 | Key::Num1 | Key::Num2 | Key::Num3 | Key::Num4 => format!("Digit{name}"),
        Key::Num5 | Key::Num6 | Key::Num7 | Key::Num8 | Key::Num9 => format!("Digit{name}"),
        Key::OpenBracket => "BracketLeft".into(),
        Key::CloseBracket => "BracketRight".into(),
        Key::Backtick => "Backquote".into(),
        Key::Equals => "Equal".into(),
        Key::Backslash
        | Key::Comma
        | Key::Minus
        | Key::Period
        | Key::Quote
        | Key::Semicolon
        | Key::Slash
        | Key::Space
        | Key::Tab
        | Key::Backspace
        | Key::Enter
        | Key::Delete
        | Key::Insert
        | Key::Home
        | Key::End
        | Key::PageUp
        | Key::PageDown
        | Key::ArrowDown
        | Key::ArrowLeft
        | Key::ArrowRight
        | Key::ArrowUp
        | Key::Escape
        | Key::Pause
        | Key::Help => format!("{key:?}"),
        k if function(k) => name.to_string(),
        _ => return None,
    };
    text.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers as M;

    #[test]
    fn zlog_text_becomes_an_accelerator() {
        let cmd = Modifiers::META;
        assert_eq!(parse("Ctrl+S"), Some((cmd, Code::KeyS)));
        assert_eq!(
            parse("Ctrl+Shift+P"),
            Some((cmd | Modifiers::SHIFT, Code::KeyP))
        );
        assert_eq!(
            parse("Alt+Shift+B"),
            Some((Modifiers::ALT | Modifiers::SHIFT, Code::KeyB))
        );
        assert_eq!(parse("MacCtrl+S"), Some((Modifiers::CONTROL, Code::KeyS)));
        assert_eq!(parse("F9"), Some((Modifiers::empty(), Code::F9)));
        assert_eq!(parse("Ctrl+1"), Some((cmd, Code::Digit1)));
        assert_eq!(parse("Ctrl+Enter"), Some((cmd, Code::Enter)));
        assert_eq!(parse("Alt+ArrowUp"), Some((Modifiers::ALT, Code::ArrowUp)));
        assert_eq!(parse("Ctrl+F35"), Some((cmd, Code::F35)));
        assert_eq!(parse("Ctrl+Nothing"), None);
    }

    #[test]
    fn only_modified_or_function_keys_bind_natively() {
        let k = |m, key| KeyboardShortcut::new(m, key);
        for safe in [
            k(M::COMMAND, Key::S),
            k(M::CTRL, Key::S),
            k(M::ALT, Key::B),
            k(M::ALT | M::SHIFT, Key::B),
            k(M::NONE, Key::F9),
            k(M::SHIFT, Key::F12),
        ] {
            assert!(matches!(shown(&safe), Shown::Native(..)), "{safe:?}");
        }
        for hidden in [
            k(M::SHIFT, Key::B),
            k(M::SHIFT, Key::V),
            k(M::NONE, Key::Enter),
            k(M::NONE, Key::Escape),
            k(M::NONE, Key::Delete),
            k(M::NONE, Key::A),
            k(M::COMMAND, Key::Plus),
        ] {
            assert_eq!(shown(&hidden), Shown::Hidden, "{hidden:?}");
        }
    }
}
