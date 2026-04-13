/// Scraper for Pharmiweb.com — a pharma-specific job board with plain HTML listings.
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct PharmiwebScraper;

#[async_trait]
impl Scraper for PharmiwebScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let loc = location.unwrap_or("Switzerland");
            let url = format!(
                "https://www.pharmiweb.jobs/jobs/?keywords={}&location={}",
                encode(keyword),
                encode(loc)
            );

            tracing::info!("Pharmiweb: fetching {url}");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Pharmiweb request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("Pharmiweb returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_pharmiweb(&html, keyword);
            tracing::info!("Pharmiweb: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);
        }

        Ok(all_jobs)
    }
}

fn parse_pharmiweb(html: &str, _keyword: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // Selectors verified against live pharmiweb.jobs HTML (April 2026)
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

        // href contains leading/trailing whitespace — must trim
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
            format!("https://www.pharmiweb.jobs{href}")
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
            id: make_id("pharmiweb", &url),
            title,
            company,
            location,
            url,
            source: "Pharmiweb".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
