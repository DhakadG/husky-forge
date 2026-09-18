//! Named option sets as TOML files in the per-user config dir.
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use forge_core::Options;

fn dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("husky-forge").join("presets")
}

pub fn list() -> Vec<slint::SharedString> {
    let mut names: Vec<_> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect();
    names.sort();
    names.into_iter().map(Into::into).collect()
}

pub fn save(name: &str, o: &Options) -> Result<()> {
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\', '.']) {
        bail!("preset name must be a plain word");
    }
    std::fs::create_dir_all(dir())?;
    std::fs::write(dir().join(format!("{name}.toml")), toml::to_string_pretty(o)?).context("write preset")
}

pub fn load(name: &str) -> Result<Options> {
    let text = std::fs::read_to_string(dir().join(format!("{name}.toml"))).with_context(|| format!("preset {name}"))?;
    Ok(toml::from_str(&text)?)
}
