use anyhow::Result;
use rusqlite::{Connection, params};
use std::collections::HashSet;

/// Opens (or creates) the SQLite dedup database and ensures the schema exists.
pub fn open_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS seen_jobs (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            company     TEXT NOT NULL,
            source      TEXT NOT NULL,
            url         TEXT NOT NULL,
            first_seen  TEXT NOT NULL
        );",
    )?;
    Ok(conn)
}

/// Returns the set of job IDs already seen.
pub fn load_seen_ids(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM seen_jobs")?;
    let ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(ids)
}

/// Persists newly seen jobs into the database.
pub fn mark_seen(conn: &Connection, job: &crate::models::JobListing) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO seen_jobs (id, title, company, source, url, first_seen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            job.id,
            job.title,
            job.company,
            job.source,
            job.url,
            job.scraped_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Prunes entries older than `days` days to keep the DB small.
pub fn prune_old(conn: &Connection, days: u32) -> Result<usize> {
    let deleted = conn.execute(
        &format!(
            "DELETE FROM seen_jobs WHERE first_seen < datetime('now', '-{days} days')"
        ),
        [],
    )?;
    Ok(deleted)
}
