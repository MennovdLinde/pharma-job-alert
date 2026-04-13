/// Scraper for Bayer's career portal (career.bayer.com).
///
/// Bayer uses SAP SuccessFactors. Their public search endpoint returns JSON
/// and is accessible without authentication.
///
/// Endpoint:
///   GET https://career.bayer.com/api/jobSearch?
///       query={keyword}&location={location}&pageSize=20&pageNumber=1
///
/// If the API endpoint changes (it's undocumented), the HTML fallback parser
/// is also included and will activate automatically.
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use urlencoding::encode;

pub struct BayerScraper;

// ── SuccessFactors JSON response types ─────────────────────────────────────

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SfResponse {
    #[serde(default)]
    job_postings: Vec<SfJob>,
    // Bayer's API may also use "results" or "data" — both are tried below
    #[serde(default)]
    results: Vec<SfJob>,
    #[serde(default)]
    data: Vec<SfJob>,
}

impl SfResponse {
    fn jobs(self) -> Vec<SfJob> {
        if !self.job_postings.is_empty() {
            self.job_postings
        } else if !self.results.is_empty() {
            self.results
        } else {
            self.data
        }
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct SfJob {
    #[serde(alias = "jobTitle", alias = "title", alias = "name")]
    title: Option<String>,
    #[serde(alias = "jobId", alias = "id", alias = "requisitionId")]
    job_id: Option<String>,
    #[serde(alias = "companyName", alias = "company")]
    company_name: Option<String>,
    #[serde(alias = "jobLocation", alias = "location", alias = "city")]
    location: Option<String>,
    #[serde(alias = "postingDate", alias = "startDate", alias = "publishDate")]
    posting_date: Option<String>,
    /// Direct apply URL (not always present — built from job_id as fallback)
    #[serde(alias = "applyUrl", alias = "jobUrl", alias = "url")]
    apply_url: Option<String>,
}

// ── Scraper impl ───────────────────────────────────────────────────────────

#[async_trait]
impl Scraper for BayerScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();
        let loc = location.unwrap_or("Basel");

        for keyword in keywords {
            tracing::info!("Bayer: searching '{keyword}'");

            // Try JSON API first
            let api_url = format!(
                "https://career.bayer.com/api/jobSearch?query={}&location={}&pageSize=20&pageNumber=1",
                encode(keyword),
                encode(loc),
            );

            let json_result = client
                .get(&api_url)
                .header("Accept", "application/json")
                .send()
                .await;

            let jobs = match json_result {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<SfResponse>().await {
                        Ok(data) => {
                            let jobs = data.jobs();
                            tracing::info!("Bayer API: {} results for '{keyword}'", jobs.len());
                            jobs.into_iter()
                                .filter_map(|j| map_sf_job(j))
                                .collect::<Vec<_>>()
                        }
                        Err(e) => {
                            tracing::warn!("Bayer JSON parse failed for '{keyword}': {e} — trying HTML");
                            scrape_bayer_html(&client, keyword, loc).await?
                        }
                    }
                }
                Ok(resp) => {
                    tracing::warn!("Bayer API returned {} for '{keyword}' — trying HTML", resp.status());
                    scrape_bayer_html(&client, keyword, loc).await?
                }
                Err(e) => {
                    tracing::warn!("Bayer request failed for '{keyword}': {e} — trying HTML");
                    scrape_bayer_html(&client, keyword, loc).await?
                }
            };

            tracing::info!("Bayer: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        }

        Ok(all_jobs)
    }
}

fn map_sf_job(j: SfJob) -> Option<JobListing> {
    let title = j.title?.trim().to_string();
    if title.is_empty() {
        return None;
    }

    let url = j.apply_url.unwrap_or_else(|| {
        j.job_id
            .as_deref()
            .map(|id| format!("https://career.bayer.com/en/jobs/{id}"))
            .unwrap_or_else(|| "https://career.bayer.com/en/jobs".to_string())
    });

    Some(JobListing {
        id: make_id("bayer", &url),
        title,
        company: j.company_name.unwrap_or_else(|| "Bayer".to_string()),
        location: j.location.unwrap_or_default(),
        url,
        source: "Bayer (Careers)".to_string(),
        posted_at: j.posting_date,
        scraped_at: Utc::now(),
        description_snippet: None,
    })
}

// ── HTML fallback ──────────────────────────────────────────────────────────

async fn scrape_bayer_html(
    client: &reqwest::Client,
    keyword: &str,
    location: &str,
) -> Result<Vec<JobListing>> {
    use scraper::{Html, Selector};

    // Bayer's server-rendered search page (no JS required for initial results)
    let url = format!(
        "https://career.bayer.com/en/jobs.html?search={}&location={}",
        encode(keyword),
        encode(location),
    );

    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => {
            tracing::warn!("Bayer HTML returned {}", r.status());
            return Ok(vec![]);
        }
        Err(e) => {
            tracing::warn!("Bayer HTML request failed: {e}");
            return Ok(vec![]);
        }
    };

    let html = resp.text().await?;
    let document = Html::parse_document(&html);
    let mut jobs = Vec::new();

    let card_sel = Selector::parse(
        "div[class*='jobCard'], article[class*='job'], li[class*='job-item'], \
         div[data-job-id], div[class*='result-item']",
    )
    .unwrap();
    let title_sel = Selector::parse("h2, h3, [class*='jobTitle'], [class*='job-title']").unwrap();
    let company_sel = Selector::parse("[class*='companyName'], [class*='company']").unwrap();
    let location_sel = Selector::parse("[class*='location'], [class*='city']").unwrap();
    let link_sel = Selector::parse("a[href]").unwrap();

    for card in document.select(&card_sel) {
        let title = card
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let href = card
            .select(&link_sel)
            .next()
            .and_then(|el| el.value().attr("href"))
            .unwrap_or("");

        let job_url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("https://career.bayer.com{href}")
        };

        let company = card
            .select(&company_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "Bayer".to_string());

        let location = card
            .select(&location_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        jobs.push(JobListing {
            id: make_id("bayer", &job_url),
            title,
            company,
            location,
            url: job_url,
            source: "Bayer (Careers)".to_string(),
            posted_at: None,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    Ok(jobs)
}
