pub mod pharmiweb;
pub mod linkedin;
pub mod indeed;
pub mod jobs_ch;
pub mod workday;
pub mod bayer;
pub mod biospace;
pub mod csl_vifor;
pub mod smartrecruiters;

use crate::models::JobListing;
use anyhow::Result;

/// Build a stable job ID from source + url (or title+company as fallback).
pub fn make_id(source: &str, url: &str) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("{source}|{url}");
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8]) // 16 hex chars is plenty
}

/// Shared reqwest client builder — sets a browser-like User-Agent to avoid
/// being blocked by simple bot filters.
pub fn build_client() -> Result<reqwest::Client> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
             AppleWebKit/537.36 (KHTML, like Gecko) \
             Chrome/124.0.0.0 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    Ok(client)
}

/// Trait every scraper must implement.
/// `Send + Sync` bounds are required for use with `tokio::task::JoinSet`.
#[async_trait::async_trait]
pub trait Scraper: Send + Sync {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>>;
}
