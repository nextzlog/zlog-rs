use std::process::ExitCode;

fn main() -> ExitCode {
    zlog_cli::run(|| zlog_core::Registry::new().with(zlog_rule::Base))
}
