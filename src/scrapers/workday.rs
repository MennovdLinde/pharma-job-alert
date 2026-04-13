/// Generic scraper for Workday-hosted career portals.
///
/// Most large pharma companies use Workday. The JSON search API is the same
/// across all of them — only the subdomain and portal path differ.
///
/// How to find the correct values for a new company:
///   1. Go to their careers page and search for a job.
///   2. The URL will look like:
///        https://{company_id}.wd3.myworkdayjobs.com/{portal}/jobs?q=...
///   3. Use those two values in config.toml.
///
/// Confirmed portals (as of early 2026):
///   Roche:   company_id="roche",    portal="Roche-Careers"
///   Novartis: company_id="novartis", portal="novartis"
///   Lonza:   company_id="lonza",    portal="lonza-careers"
///   Sanofi:  company_id="sanofi",   portal="Sanofi"
///   UCB:     company_id="ucb",      portal="UCBCareers"
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;

pub struct WorkdayScraper {
    pub company_id: String,
    pub portal: String,
    pub display_name: String,
}

// ── Workday JSON response types ────────────────────────────────────────────

#[derive(Deserialize)]
struct WorkdayResponse {
    #[serde(rename = "jobPostings", default)]
    job_postings: Vec<WorkdayJob>,
}

#[derive(Deserialize)]
struct WorkdayJob {
    title: String,
    /// Relative path used to build the apply URL, e.g. "/job/Basel/Manager_R12345"
    #[serde(rename = "externalPath")]
    external_path: String,
    #[serde(rename = "locationsText")]
    locations_text: Option<String>,
    /// Human string like "Posted 2 Days Ago"
    #[serde(rename = "postedOn")]
    posted_on: Option<String>,
    /// Extra metadata chips (employment type, department, etc.)
    #[serde(rename = "bulletFields", default)]
    bullet_fields: Vec<String>,
}

// ── Scraper impl ───────────────────────────────────────────────────────────

#[async_trait]
impl Scraper for WorkdayScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        let api_url = format!(
            "https://{id}.wd3.myworkdayjobs.com/wday/cxs/{id}/{portal}/jobs",
            id = self.company_id,
            portal = self.portal,
        );

        for keyword in keywords {
            // Optionally append location to search text — Workday has no separate location field
            // in the free-text search; filtering by location ID requires a separate lookup.
            let search_text = match location {
                Some(loc) if !keyword.to_lowercase().contains("basel")
                          && !keyword.to_lowercase().contains("switzerland") =>
                {
                    format!("{keyword} {loc}")
                }
                _ => keyword.clone(),
            };

            let body = serde_json::json!({
                "appliedFacets": {},
                "limit": 20,
                "offset": 0,
                "searchText": search_text,
            });

            tracing::info!("{}: searching '{}'", self.display_name, keyword);

            let resp = match client
                .post(&api_url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("{} request failed for '{}': {e}", self.display_name, keyword);
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("{} returned {} for '{}'", self.display_name, resp.status(), keyword);
                continue;
            }

            let data: WorkdayResponse = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("{} JSON parse error for '{}': {e}", self.display_name, keyword);
                    continue;
                }
            };

            let count = data.job_postings.len();
            tracing::info!("{}: {} results for '{}'", self.display_name, count, keyword);

            for job in data.job_postings {
                let url = format!(
                    "https://{id}.wd3.myworkdayjobs.com/{portal}{path}",
                    id = self.company_id,
                    portal = self.portal,
                    path = job.external_path,
                );

                let snippet = if job.bullet_fields.is_empty() {
                    None
                } else {
                    Some(job.bullet_fields.join(" · "))
                };

                all_jobs.push(JobListing {
                    id: make_id(&self.display_name, &url),
                    title: job.title,
                    company: self.display_name.clone(),
                    location: job.locations_text.unwrap_or_default(),
                    url,
                    source: format!("{} (Careers)", self.display_name),
                    posted_at: job.posted_on,
                    scraped_at: Utc::now(),
                    description_snippet: snippet,
                });
            }

            // Polite delay between keyword searches
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        }

        Ok(all_jobs)
    }
}
