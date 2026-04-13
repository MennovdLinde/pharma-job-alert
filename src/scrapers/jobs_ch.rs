/// Scraper for jobs.ch — the dominant Swiss job board.
/// Uses their public search URL which returns server-rendered HTML.
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct JobsChScraper;

#[async_trait]
impl Scraper for JobsChScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let loc = location.unwrap_or("Basel");
            let url = format!(
                "https://www.jobs.ch/en/vacancies/?term={}&location={}",
                encode(keyword),
                encode(loc)
            );

            tracing::info!("jobs.ch: fetching '{keyword}'");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("jobs.ch request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("jobs.ch returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_jobs_ch(&html);
            tracing::info!("jobs.ch: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        }

        Ok(all_jobs)
    }
}

fn parse_jobs_ch(html: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // Selectors verified against live jobs.ch HTML (April 2026, Next.js SSR)
    // data-cy attributes are stable; utility CSS class names are not.
    let card_sel    = Selector::parse(r#"[data-cy="serp-item"]"#).unwrap();
    let link_sel    = Selector::parse(r#"[data-cy="job-link"]"#).unwrap();
    let company_sel = Selector::parse("p.fw_bold").unwrap();
    let loc_sel     = Selector::parse("p.mb_s12").unwrap();
    let date_sel    = Selector::parse("p.white-space_nowrap").unwrap();

    for card in document.select(&card_sel) {
        let link_el = card.select(&link_sel).next();

        // The <a data-cy="job-link"> carries title= with the exact job title
        let title = link_el
            .and_then(|el| el.value().attr("title"))
            .unwrap_or("")
            .trim()
            .to_string();

        if title.is_empty() {
            continue;
        }

        let href = link_el
            .and_then(|el| el.value().attr("href"))
            .unwrap_or("");

        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("https://www.jobs.ch{href}")
        };

        let company = card
            .select(&company_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        // First p.mb_s12 inside the card is the location
        let location = card
            .select(&loc_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let posted_at = card
            .select(&date_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string());

        jobs.push(JobListing {
            id: make_id("jobs_ch", &url),
            title,
            company,
            location,
            url,
            source: "jobs.ch".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
