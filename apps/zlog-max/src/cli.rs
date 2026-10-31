use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Subcommand;
use zlog_cli::MakeRegistry;

fn builder() -> zlog_ui::Builder {
    let mut b = zlog_ui::Builder::new();
    zlog_max::register(&mut b);
    b
}

#[derive(Subcommand)]
enum Tools {
    CheckCfg {
        file: PathBuf,
    },
    Summary {
        settings: PathBuf,
        log: PathBuf,
        out: PathBuf,
        #[arg(long)]
        contest: String,
        #[arg(long, default_value = "mix")]
        mode: String,
        #[arg(long)]
        format: Option<String>,
        #[arg(long)]
        date: Option<String>,
    },
}

impl zlog_cli::Tools for Tools {
    fn run(self, registry: MakeRegistry, out: &mut dyn Write) -> Result<(), String> {
        match self {
            Tools::CheckCfg { file } => check_cfg(&file, out),
            Tools::Summary {
                settings,
                log,
                out: path,
                contest,
                mode,
                format,
                date,
            } => {
                let kind = format
                    .as_deref()
                    .or(path.extension().and_then(|e| e.to_str()))
                    .and_then(zlog_summary::Kind::parse)
                    .ok_or("give --format elog or --format cabrillo")?;
                let text = std::fs::read_to_string(&settings)
                    .map_err(|e| format!("{}: {e}", settings.display()))?;
                let settings = zlog_summary::parse_settings(&text)?;
                let (mut log, _) = zlog_core::Log::load(&log).map_err(|e| e.to_string())?;
                let mode = zlog_cli::contest_mode(&mode)?;
                let mut rules = zlog_cli::contest(&registry(), &contest, mode)?;
                let sheet = zlog_core::rescore(&mut log, rules.as_mut())?;
                let bytes = zlog_summary::write(
                    &settings,
                    kind,
                    &log,
                    &sheet,
                    rules.as_ref(),
                    date.as_deref(),
                )?;
                std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))
            }
        }
    }
}

fn check_cfg(file: &std::path::Path, out: &mut dyn Write) -> Result<(), String> {
    let report = zlog_cfg::check::check(file);
    let lines = (report.notes.iter().map(|m| ("note", m)))
        .chain(report.warnings.iter().map(|m| ("warning", m)))
        .chain(report.errors.iter().map(|m| ("error", m)));
    for (level, message) in lines {
        writeln!(out, "{level}: {message}").map_err(|e| e.to_string())?;
    }
    match report.errors.len() {
        0 => Ok(()),
        n => Err(format!("{}: {n} errors", file.display())),
    }
}

fn main() -> ExitCode {
    if let Some(run) = std::env::args()
        .nth(1)
        .and_then(|a| builder().subcommand_of(&a))
    {
        run();
        return ExitCode::SUCCESS;
    }
    zlog_cli::run_with::<Tools>(|| builder().registry())
}
