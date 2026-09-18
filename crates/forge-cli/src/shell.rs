//! Explorer context menu (Windows): a "Husky Forge" cascade on folders, folder backgrounds and
//! image files, in HKCU so no admin is needed. The installer calls `forge shell install`;
//! portable users run it themselves. Re-run after saving presets to list them in the menu.
#![cfg(windows)]
use std::path::PathBuf;

use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};

const VERB: &str = "HuskyForge";
const RAW_EXT: &[&str] = &["arw", "srf", "sr2", "cr2", "cr3", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "3fr", "iiq", "jxl", "avif", "heic", "heif"];

/// (menu label, extra flags for husky-forge.exe)
fn entries() -> Vec<(String, String)> {
    let mut v = vec![
        ("Open in Husky Forge".into(), "".into()),
        ("Compress here → JPEG, keep originals".into(), "--to jpeg --mode copy --start".into()),
        ("Convert → JPEG XL, keep originals".into(), "--to jxl --mode copy --start".into()),
        ("Convert → AVIF, keep originals".into(), "--to avif --mode copy --start".into()),
        ("Archive originals → JPEG tuned to 2.5 MB".into(), "--to jpeg --target-mb 2.5 --mode archive --start".into()),
        ("Copy without GPS".into(), "--meta strip-gps --mode copy --start".into()),
    ];
    for name in presets() {
        v.push((format!("Preset: {name}"), format!("--preset \"{name}\" --start")));
    }
    v
}

fn presets() -> Vec<String> {
    let dir = dirs::config_dir().unwrap_or_default().join("husky-forge").join("presets");
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
        .filter_map(|e| e.path().file_stem().map(|s| s.to_string_lossy().to_string()))
        .collect();
    names.sort();
    names
}

/// Where the desktop app lives: next to this `forge.exe`.
fn app_exe() -> Result<PathBuf> {
    let p = std::env::current_exe()?.with_file_name("husky-forge.exe");
    anyhow::ensure!(p.exists(), "husky-forge.exe not found next to forge.exe ({})", p.display());
    Ok(p)
}

/// Registry roots and the placeholder Explorer substitutes for the clicked item.
fn roots() -> Vec<(String, &'static str)> {
    let mut v = vec![
        ("Software\\Classes\\Directory\\shell".to_string(), "%1"),
        ("Software\\Classes\\Directory\\Background\\shell".to_string(), "%V"),
        ("Software\\Classes\\SystemFileAssociations\\image\\shell".to_string(), "%1"),
    ];
    v.extend(RAW_EXT.iter().map(|e| (format!("Software\\Classes\\SystemFileAssociations\\.{e}\\shell"), "%1")));
    v
}

pub fn install() -> Result<String> {
    let app = app_exe()?;
    let app = app.to_string_lossy();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let entries = entries();
    for (root, arg) in roots() {
        let (top, _) = hkcu.create_subkey(format!("{root}\\{VERB}")).with_context(|| root.clone())?;
        top.set_value("MUIVerb", &"Husky Forge")?;
        top.set_value("Icon", &format!("\"{app}\",0"))?;
        top.set_value("SubCommands", &"")?;
        // Each selected file launches its own process; the app funnels them into one window.
        top.set_value("MultiSelectModel", &"Document")?;
        let (shell, _) = top.create_subkey("shell")?;
        for (i, (label, flags)) in entries.iter().enumerate() {
            let (item, _) = shell.create_subkey(format!("{i:02}"))?;
            item.set_value("MUIVerb", label)?;
            let (cmd, _) = item.create_subkey("command")?;
            let sep = if flags.is_empty() { "" } else { " " };
            cmd.set_value("", &format!("\"{app}\"{sep}{flags} \"{arg}\""))?;
        }
    }
    Ok(format!("Explorer menu installed with {} entries", entries.len()))
}

pub fn remove() -> Result<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    for (root, _) in roots() {
        if let Ok(k) = hkcu.open_subkey_with_flags(&root, KEY_ALL_ACCESS) {
            let _ = k.delete_subkey_all(VERB);
        }
    }
    Ok("Explorer menu removed".into())
}
