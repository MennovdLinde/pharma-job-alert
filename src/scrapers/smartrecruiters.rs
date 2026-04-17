/// Generic scraper for companies using SmartRecruiters ATS.
/// Uses the public SmartRecruiters JSON API — no auth required.
///
/// API reference:
///   GET https://api.smartrecruiters.com/v1/companies/{company_id}/postings
///   Params: keyword, country (ISO 3166-1 alpha-2), limit, offset
///
/// Confirmed portals (verified April 2026):
///   Straumann: company_id="StraumannGroup1"
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use urlencoding::encode;

pub struct SmartRecruitersScraper {
    pub company_id: String,
    pub display_name: String,
}

// ── API response types ─────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SrResponse {
    #[serde(default)]
    content: Vec<SrJob>,
}

#[derive(Deserialize)]
struct SrJob {
    id: Option<String>,
    name: Option<String>,
    #[serde(rename = "releasedDate")]
    released_date: Option<String>,
    location: Option<SrLocation>,
    #[serde(rename = "ref")]
    job_ref: Option<String>,
}

#[derive(Deserialize)]
struct SrLocation {
    city: Option<String>,
    country: Option<String>,
    region: Option<String>,
}

// ── Scraper impl ───────────────────────────────────────────────────────────

#[async_trait]
impl Scraper for SmartRecruitersScraper {
    async fn scrape(&self, keywords: &[String], _location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let url = format!(
                "https://api.smartrecruiters.com/v1/companies/{company}/postings\
                 ?keyword={kw}&country=CH&limit=100",
                company = self.company_id,
                kw = encode(keyword),
            );

            tracing::info!("{}: searching '{keyword}'", self.display_name);

            let resp = match client
                .get(&url)
                .header("Accept", "application/json")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("{} request failed for '{keyword}': {e}", self.display_name);
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("{} returned {} for '{keyword}'", self.display_name, resp.status());
                continue;
            }

            let data: SrResponse = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!("{} JSON parse error for '{keyword}': {e}", self.display_name);
                    continue;
                }
            };

            let count = data.content.len();
            tracing::info!("{}: {} results for '{keyword}'", self.display_name, count);

            for job in data.content {
                let title = match job.name {
                    Some(t) if !t.is_empty() => t,
                    _ => continue,
                };

                let url = match &job.job_ref {
                    Some(r) if !r.is_empty() => r.clone(),
                    _ => match &job.id {
                        Some(id) => format!(
                            "https://jobs.smartrecruiters.com/{}/{id}",
                            self.company_id
                        ),
                        None => continue,
                    },
                };

                let location = job.location.as_ref().map(|l| {
                    [
                        l.city.as_deref().unwrap_or(""),
                        l.region.as_deref().unwrap_or(""),
                        l.country.as_deref().unwrap_or(""),
                    ]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
                }).unwrap_or_default();

                let posted_at = job.released_date.map(|d| {
                    // trim to date part "2026-04-15T..." → "2026-04-15"
                    d.chars().take(10).collect()
                });

                all_jobs.push(JobListing {
                    id: make_id(&self.display_name, &url),
                    title,
                    company: self.display_name.clone(),
                    location,
                    url,
                    source: format!("{} (Careers)", self.display_name),
                    posted_at,
                    scraped_at: Utc::now(),
                    description_snippet: None,
                });
            }

            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        }

        Ok(all_jobs)
    }
}
