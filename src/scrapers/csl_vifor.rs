/// Scraper for CSL Vifor jobs portal (jobs.csl.com).
/// CSL Vifor is the Swiss subsidiary of CSL Limited, HQ Glattbrugg ZH.
/// The portal uses the Ruby/Rails "filterrific" gem — plain HTML, no JS required.
///
/// ⚠ Selectors verified visually April 2026 but NOT via live fetch (SSL issue in dev).
///   If scraper returns 0 results: open jobs.csl.com/en/jobs in browser → DevTools →
///   inspect a job card → update selectors below.
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct CslViforScraper;

const BASE_URL: &str = "https://jobs.csl.com/en/jobs";

#[async_trait]
impl Scraper for CslViforScraper {
    async fn scrape(&self, keywords: &[String], _location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            // filterrific param names from the live URL in companies.txt
            let url = format!(
                "{BASE_URL}?filterrific%5Bsearch_by_title_and_description%5D={kw}\
                 &filterrific%5Bwith_location3%5D=switzerland",
                kw = encode(keyword),
            );

            tracing::info!("CSL Vifor: fetching '{keyword}'");

            let resp = match client
                .get(&url)
                .header("Accept", "text/html")
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("CSL Vifor request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("CSL Vifor returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_csl(&html);
            tracing::info!("CSL Vifor: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }

        Ok(all_jobs)
    }
}

fn parse_csl(html: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // ── Selectors ─────────────────────────────────────────────────────────
    // Tune these if scraper returns 0: inspect a job card on jobs.csl.com/en/jobs
    // and replace with the actual element/class names you see.
    let card_sel = Selector::parse(
        ".job-listing, .job-item, .vacancy, .position, article.job, div.job",
    )
    .unwrap();

    let title_sel    = Selector::parse("h2 a, h3 a, .job-title a, .title a").unwrap();
    let company_sel  = Selector::parse(".company, .employer, .recruiter").unwrap();
    let location_sel = Selector::parse(".location, .job-location, .city").unwrap();
    let date_sel     = Selector::parse(".date, .posted, time").unwrap();

    for card in document.select(&card_sel) {
        let title_el = card.select(&title_sel).next();

        let title = title_el
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let href = title_el
            .and_then(|el| el.value().attr("href"))
            .unwrap_or("")
            .trim()
            .to_string();

        let url = if href.starts_with("http") {
            href
        } else if href.is_empty() {
            continue;
        } else {
            format!("https://jobs.csl.com{href}")
        };

        let company = card
            .select(&company_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "CSL Vifor".to_string());

        let location = card
            .select(&location_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "Switzerland".to_string());

        let posted_at = card
            .select(&date_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string());

        jobs.push(JobListing {
            id: make_id("csl_vifor", &url),
            title,
            company,
            location,
            url,
            source: "CSL Vifor".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
