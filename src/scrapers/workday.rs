/// Generic scraper for Workday-hosted career portals.
///
/// The JSON search API is identical across all Workday instances —
/// only the subdomain, Workday instance version, and portal path differ.
///
/// How to find values for a new company:
///   1. Visit their careers page and search for any job.
///   2. The URL will look like:
///        https://{company_id}.wd3.myworkdayjobs.com/{portal}/jobs?q=...
///   3. Some companies use wd5 instead of wd3 (e.g. J&J, Abbott).
///
/// Confirmed portals (verified April 2026):
///   Roche:      company_id="roche",         portal="roche-ext",        wd_instance="wd3"
///   Novartis:   company_id="novartis",       portal="Novartis_Careers", wd_instance="wd3"
///   Lonza:      company_id="lonza",          portal="Lonza_Careers",    wd_instance="wd3"
///   Sanofi:     company_id="sanofi",         portal="SanofiCareers",    wd_instance="wd3"
///   J&J:        company_id="jj",             portal="JJ",               wd_instance="wd5"
///   AstraZeneca: company_id="astrazeneca",   portal="Careers",          wd_instance="wd3"
///   Takeda:     company_id="takeda",         portal="External",         wd_instance="wd3"
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;

/// Location words to strip from search text before sending to Workday.
/// Workday searches job titles/descriptions — location words reduce recall
/// without improving precision (the portal already scopes to that company).
const LOCATION_WORDS: &[&str] = &[
    "Basel", "Switzerland", "Zurich", "Zürich", "Zug", "Geneva", "Genf",
    "Allschwil", "pharma",
];

pub struct WorkdayScraper {
    pub company_id: String,
    pub portal: String,
    pub display_name: String,
    /// Workday instance version. Most companies use "wd3"; J&J and Abbott use "wd5".
    pub wd_instance: String,
}

fn strip_location_words(keyword: &str) -> String {
    let mut result = keyword.to_string();
    for word in LOCATION_WORDS {
        // Replace whole-word occurrences, case-insensitive
        let lower_result = result.to_lowercase();
        let lower_word = word.to_lowercase();
        if let Some(pos) = lower_result.find(&lower_word) {
            // Check it's a word boundary (space before/after or start/end)
            let before_ok = pos == 0 || result.as_bytes()[pos - 1] == b' ';
            let after_pos = pos + word.len();
            let after_ok = after_pos >= result.len() || result.as_bytes()[after_pos] == b' ';
            if before_ok && after_ok {
                result = format!("{}{}", result[..pos].trim_end(), result[after_pos..].trim_start());
            }
        }
    }
    result.trim().to_string()
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
            "https://{id}.{wdi}.myworkdayjobs.com/wday/cxs/{id}/{portal}/jobs",
            id  = self.company_id,
            wdi = self.wd_instance,
            portal = self.portal,
        );
        let apply_base = format!(
            "https://{id}.{wdi}.myworkdayjobs.com/{portal}",
            id  = self.company_id,
            wdi = self.wd_instance,
            portal = self.portal,
        );

        for keyword in keywords {
            // Strip location words — Workday searches job titles/descriptions so
            // "clinical operations Basel" → "clinical operations" gives far more results.
            let search_text = strip_location_words(keyword);
            if search_text.is_empty() {
                continue;
            }

            // locationSearchText tells Workday to pre-filter to Swiss jobs,
            // so all 20 returned slots are Swiss rather than 1-in-20.
            let body = serde_json::json!({
                "appliedFacets": {},
                "limit": 20,
                "offset": 0,
                "searchText": search_text,
                "locationSearchText": "Switzerland",
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
                let url = format!("{apply_base}{path}", path = job.external_path);

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
