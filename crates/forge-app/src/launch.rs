//! Command line of the desktop app (also what Explorer / Finder / a second instance hand us),
//! and the single-instance funnel so multi-select in Explorer lands in one window.
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Stream, ToNsName, prelude::*};

#[derive(Parser, Debug, Default, Clone)]
#[command(name = "husky-forge", about = "Husky Forge desktop app")]
pub struct Launch {
    /// Output format.
    #[arg(long, value_enum)]
    pub to: Option<Fmt>,
    #[arg(long)]
    pub quality: Option<u8>,
    /// Megapixel cap (0 = none).
    #[arg(long)]
    pub max_mp: Option<f64>,
    /// Per-file size target in MB.
    #[arg(long)]
    pub target_mb: Option<f64>,
    #[arg(long, value_enum)]
    pub meta: Option<MetaArg>,
    #[arg(long, value_enum)]
    pub mode: Option<ModeArg>,
    /// Load a saved preset first; explicit flags override it.
    #[arg(long)]
    pub preset: Option<String>,
    /// Start processing as soon as the paths are planned.
    #[arg(long)]
    pub start: bool,
    /// Files or folders.
    pub paths: Vec<PathBuf>,
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum Fmt {
    Same,
    Jpeg,
    Png,
    Webp,
    Avif,
    Jxl,
}
#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum MetaArg {
    Keep,
    StripGps,
    Strip,
}
#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum ModeArg {
    Copy,
    Archive,
    Replace,
}

const SOCKET: &str = "husky-forge.instance";

/// Parse this process's argv (unknown flags are ignored rather than fatal — a shell launch must never fail to open a window).
pub fn from_env() -> Launch {
    Launch::try_parse().unwrap_or_else(|_| Launch { paths: std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect(), ..Default::default() })
}

fn parse_lines(s: &str) -> Launch {
    let args = std::iter::once("husky-forge".to_string()).chain(s.lines().filter(|l| !l.is_empty()).map(String::from));
    Launch::try_parse_from(args).unwrap_or_default()
}

/// If another instance is running, hand it our argv and return true (caller exits).
pub fn forward_to_running() -> bool {
    let Ok(name) = SOCKET.to_ns_name::<GenericNamespaced>() else { return false };
    let Ok(mut conn) = Stream::connect(name) else { return false };
    let payload: Vec<String> = std::env::args().skip(1).collect();
    conn.write_all(payload.join("\n").as_bytes()).is_ok()
}

/// Become the instance others forward to; `on` runs on a background thread per forwarded launch.
pub fn listen(on: impl Fn(Launch) + Send + 'static) {
    let Ok(name) = SOCKET.to_ns_name::<GenericNamespaced>() else { return };
    let Ok(listener) = ListenerOptions::new().name(name).create_sync() else { return };
    std::thread::spawn(move || {
        for mut conn in listener.incoming().filter_map(Result::ok) {
            let mut s = String::new();
            if conn.read_to_string(&mut s).is_ok() {
                on(parse_lines(&s));
            }
        }
    });
}
