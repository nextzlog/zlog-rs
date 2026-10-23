//! The entry row of zLog: one line under the log grid, its fields lined up with the
//! grid's columns, typed into with zLog's keys ([`row`]).

pub mod row;

pub use row::ZlogRow;

zlog_core::texts!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_complete() {
        assert!(texts().missing().is_empty());
    }
}
