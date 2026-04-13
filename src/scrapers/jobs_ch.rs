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

    let card_sel = Selector::parse(
        "article[data-cy='vacancy-item'], \
         div[class*='JobCard'], \
         li[class*='vacancy']",
    )
    .unwrap();
    let title_sel = Selector::parse("h2, h3, [class*='jobTitle'], [data-cy='vacancy-title']").unwrap();
    let company_sel =
        Selector::parse("[class*='companyName'], [data-cy='vacancy-company'], span[class*='company']")
            .unwrap();
    let location_sel =
        Selector::parse("[class*='location'], [data-cy='vacancy-location'], span[class*='city']").unwrap();
    let link_sel = Selector::parse("a[href*='/vacancies/']").unwrap();
    let date_sel = Selector::parse("time, [class*='date'], [data-cy='publication-date']").unwrap();

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
            format!("https://www.jobs.ch{href}")
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
            .and_then(|el| {
                el.value()
                    .attr("datetime")
                    .map(|s| s.to_string())
                    .or_else(|| Some(el.text().collect::<String>().trim().to_string()))
            });

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
