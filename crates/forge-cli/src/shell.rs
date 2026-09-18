//! Explorer context menu (Windows): a "Husky Forge" cascade on folders, folder backgrounds and
//! image files. The installer calls `forge shell install --machine` (HKLM, elevated); portable
//! users run `forge shell install` (HKCU). Re-run after saving presets to list them in the menu.
#![cfg(windows)]
use std::path::PathBuf;

use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_ALL_ACCESS};

const VERB: &str = "HuskyForge";
const RAW_EXT: &[&str] = &["arw", "srf", "sr2", "cr2", "cr3", "nef", "nrw", "dng", "raf", "orf", "rw2", "pef", "3fr", "iiq", "jxl", "avif", "heic", "heif"];
const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "jpe", "jfif", "png", "webp", "tif", "tiff", "gif", "bmp"];

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
/// Per-machine (HKLM) uses SystemFileAssociations like every other image tool. Explorer ignores
/// per-user SystemFileAssociations verbs, so HKCU uses `*\shell` limited to image extensions
/// with an AQS `AppliesTo` filter.
fn roots(machine: bool) -> Vec<(String, &'static str)> {
    let mut v = vec![(r"Software\Classes\Directory\shell".to_string(), "%1"), (r"Software\Classes\Directory\Background\shell".to_string(), "%V")];
    if machine {
        v.push((r"Software\Classes\SystemFileAssociations\image\shell".to_string(), "%1"));
        v.extend(RAW_EXT.iter().map(|e| (format!(r"Software\Classes\SystemFileAssociations\.{e}\shell"), "%1")));
    } else {
        v.push((r"Software\Classes\*\shell".to_string(), "%1"));
    }
    v
}

fn applies_to() -> String {
    IMAGE_EXT.iter().chain(RAW_EXT.iter()).map(|e| format!("System.FileExtension:=.{e}")).collect::<Vec<_>>().join(" OR ")
}

fn hive(machine: bool) -> RegKey {
    RegKey::predef(if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER })
}

/// `machine`: HKLM for all users (needs elevation; the installer does this). Otherwise HKCU.
pub fn install(machine: bool) -> Result<String> {
    let app = app_exe()?;
    let app = app.to_string_lossy();
    let hive = hive(machine);
    let entries = entries();
    for (root, arg) in roots(machine) {
        let (top, _) = hive.create_subkey(format!(r"{root}\{VERB}")).with_context(|| format!("{root} (run elevated for --machine)"))?;
        top.set_value("MUIVerb", &"Husky Forge")?;
        top.set_value("Icon", &format!("\"{app}\",0"))?;
        top.set_value("SubCommands", &"")?;
        // Each selected file launches its own process; the app funnels them into one window.
        top.set_value("MultiSelectModel", &"Document")?;
        if root.contains(r"\*\") {
            top.set_value("AppliesTo", &applies_to())?;
        }
        let (shell, _) = top.create_subkey("shell")?;
        for (i, (label, flags)) in entries.iter().enumerate() {
            let (item, _) = shell.create_subkey(format!("{i:02}"))?;
            item.set_value("MUIVerb", label)?;
            let (cmd, _) = item.create_subkey("command")?;
            let sep = if flags.is_empty() { "" } else { " " };
            cmd.set_value("", &format!("\"{app}\"{sep}{flags} \"{arg}\""))?;
        }
    }
    Ok(format!("Explorer menu installed for {} with {} entries", if machine { "all users" } else { "this user" }, entries.len()))
}

pub fn remove(machine: bool) -> Result<String> {
    let hive = hive(machine);
    // Also clear roots from the other registration shape so an upgrade never leaves a stale cascade.
    for (root, _) in roots(machine).into_iter().chain(roots(!machine)) {
        if let Ok(k) = hive.open_subkey_with_flags(&root, KEY_ALL_ACCESS) {
            let _ = k.delete_subkey_all(VERB);
        }
    }
    Ok("Explorer menu removed".into())
}
