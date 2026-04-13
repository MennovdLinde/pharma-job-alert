use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobListing {
    /// Stable unique ID: SHA-256 of (source + url or title+company)
    pub id: String,
    pub title: String,
    pub company: String,
    pub location: String,
    pub url: String,
    pub source: String,
    pub posted_at: Option<String>,
    pub scraped_at: DateTime<Utc>,
    pub description_snippet: Option<String>,
}
