//! Recurring rules: folders + options + cadence, stored beside the history.
//! The OS scheduler (Task Scheduler / launchd / systemd timer) just calls `run_due`.
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::history::History;
use crate::impact::Impact;
use crate::job::{Event, Options, plan, run};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: i64,
    pub name: String,
    pub paths: Vec<PathBuf>,
    pub options: Options,
    pub every_days: u32,
    pub last_run: Option<String>,
    pub enabled: bool,
}

impl History {
    pub fn add_rule(&self, name: &str, paths: &[PathBuf], o: &Options, every_days: u32) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO rules(name, paths, options, every_days) VALUES (?1, ?2, ?3, ?4)",
            params![name, serde_json::to_string(paths)?, serde_json::to_string(o)?, every_days],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_rule(&self, id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM rules WHERE id = ?1", [id])? > 0)
    }

    pub fn set_rule_enabled(&self, id: i64, enabled: bool) -> Result<bool> {
        Ok(self.conn.execute("UPDATE rules SET enabled = ?2 WHERE id = ?1", params![id, enabled as i64])? > 0)
    }

    pub fn rules(&self) -> Result<Vec<Rule>> {
        self.rules_where("1")
    }

    pub fn due_rules(&self) -> Result<Vec<Rule>> {
        self.rules_where("enabled = 1 AND (last_run IS NULL OR julianday('now') - julianday(last_run) >= every_days)")
    }

    fn rules_where(&self, cond: &str) -> Result<Vec<Rule>> {
        let mut st = self.conn.prepare(&format!("SELECT id, name, paths, options, every_days, last_run, enabled FROM rules WHERE {cond} ORDER BY id"))?;
        let rows = st.query_map([], |r| {
            let paths: String = r.get(2)?;
            let options: String = r.get(3)?;
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, paths, options, r.get::<_, i64>(4)?, r.get::<_, Option<String>>(5)?, r.get::<_, i64>(6)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, paths, options, every, last_run, enabled) = row?;
            out.push(Rule {
                id,
                name,
                paths: serde_json::from_str(&paths)?,
                options: serde_json::from_str(&options)?,
                every_days: every as u32,
                last_run,
                enabled: enabled != 0,
            });
        }
        Ok(out)
    }

    /// Run every due rule, record each as a job. Returns (rule id, job id, impact) per run.
    pub fn run_due(&mut self, on: &(dyn Fn(Event) + Sync)) -> Result<Vec<(i64, i64, Impact)>> {
        let mut done = Vec::new();
        for r in self.due_rules()? {
            let p = plan(&r.paths, &r.options);
            let outs = run(&p, &r.options, on);
            let impact = Impact::from_outcomes(&outs, &r.options);
            let job = self.record(&r.options, &outs, &impact)?;
            self.conn.execute("UPDATE rules SET last_run = datetime('now') WHERE id = ?1", [r.id])?;
            done.push((r.id, job, impact));
        }
        Ok(done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rule_is_due_once_per_period() {
        let dir = std::env::temp_dir().join(format!("forge-rules-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut h = History::open(&dir.join("h.db")).unwrap();
        let id = h.add_rule("nightly", &[dir.clone()], &Options::default(), 1).unwrap();
        assert_eq!(h.due_rules().unwrap().len(), 1);
        let ran = h.run_due(&|_| {}).unwrap();
        assert_eq!(ran[0].0, id);
        assert!(h.due_rules().unwrap().is_empty());
        assert!(h.remove_rule(id).unwrap());
        drop(h);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
