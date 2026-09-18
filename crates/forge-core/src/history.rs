//! SQLite record of every job and file, plus undo for copy/archive jobs.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

use crate::impact::Impact;
use crate::job::{Mode, Options, Outcome};

pub struct History {
    pub(crate) conn: Connection,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct JobRow {
    pub id: i64,
    pub started: String,
    pub mode: Mode,
    pub files: usize,
    pub before: u64,
    pub after: u64,
    pub undone: bool,
    pub card: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileRow {
    pub src: PathBuf,
    pub out: Option<PathBuf>,
    pub before: u64,
    pub after: u64,
    pub quality: u8,
    pub error: Option<String>,
}

impl History {
    /// `<local data dir>/husky-forge/history.db` (AppData, ~/Library/Application Support, XDG).
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("husky-forge").join("history.db")
    }

    pub fn open(path: &Path) -> Result<History> {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE IF NOT EXISTS jobs(
               id INTEGER PRIMARY KEY, started TEXT NOT NULL DEFAULT (datetime('now')),
               options TEXT NOT NULL, card TEXT NOT NULL, undone INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS files(
               job INTEGER NOT NULL REFERENCES jobs(id), src TEXT NOT NULL, out TEXT,
               before INTEGER NOT NULL, after INTEGER NOT NULL, quality INTEGER NOT NULL, error TEXT);
             CREATE TABLE IF NOT EXISTS rules(
               id INTEGER PRIMARY KEY, name TEXT NOT NULL, paths TEXT NOT NULL, options TEXT NOT NULL,
               every_days INTEGER NOT NULL, last_run TEXT, enabled INTEGER NOT NULL DEFAULT 1);",
        )?;
        Ok(History { conn })
    }

    pub fn record(&mut self, o: &Options, outs: &[Outcome], impact: &Impact) -> Result<i64> {
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO jobs(options, card) VALUES (?1, ?2)", params![serde_json::to_string(o)?, impact.lines().join("\n")])?;
        let id = tx.last_insert_rowid();
        {
            let mut ins = tx.prepare("INSERT INTO files(job, src, out, before, after, quality, error) VALUES (?1,?2,?3,?4,?5,?6,?7)")?;
            for r in outs {
                ins.execute(params![
                    id,
                    r.path.to_string_lossy(),
                    r.out.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    r.before as i64,
                    r.after as i64,
                    r.quality,
                    r.error
                ])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn jobs(&self, limit: usize) -> Result<Vec<JobRow>> {
        let mut st = self.conn.prepare(
            "SELECT j.id, j.started, j.options, j.undone, j.card,
                    COUNT(f.job), COALESCE(SUM(f.before),0), COALESCE(SUM(CASE WHEN f.out IS NULL THEN f.before ELSE f.after END),0)
             FROM jobs j LEFT JOIN files f ON f.job = j.id GROUP BY j.id ORDER BY j.id DESC LIMIT ?1",
        )?;
        let rows = st.query_map([limit as i64], |r| {
            let opts: String = r.get(2)?;
            Ok(JobRow {
                id: r.get(0)?,
                started: r.get(1)?,
                mode: serde_json::from_str::<Options>(&opts).map(|o| o.mode).unwrap_or(Mode::Copy),
                undone: r.get::<_, i64>(3)? != 0,
                card: r.get(4)?,
                files: r.get::<_, i64>(5)? as usize,
                before: r.get::<_, i64>(6)? as u64,
                after: r.get::<_, i64>(7)? as u64,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn files(&self, job: i64) -> Result<Vec<FileRow>> {
        let mut st = self.conn.prepare("SELECT src, out, before, after, quality, error FROM files WHERE job = ?1")?;
        let rows = st.query_map([job], |r| {
            Ok(FileRow {
                src: PathBuf::from(r.get::<_, String>(0)?),
                out: r.get::<_, Option<String>>(1)?.map(PathBuf::from),
                before: r.get::<_, i64>(2)? as u64,
                after: r.get::<_, i64>(3)? as u64,
                quality: r.get::<_, i64>(4)? as u8,
                error: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Copy: delete the outputs. Archive: delete outputs, move originals back. Replace: nothing to undo.
    /// Returns the number of files restored.
    pub fn undo(&mut self, job: i64) -> Result<usize> {
        let (opts, undone): (String, i64) = self
            .conn
            .query_row("SELECT options, undone FROM jobs WHERE id = ?1", [job], |r| Ok((r.get(0)?, r.get(1)?)))
            .with_context(|| format!("job {job} not found"))?;
        if undone != 0 {
            bail!("job {job} already undone");
        }
        let mode = serde_json::from_str::<Options>(&opts)?.mode;
        if mode == Mode::Replace {
            bail!("replace jobs delete the originals; nothing to restore");
        }
        let mut n = 0;
        for f in self.files(job)? {
            let Some(out) = f.out else { continue };
            if mode == Mode::Archive {
                let archived = f.src.parent().unwrap_or(Path::new(".")).join("_archive").join(f.src.file_name().unwrap_or_default());
                if !archived.exists() {
                    continue; // original gone; leave the output rather than lose both
                }
                if out.exists() {
                    fs::remove_file(&out)?;
                }
                fs::rename(&archived, &f.src)?;
            } else if out.exists() {
                fs::remove_file(&out)?;
            }
            n += 1;
        }
        self.conn.execute("UPDATE jobs SET undone = 1 WHERE id = ?1", [job])?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::Format;
    use crate::inspect::Kind;

    #[test]
    fn record_list_undo_copy() {
        let dir = std::env::temp_dir().join(format!("forge-hist-{}", std::process::id()));
        fs::create_dir_all(dir.join("_compressed")).unwrap();
        let out = dir.join("_compressed/a.jpg");
        fs::write(&out, b"x").unwrap();
        let mut h = History::open(&dir.join("h.db")).unwrap();
        let o = Options::default();
        let outs = vec![Outcome {
            path: dir.join("a.png"),
            out: Some(out.clone()),
            kind: Kind::Png,
            format: Format::Jpeg,
            before: 100,
            after: 10,
            quality: 82,
            width: 1,
            height: 1,
            via: "test",
            had_icc: false,
            had_exif: false,
            had_gps: false,
            icc_converted: false,
            bits: 8,
            ms: 1,
            error: None,
        }];
        let id = h.record(&o, &outs, &Impact::from_outcomes(&outs, &o)).unwrap();
        let jobs = h.jobs(10).unwrap();
        assert_eq!((jobs[0].id, jobs[0].files, jobs[0].before, jobs[0].after), (id, 1, 100, 10));
        assert_eq!(h.undo(id).unwrap(), 1);
        assert!(!out.exists());
        assert!(h.undo(id).is_err());
        drop(h);
        fs::remove_dir_all(&dir).unwrap();
    }
}
