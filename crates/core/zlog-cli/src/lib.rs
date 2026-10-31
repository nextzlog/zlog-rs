mod regress;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, FromArgMatches, Parser, Subcommand};
use zlog_core::{Contest, ContestMode, Format, Log, LogFormat, Registry};

pub type MakeRegistry = fn() -> Registry;

pub trait Tools: Subcommand {
    fn run(self, registry: MakeRegistry, out: &mut dyn Write) -> Result<(), String>;
}

#[derive(Subcommand)]
pub enum NoTools {}

impl Tools for NoTools {
    fn run(self, _: MakeRegistry, _: &mut dyn Write) -> Result<(), String> {
        match self {}
    }
}

#[derive(Parser)]
#[command(name = "zlog", about = "zLog developer tool")]
struct Cli<T: Tools> {
    #[command(subcommand)]
    command: Command<T>,
}

#[derive(Subcommand)]
enum Command<T: Tools> {
    #[command(flatten)]
    Core(Core),
    #[command(flatten)]
    Tool(T),
}

#[derive(Subcommand)]
enum Core {
    Dump {
        file: PathBuf,
    },
    Score {
        file: PathBuf,
        contest: String,
        #[arg(default_value = "mix")]
        mode: String,
        coeff: Option<f64>,
    },
    Convert(Convert),
    Regress {
        dir: PathBuf,
        #[arg(long)]
        update: bool,
    },
}

#[derive(Args)]
struct Convert {
    input: PathBuf,
    output: PathBuf,
    #[arg(long)]
    from: Option<String>,
    #[arg(long)]
    to: Option<String>,
    #[arg(long)]
    contest: Option<String>,
}

pub fn run(registry: MakeRegistry) -> ExitCode {
    run_with::<NoTools>(registry)
}

pub fn run_with<T: Tools>(registry: MakeRegistry) -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut out = std::io::stdout();
    match execute::<T>(registry, &args, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(e)) => {
            let _ = e.print();
            ExitCode::from(e.exit_code().clamp(0, 255) as u8)
        }
        Err(Failure::Failed(e)) => {
            eprintln!("zlog: {e}");
            ExitCode::FAILURE
        }
    }
}

pub enum Failure {
    Usage(clap::Error),
    Failed(String),
}

pub fn execute<T: Tools>(
    registry: MakeRegistry,
    args: &[String],
    out: &mut dyn Write,
) -> Result<(), Failure> {
    let matches = <Cli<T> as clap::CommandFactory>::command()
        .try_get_matches_from(args)
        .map_err(Failure::Usage)?;
    let cli = Cli::<T>::from_arg_matches(&matches).map_err(Failure::Usage)?;
    match cli.command {
        Command::Core(c) => core::<T>(c, registry, out),
        Command::Tool(t) => t.run(registry, out),
    }
    .map_err(Failure::Failed)
}

fn core<T: Tools>(c: Core, registry: MakeRegistry, out: &mut dyn Write) -> Result<(), String> {
    match c {
        Core::Dump { file } => dump(&file, out),
        Core::Score {
            file,
            contest,
            mode,
            coeff,
        } => score(registry, &file, &contest, &mode, coeff, out),
        Core::Convert(c) => convert(registry, &c),
        Core::Regress { dir, update } => regress::run::<T>(registry, &dir, update, out),
    }
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

fn dump(file: &Path, out: &mut dyn Write) -> Result<(), String> {
    let (log, format) = Log::load(file).map_err(|e| e.to_string())?;
    writeln!(
        out,
        "# {format:?}, {} QSOs, start {}",
        log.qsos.len(),
        log.start_time()
    )
    .map_err(io)?;
    for q in &log.qsos {
        writeln!(
            out,
            "{}  {:<12} {:>3} {:<8} {:>3} {:<8} {:>5} {:<4} {:<8} {}",
            q.time,
            q.callsign,
            q.rst_sent,
            q.nr_sent,
            q.rst_rcvd,
            q.nr_rcvd,
            q.band,
            q.mode,
            q.operator,
            q.memo
        )
        .map_err(io)?;
    }
    Ok(())
}

pub fn contest_mode(mode: &str) -> Result<ContestMode, String> {
    match mode {
        "mix" => Ok(ContestMode::Mix),
        "cw" => Ok(ContestMode::Cw),
        "ph" => Ok(ContestMode::Phone),
        _ => Err(format!("unknown mode {mode}")),
    }
}

pub fn contest(
    registry: &Registry,
    contest: &str,
    mode: ContestMode,
) -> Result<Box<dyn Contest>, String> {
    let path = Path::new(contest);
    if registry.contest_files().iter().any(|f| f.accepts(path)) {
        let opened = registry.open_contest_file(path, "en")?;
        return Ok(opened.def.build(mode));
    }
    let defs = registry.contests();
    let def = defs
        .iter()
        .find(|d| d.id() == contest)
        .ok_or_else(|| format!("unknown contest {contest}"))?;
    Ok(def.build(mode))
}

fn score(
    registry: MakeRegistry,
    file: &Path,
    contest_id: &str,
    mode: &str,
    coeff: Option<f64>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (mut log, _) = Log::load(file).map_err(|e| e.to_string())?;
    if let Some(coeff) = coeff {
        log.set_score_coeff(coeff);
    }
    let mut rules = contest(&registry(), contest_id, contest_mode(mode)?)?;
    let sheet = zlog_core::rescore(&mut log, rules.as_mut())?;
    let mut text = format!(
        "{:>6} {:>6} {:>6} {:>6}\n",
        "MHz", "QSOs", "Points", "Multi"
    );
    for (band, row) in &sheet.bands {
        text += &format!(
            "{:>6} {:>6} {:>6} {:>6}\n",
            band.label(),
            row.qsos,
            row.points,
            row.multi
        );
    }
    let t = sheet.total;
    text += &format!(
        "{:>6} {:>6} {:>6} {:>6}\n",
        "Total", t.qsos, t.points, t.multi
    );
    text += &format!("Score  {}\n", sheet.score);
    if let Some((coeff, s)) = sheet.with_coeff {
        text += &format!("Score with coefficient {coeff:.2}: {s:.0}\n");
    }
    out.write_all(text.as_bytes()).map_err(io)
}

enum Kind<'a> {
    Native(Format),
    Other(&'a dyn LogFormat),
}

fn kind<'a>(registry: &'a Registry, path: &Path, named: Option<&str>) -> Result<Kind<'a>, String> {
    let key = match named {
        Some(n) => n.to_string(),
        None => path
            .extension()
            .and_then(|e| e.to_str())
            .ok_or_else(|| format!("{}: no extension; give the format", path.display()))?
            .to_string(),
    };
    match key.to_ascii_lowercase().as_str() {
        "zlo" => return Ok(Kind::Native(Format::Zlo)),
        "zlox" => return Ok(Kind::Native(Format::Zlox)),
        _ => {}
    }
    let formats = registry.formats();
    formats
        .iter()
        .find(|f| f.name().eq_ignore_ascii_case(&key))
        .or_else(|| {
            formats.iter().find(|f| {
                f.extensions()
                    .iter()
                    .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&key))
            })
        })
        .map(|f| Kind::Other(f.as_ref()))
        .ok_or_else(|| {
            let mut names = vec!["zlo".to_string(), "zlox".to_string()];
            names.extend(formats.iter().map(|f| f.name().to_string()));
            format!("unknown format {key} (known: {})", names.join(", "))
        })
}

fn convert(registry: MakeRegistry, c: &Convert) -> Result<(), String> {
    let registry = registry();
    let from = kind(&registry, &c.input, c.from.as_deref())?;
    let to = kind(&registry, &c.output, c.to.as_deref())?;
    let bytes = std::fs::read(&c.input).map_err(|e| format!("{}: {e}", c.input.display()))?;
    let mut log = match from {
        Kind::Native(_) => Log::from_bytes(&bytes).map_err(|e| e.to_string())?.0,
        Kind::Other(f) if f.can_import() => Log {
            qsos: f.import(&bytes)?,
            ..Log::default()
        },
        Kind::Other(f) => return Err(format!("{} cannot import", f.name())),
    };
    let bytes = match to {
        Kind::Native(format) => log.to_bytes(format),
        Kind::Other(f) if f.can_export() => {
            let id = match &c.contest {
                Some(id) => id.clone(),
                None => registry
                    .contests()
                    .first()
                    .map(|d| d.id().to_string())
                    .ok_or("no contest to export with")?,
            };
            let mut rules = contest(&registry, &id, ContestMode::Mix)?;
            zlog_core::rescore(&mut log, rules.as_mut())?;
            f.export(&log, rules.as_ref())?
        }
        Kind::Other(f) => return Err(format!("{} cannot export", f.name())),
    };
    std::fs::write(&c.output, bytes).map_err(|e| format!("{}: {e}", c.output.display()))
}
