/// Scraper for BioSpace.com — one of the largest pharma/biotech job boards.
///
/// BioSpace uses the same job listing platform as Pharmiweb (CV-Library engine),
/// so the HTML selectors are identical: li.lister__item, lister__header, etc.
///
/// Base URL: https://jobs.biospace.com (www.biospace.com/jobs redirects here)
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct BiospaceScraper;

#[async_trait]
impl Scraper for BiospaceScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let loc = location.unwrap_or("Switzerland");
            let url = format!(
                "https://jobs.biospace.com/jobs/?q={}&location={}",
                encode(keyword),
                encode(loc),
            );

            tracing::info!("Biospace: fetching '{keyword}'");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Biospace request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("Biospace returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_biospace(&html);
            tracing::info!("Biospace: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        }

        Ok(all_jobs)
    }
}

fn parse_biospace(html: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // BioSpace uses the same CV-Library engine as Pharmiweb (verified April 2026)
    let card_sel     = Selector::parse("li.lister__item").unwrap();
    let title_sel    = Selector::parse("h3.lister__header a span").unwrap();
    let link_sel     = Selector::parse("h3.lister__header a.js-clickable-area-link").unwrap();
    let company_sel  = Selector::parse("li.lister__meta-item--recruiter").unwrap();
    let location_sel = Selector::parse("li.lister__meta-item--location").unwrap();
    let date_sel     = Selector::parse("li.job-actions__action.pipe").unwrap();

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
            .unwrap_or("")
            .trim()
            .to_string();

        let url = if href.starts_with("http") {
            href
        } else {
            format!("https://jobs.biospace.com{href}")
        };

        let company = card
            .select(&company_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let location = card
            .select(&location_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let posted_at = card
            .select(&date_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string());

        jobs.push(JobListing {
            id: make_id("biospace", &url),
            title,
            company,
            location,
            url,
            source: "BioSpace".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
