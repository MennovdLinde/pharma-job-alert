use crate::models::JobListing;
use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

#[derive(Serialize)]
struct JobsFile<'a> {
    scraped_at: String,
    total: usize,
    jobs: &'a [JobListing],
}

/// Writes all scraped jobs to `path` as pretty-printed JSON.
/// Creates parent directories if needed.
pub fn write_jobs_json(path: &str, jobs: &[JobListing]) -> Result<()> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = JobsFile {
        scraped_at: Utc::now().to_rfc3339(),
        total: jobs.len(),
        jobs,
    };

    let json = serde_json::to_string_pretty(&payload)?;
    std::fs::write(path, &json)?;
    tracing::info!("Wrote {} jobs to {path}", jobs.len());
    Ok(())
}
