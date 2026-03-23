use crate::contracts::CodaContract;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub struct MetricsStore {
    conn: Mutex<Connection>,
}

impl MetricsStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path).context("opening metrics database")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS performances (
                id TEXT PRIMARY KEY,
                namespace TEXT NOT NULL,
                prompt TEXT NOT NULL,
                formation TEXT NOT NULL,
                harmony INTEGER NOT NULL,
                summary TEXT NOT NULL,
                total_duration_ms INTEGER NOT NULL,
                total_tokens_in INTEGER NOT NULL DEFAULT 0,
                total_tokens_out INTEGER NOT NULL DEFAULT 0,
                total_cost_usd REAL NOT NULL DEFAULT 0.0,
                sections_json TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            );
            CREATE INDEX IF NOT EXISTS idx_performances_namespace
                ON performances(namespace);",
        )
        .context("creating schema")?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn save(&self, namespace: &str, prompt: &str, coda: &CodaContract) -> Result<()> {
        let sections_json =
            serde_json::to_string(&coda.sections).context("serializing sections")?;
        let formation = serde_json::to_value(coda.formation)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string();

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO performances
                (id, namespace, prompt, formation, harmony, summary,
                 total_duration_ms, total_tokens_in, total_tokens_out,
                 total_cost_usd, sections_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                coda.performance_id,
                namespace,
                prompt,
                formation,
                coda.harmony,
                coda.summary,
                coda.total_duration_ms as i64,
                coda.total_tokens_in as i64,
                coda.total_tokens_out as i64,
                coda.total_cost_usd,
                sections_json,
            ],
        )
        .context("saving performance")?;

        Ok(())
    }

    pub fn list(&self, namespace: &str) -> Result<Vec<PerformanceSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, namespace, prompt, formation, harmony, total_duration_ms, created_at
             FROM performances WHERE namespace = ?1
             ORDER BY created_at DESC",
        )?;

        let rows = stmt
            .query_map([namespace], |row| {
                Ok(PerformanceSummary {
                    performance_id: row.get(0)?,
                    namespace: row.get(1)?,
                    prompt: row.get(2)?,
                    formation: row.get(3)?,
                    harmony: row.get(4)?,
                    duration_ms: row.get::<_, i64>(5)? as u64,
                    created_at: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    pub fn get(&self, id: &str) -> Result<Option<PerformanceDetail>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, namespace, prompt, formation, harmony, summary,
                    total_duration_ms, total_tokens_in, total_tokens_out,
                    total_cost_usd, sections_json, created_at
             FROM performances WHERE id = ?1",
        )?;

        let result = stmt
            .query_row([id], |row| {
                Ok(PerformanceDetail {
                    performance_id: row.get(0)?,
                    namespace: row.get(1)?,
                    prompt: row.get(2)?,
                    formation: row.get(3)?,
                    harmony: row.get(4)?,
                    summary: row.get(5)?,
                    duration_ms: row.get::<_, i64>(6)? as u64,
                    tokens_in: row.get::<_, i64>(7)? as u64,
                    tokens_out: row.get::<_, i64>(8)? as u64,
                    cost_usd: row.get(9)?,
                    sections_json: row.get(10)?,
                    created_at: row.get(11)?,
                })
            })
            .optional()?;

        Ok(result)
    }

    pub fn summary(&self) -> Result<MetricsSummary> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT COUNT(*), COALESCE(SUM(total_tokens_in), 0),
                    COALESCE(SUM(total_tokens_out), 0), COALESCE(SUM(total_cost_usd), 0.0)
             FROM performances",
        )?;

        stmt.query_row([], |row| {
            Ok(MetricsSummary {
                total_performances: row.get::<_, i64>(0)? as u64,
                total_tokens_in: row.get::<_, i64>(1)? as u64,
                total_tokens_out: row.get::<_, i64>(2)? as u64,
                total_cost_usd: row.get(3)?,
            })
        })
        .context("querying metrics summary")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceSummary {
    pub performance_id: String,
    pub namespace: String,
    pub prompt: String,
    pub formation: String,
    pub harmony: bool,
    pub duration_ms: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PerformanceDetail {
    pub performance_id: String,
    pub namespace: String,
    pub prompt: String,
    pub formation: String,
    pub harmony: bool,
    pub summary: String,
    pub duration_ms: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
    pub sections_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricsSummary {
    pub total_performances: u64,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost_usd: f64,
}
