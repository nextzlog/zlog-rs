#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    RenamedPanel(&'static str, &'static str),
    MergedPanel(&'static str, &'static str),
    MovedCommand(&'static str, &'static str),
    CopiedKey {
        from: (&'static str, &'static str),
        to: (&'static str, &'static str),
    },
}
