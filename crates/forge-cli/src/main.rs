//! `forge` — headless front end for forge-core. Same engine the desktop app uses.
mod schedule;
mod shell;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use forge_core::{Event, Format, History, Impact, MetaMode, Mode, Options, Resize, impact::human, plan, run};

#[derive(Parser)]
#[command(name = "forge", version, about = "Husky Forge — image processing, perfected.")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    run: RunArgs,
    /// History database (default: per-user data dir).
    #[arg(long, global = true)]
    db: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Process files or folders (default command).
    Run(RunArgs),
    /// List past jobs.
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show the files of one job.
    Files { job: i64 },
    /// Undo a copy or archive job: delete outputs, restore originals.
    Undo { job: i64 },
    /// Recurring rules.
    Rule {
        #[command(subcommand)]
        cmd: RuleCmd,
    },
    /// File-manager integration (Windows: Explorer right-click menu; re-run after saving presets).
    Shell {
        #[command(subcommand)]
        cmd: ShellCmd,
    },
}

#[derive(Subcommand)]
enum ShellCmd {
    Install,
    Remove,
}

#[derive(Subcommand)]
enum RuleCmd {
    /// Add a rule: same options as `run`, plus a name and cadence.
    Add {
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = 7)]
        every_days: u32,
        #[command(flatten)]
        run: RunArgs,
    },
    List,
    Rm { id: i64 },
    Enable { id: i64 },
    Disable { id: i64 },
    /// Run every rule whose period has elapsed (what the OS scheduler calls).
    RunDue,
    /// Register a daily `rule run-due` with Task Scheduler / launchd / systemd.
    Schedule {
        #[arg(long, default_value_t = 3)]
        hour: u8,
        /// Remove the scheduled run instead.
        #[arg(long)]
        remove: bool,
    },
}

#[derive(Args, Clone)]
struct RunArgs {
    /// Files or folders to process.
    paths: Vec<PathBuf>,
    /// Output format (default: same family; RAW/TIFF → JPEG).
    #[arg(long, value_enum)]
    to: Option<Fmt>,
    #[arg(long, default_value_t = 82)]
    quality: u8,
    /// Cap output at this many megapixels (0 = no cap).
    #[arg(long, default_value_t = 8.0, conflicts_with_all = ["fit", "fill", "pad"])]
    max_mp: f64,
    /// Shrink to fit inside WxH.
    #[arg(long, value_parser = parse_wh)]
    fit: Option<(u32, u32)>,
    /// Scale and centre-crop to exactly WxH.
    #[arg(long, value_parser = parse_wh)]
    fill: Option<(u32, u32)>,
    /// Fit inside WxH and pad to exactly WxH.
    #[arg(long, value_parser = parse_wh)]
    pad: Option<(u32, u32)>,
    /// Pad colour as hex, e.g. ffffff.
    #[arg(long, default_value = "000000")]
    pad_color: String,
    /// Apply a .cube LUT after resizing.
    #[arg(long)]
    lut: Option<PathBuf>,
    /// Tune quality per file toward this size in MB.
    #[arg(long, default_value_t = 0.0)]
    target_mb: f64,
    #[arg(long, value_enum, default_value_t = Meta::Keep)]
    meta: Meta,
    #[arg(long, value_enum, default_value_t = ModeArg::Copy)]
    mode: ModeArg,
    #[arg(long, default_value_t = 0)]
    workers: usize,
    /// Process files smaller than this too (default skips < 300 KB).
    #[arg(long)]
    include_small: bool,
    /// Keep the result even when it is not smaller than the source.
    #[arg(long)]
    keep_larger: bool,
    /// Plan and show the Impact estimate without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Do not record this run in the history database.
    #[arg(long)]
    no_history: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum Fmt {
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
}
#[derive(Clone, Copy, ValueEnum)]
enum Meta {
    Keep,
    StripGps,
    Strip,
}
#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    Copy,
    Archive,
    Replace,
}

fn parse_wh(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once(['x', 'X', '×']).ok_or("expected WxH")?;
    Ok((w.parse().map_err(|_| "bad width")?, h.parse().map_err(|_| "bad height")?))
}

impl RunArgs {
    fn options(&self) -> Result<Options> {
        let rgb = u32::from_str_radix(self.pad_color.trim_start_matches('#'), 16).context("pad colour must be hex")?;
        let rgb = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
        let resize = match (self.fit, self.fill, self.pad) {
            (Some((w, h)), _, _) => Resize::Fit { w, h },
            (_, Some((w, h)), _) => Resize::Fill { w, h },
            (_, _, Some((w, h))) => Resize::Pad { w, h, rgb },
            _ if self.max_mp > 0.0 => Resize::Cap { mp: self.max_mp },
            _ => Resize::None,
        };
        Ok(Options {
            format: self.to.map(|f| match f {
                Fmt::Jpeg => Format::Jpeg,
                Fmt::Png => Format::Png,
                Fmt::Webp => Format::Webp,
                Fmt::Avif => Format::Avif,
                Fmt::Jxl => Format::Jxl,
            }),
            quality: self.quality,
            resize,
            lut: self.lut.clone(),
            target_bytes: (self.target_mb * 1024.0 * 1024.0) as u64,
            meta: match self.meta {
                Meta::Keep => MetaMode::Keep,
                Meta::StripGps => MetaMode::StripGps,
                Meta::Strip => MetaMode::Strip,
            },
            mode: match self.mode {
                ModeArg::Copy => Mode::Copy,
                ModeArg::Archive => Mode::Archive,
                ModeArg::Replace => Mode::Replace,
            },
            workers: self.workers,
            min_bytes: if self.include_small { 0 } else { Options::default().min_bytes },
            only_if_smaller: !self.keep_larger,
            ..Options::default()
        })
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db = || History::open(&cli.db.clone().unwrap_or_else(History::default_path));
    match cli.cmd {
        None => do_run(&cli.run, db),
        Some(Cmd::Run(r)) => do_run(&r, db),
        Some(Cmd::History { limit }) => {
            for j in db()?.jobs(limit)? {
                let flag = if j.undone { " (undone)" } else { "" };
                println!("#{}  {}  {:?}  {} files  {} → {}{flag}", j.id, j.started, j.mode, j.files, human(j.before), human(j.after));
            }
            Ok(())
        }
        Some(Cmd::Files { job }) => {
            for f in db()?.files(job)? {
                match f.error {
                    Some(e) => println!("FAIL  {}  {e}", f.src.display()),
                    None => println!("{}  {} → {}  q{}  → {}", f.src.display(), human(f.before), human(f.after), f.quality, f.out.map(|p| p.display().to_string()).unwrap_or_default()),
                }
            }
            Ok(())
        }
        Some(Cmd::Undo { job }) => {
            let n = db()?.undo(job)?;
            println!("restored {n} files");
            Ok(())
        }
        Some(Cmd::Shell { cmd }) => {
            #[cfg(windows)]
            {
                println!("{}", match cmd {
                    ShellCmd::Install => shell::install()?,
                    ShellCmd::Remove => shell::remove()?,
                });
                Ok(())
            }
            #[cfg(not(windows))]
            {
                let _ = cmd;
                bail!("shell integration is Windows-only for now; macOS/Linux use the .app / .desktop file")
            }
        }
        Some(Cmd::Rule { cmd }) => {
            let mut h = db()?;
            match cmd {
                RuleCmd::Add { name, every_days, run } => {
                    if run.paths.is_empty() {
                        bail!("a rule needs at least one path");
                    }
                    let id = h.add_rule(&name, &run.paths, &run.options()?, every_days)?;
                    println!("rule #{id} added");
                }
                RuleCmd::List => {
                    for r in h.rules()? {
                        let state = if r.enabled { "on " } else { "off" };
                        println!("#{} {state} every {}d  last {}  {}  {:?}", r.id, r.every_days, r.last_run.unwrap_or_else(|| "never".into()), r.name, r.paths);
                    }
                }
                RuleCmd::Rm { id } => println!("{}", if h.remove_rule(id)? { "removed" } else { "no such rule" }),
                RuleCmd::Enable { id } => println!("{}", if h.set_rule_enabled(id, true)? { "enabled" } else { "no such rule" }),
                RuleCmd::Disable { id } => println!("{}", if h.set_rule_enabled(id, false)? { "disabled" } else { "no such rule" }),
                RuleCmd::RunDue => {
                    for (rule, job, impact) in h.run_due(&|_| {})? {
                        println!("rule #{rule} → job #{job}");
                        print_card(&impact);
                    }
                }
                RuleCmd::Schedule { hour, remove } => println!("{}", if remove { schedule::remove()? } else { schedule::install(hour.min(23))? }),
            }
            Ok(())
        }
    }
}

fn do_run(a: &RunArgs, db: impl Fn() -> Result<History>) -> Result<()> {
    if a.paths.is_empty() {
        bail!("give at least one file or folder (see --help)");
    }
    let o = a.options()?;
    let p = plan(&a.paths, &o);
    for (path, why) in &p.skipped {
        eprintln!("skip  {}  ({why})", path.display());
    }
    eprintln!("{} files, {}", p.items.len(), human(p.total_bytes()));
    if a.dry_run || p.items.is_empty() {
        print_card(&Impact::estimate(&p, &o));
        return Ok(());
    }
    let total = p.items.len();
    let done = AtomicUsize::new(0);
    let outs = run(&p, &o, &|ev| {
        if let Event::Finished(r) = ev {
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            let name = r.path.file_name().unwrap_or_default().to_string_lossy();
            match &r.error {
                Some(e) => eprintln!("[{n}/{total}] FAIL {name}  {e}"),
                None => eprintln!("[{n}/{total}] {name}  {} → {}  q{} {}x{} {}-bit via {}", human(r.before), human(r.after), r.quality, r.width, r.height, r.bits, r.via),
            }
        }
    });
    let impact = Impact::from_outcomes(&outs, &o);
    if !a.no_history {
        let id = db()?.record(&o, &outs, &impact)?;
        eprintln!("recorded as job #{id}  (forge undo {id} reverts a copy/archive job)");
    }
    print_card(&impact);
    Ok(())
}

fn print_card(i: &Impact) {
    println!();
    for l in i.lines() {
        println!("  {l}");
    }
}
