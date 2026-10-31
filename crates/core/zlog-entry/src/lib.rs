pub mod options;
pub mod row;

pub use options::EnterOptions;
pub use row::ZlogRow;

zlog_core::texts!();

pub fn register(b: &mut zlog_ui::Builder) {
    b.entry_style(10, ZlogRow::default);
    b.page(110, |_| EnterOptions);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
