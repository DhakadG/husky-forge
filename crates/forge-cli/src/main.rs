//! `forge` — headless front end for forge-core. Same engine the desktop app uses.
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::{Parser, ValueEnum};
use forge_core::{Event, Format, Impact, MetaMode, Mode, Options, impact::human, plan, run};

#[derive(Parser)]
#[command(name = "forge", version, about = "Husky Forge — image processing, perfected.")]
struct Cli {
    /// Files or folders to process.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Output format (default: same family; RAW/TIFF → JPEG).
    #[arg(long, value_enum)]
    to: Option<Fmt>,
    #[arg(long, default_value_t = 82)]
    quality: u8,
    /// Cap output at this many megapixels (0 = no cap).
    #[arg(long, default_value_t = 8.0)]
    max_mp: f64,
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

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let o = Options {
        format: cli.to.map(|f| match f {
            Fmt::Jpeg => Format::Jpeg,
            Fmt::Png => Format::Png,
            Fmt::Webp => Format::Webp,
            Fmt::Avif => Format::Avif,
            Fmt::Jxl => Format::Jxl,
        }),
        quality: cli.quality,
        max_mp: cli.max_mp,
        target_bytes: (cli.target_mb * 1024.0 * 1024.0) as u64,
        meta: match cli.meta {
            Meta::Keep => MetaMode::Keep,
            Meta::StripGps => MetaMode::StripGps,
            Meta::Strip => MetaMode::Strip,
        },
        mode: match cli.mode {
            ModeArg::Copy => Mode::Copy,
            ModeArg::Archive => Mode::Archive,
            ModeArg::Replace => Mode::Replace,
        },
        workers: cli.workers,
        min_bytes: if cli.include_small { 0 } else { Options::default().min_bytes },
        only_if_smaller: !cli.keep_larger,
        ..Options::default()
    };
    let p = plan(&cli.paths, &o);
    for (path, why) in &p.skipped {
        eprintln!("skip  {}  ({why})", path.display());
    }
    eprintln!("{} files, {}", p.items.len(), human(p.total_bytes()));
    if cli.dry_run || p.items.is_empty() {
        print_card(&Impact::estimate(&p, &o));
        return Ok(());
    }
    let total = p.items.len();
    let done = AtomicUsize::new(0);
    let outs = run(&p, &o, &|ev| {
        if let Event::Finished(r) = ev {
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            match &r.error {
                Some(e) => eprintln!("[{n}/{total}] FAIL {}  {e}", r.path.display()),
                None => eprintln!(
                    "[{n}/{total}] {}  {} → {}  q{} {}x{} via {}",
                    r.path.file_name().unwrap_or_default().to_string_lossy(),
                    human(r.before),
                    human(r.after),
                    r.quality,
                    r.width,
                    r.height,
                    r.via
                ),
            }
        }
    });
    print_card(&Impact::from_outcomes(&outs, &o));
    Ok(())
}

fn print_card(i: &Impact) {
    println!();
    for l in i.lines() {
        println!("  {l}");
    }
}
