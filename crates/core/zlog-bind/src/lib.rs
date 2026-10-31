pub mod keymap;
pub mod options;
pub mod toolbar;

pub use options::KeysPage;
pub use toolbar::ToolbarPage;

zlog_core::texts!();

pub fn register(b: &mut zlog_ui::Builder) {
    b.page(130, |ctx| {
        ToolbarPage::new(ctx.toolbar.clone().unwrap_or_default())
    });
    b.page(140, |_| KeysPage::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
