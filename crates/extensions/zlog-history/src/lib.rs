pub mod config;
pub mod index;
pub mod loader;
pub mod options;
pub mod panel;
pub mod table;

pub use options::HistOptions;
pub use panel::History;

zlog_core::texts!();

pub fn register(b: &mut zlog_ui::Builder) {
    b.panel(530, |ctx| History::new(ctx.registry));
    b.page(420, |_| HistOptions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
