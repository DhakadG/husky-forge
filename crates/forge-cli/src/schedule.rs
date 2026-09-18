//! Register `forge rule run-due` with the OS scheduler: Task Scheduler, launchd, or a systemd user timer.
use std::process::Command;

use anyhow::{Context, Result, bail};

fn exe() -> Result<String> {
    Ok(std::env::current_exe()?.to_string_lossy().into_owned())
}

fn ok(mut c: Command, what: &str) -> Result<()> {
    let out = c.output().with_context(|| format!("run {what}"))?;
    if !out.status.success() {
        bail!("{what}: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(windows)]
const TASK: &str = "HuskyForgeRules";

/// Install a daily run at `hour`:00 local time.
#[cfg(windows)]
pub fn install(hour: u8) -> Result<String> {
    let mut c = Command::new("schtasks");
    c.args(["/Create", "/F", "/SC", "DAILY", "/TN", TASK, "/ST", &format!("{hour:02}:00"), "/TR", &format!("\"{}\" rule run-due", exe()?)]);
    ok(c, "schtasks")?;
    Ok(format!("Task Scheduler task {TASK} runs daily at {hour:02}:00"))
}

#[cfg(windows)]
pub fn remove() -> Result<String> {
    let mut c = Command::new("schtasks");
    c.args(["/Delete", "/F", "/TN", TASK]);
    ok(c, "schtasks")?;
    Ok("task removed".into())
}

#[cfg(target_os = "macos")]
fn plist() -> Result<std::path::PathBuf> {
    Ok(dirs::home_dir().context("home")?.join("Library/LaunchAgents/dev.huskyforge.rules.plist"))
}

#[cfg(target_os = "macos")]
pub fn install(hour: u8) -> Result<String> {
    let plist = plist()?;
    std::fs::create_dir_all(plist.parent().unwrap())?;
    std::fs::write(
        &plist,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.huskyforge.rules</string>
  <key>ProgramArguments</key><array><string>{}</string><string>rule</string><string>run-due</string></array>
  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>{hour}</integer><key>Minute</key><integer>0</integer></dict>
</dict></plist>
"#,
            exe()?
        ),
    )?;
    let mut c = Command::new("launchctl");
    c.args(["load", "-w"]).arg(&plist);
    ok(c, "launchctl")?;
    Ok(format!("launchd agent installed at {}", plist.display()))
}

#[cfg(target_os = "macos")]
pub fn remove() -> Result<String> {
    let plist = plist()?;
    let mut c = Command::new("launchctl");
    c.args(["unload", "-w"]).arg(&plist);
    let _ = ok(c, "launchctl");
    let _ = std::fs::remove_file(&plist);
    Ok("launchd agent removed".into())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unit_dir() -> Result<std::path::PathBuf> {
    Ok(dirs::config_dir().context("config dir")?.join("systemd/user"))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn install(hour: u8) -> Result<String> {
    let dir = unit_dir()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("husky-forge-rules.service"),
        format!("[Unit]\nDescription=Husky Forge recurring rules\n\n[Service]\nType=oneshot\nExecStart={} rule run-due\n", exe()?),
    )?;
    std::fs::write(
        dir.join("husky-forge-rules.timer"),
        format!("[Unit]\nDescription=Husky Forge recurring rules\n\n[Timer]\nOnCalendar=*-*-* {hour:02}:00:00\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n"),
    )?;
    let mut c = Command::new("systemctl");
    c.args(["--user", "enable", "--now", "husky-forge-rules.timer"]);
    ok(c, "systemctl")?;
    Ok(format!("systemd user timer installed in {}", dir.display()))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn remove() -> Result<String> {
    let mut c = Command::new("systemctl");
    c.args(["--user", "disable", "--now", "husky-forge-rules.timer"]);
    let _ = ok(c, "systemctl");
    let dir = unit_dir()?;
    let _ = std::fs::remove_file(dir.join("husky-forge-rules.timer"));
    let _ = std::fs::remove_file(dir.join("husky-forge-rules.service"));
    Ok("systemd user timer removed".into())
}
