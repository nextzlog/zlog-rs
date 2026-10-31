pub mod change;
pub mod editor;
pub mod start;

pub use editor::Editor;

zlog_core::texts!();

pub fn text(language: &str, key: &str) -> String {
    texts().get(language, key).to_string()
}

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(110, |_| Editor::default());
    b.start_time_dialog(start::dialog);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
