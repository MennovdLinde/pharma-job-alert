/// Scraper for Indeed Switzerland (ch.indeed.com).
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct IndeedScraper;

#[async_trait]
impl Scraper for IndeedScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let loc = location.unwrap_or("Switzerland");
            // fromage=1 = posted in last 1 day
            let url = format!(
                "https://ch.indeed.com/jobs?q={}&l={}&fromage=1&sort=date",
                encode(keyword),
                encode(loc)
            );

            tracing::info!("Indeed: fetching '{keyword}'");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Indeed request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("Indeed returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_indeed(&html);
            tracing::info!("Indeed: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        }

        Ok(all_jobs)
    }
}

fn parse_indeed(html: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    let card_sel = Selector::parse("div.job_seen_beacon, div.tapItem, td.resultContent").unwrap();
    let title_sel = Selector::parse("h2.jobTitle span[title], h2.jobTitle a span, a.jcs-JobTitle").unwrap();
    let company_sel = Selector::parse("span.companyName, [data-testid='company-name']").unwrap();
    let location_sel = Selector::parse("div.companyLocation, [data-testid='text-location']").unwrap();
    let link_sel = Selector::parse("h2.jobTitle a, a.jcs-JobTitle").unwrap();
    let date_sel = Selector::parse("span.date, [data-testid='myJobsStateDate']").unwrap();

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

        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("https://ch.indeed.com{href}")
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
            id: make_id("indeed", &url),
            title,
            company,
            location,
            url,
            source: "Indeed CH".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
