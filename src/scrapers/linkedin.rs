/// Scraper for LinkedIn Jobs public search results.
///
/// LinkedIn's public job search page renders results as JSON embedded in a
/// <code> tag or via a lightweight HTML snapshot for non-JS clients.
/// We use the guest search endpoint which doesn't require login.
///
/// NOTE: LinkedIn's anti-bot measures vary. This scraper adds delays and
/// rotates keywords to stay within polite limits. For production use,
/// consider the official LinkedIn Jobs API if volume is high.
use crate::models::JobListing;
use crate::scrapers::{Scraper, build_client, make_id};
use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use scraper::{Html, Selector};
use urlencoding::encode;

pub struct LinkedInScraper;

#[async_trait]
impl Scraper for LinkedInScraper {
    async fn scrape(&self, keywords: &[String], location: Option<&str>) -> Result<Vec<JobListing>> {
        let client = build_client()?;
        let mut all_jobs = Vec::new();

        for keyword in keywords {
            let loc = location.unwrap_or("Switzerland");
            // LinkedIn guest search URL — returns HTML with embedded job cards
            let url = format!(
                "https://www.linkedin.com/jobs/search/?keywords={}&location={}&f_TPR=r86400&sortBy=DD",
                encode(keyword),
                encode(loc)
            );

            tracing::info!("LinkedIn: fetching '{keyword}'");

            let resp = match client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("LinkedIn request failed for '{keyword}': {e}");
                    continue;
                }
            };

            if !resp.status().is_success() {
                tracing::warn!("LinkedIn returned {} for '{keyword}'", resp.status());
                continue;
            }

            let html = resp.text().await?;
            let jobs = parse_linkedin(&html);
            tracing::info!("LinkedIn: {} jobs for '{keyword}'", jobs.len());
            all_jobs.extend(jobs);

            // Polite delay between keyword searches
            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        }

        Ok(all_jobs)
    }
}

fn parse_linkedin(html: &str) -> Vec<JobListing> {
    let document = Html::parse_document(html);
    let mut jobs = Vec::new();

    // LinkedIn guest page selectors
    let card_sel = Selector::parse(
        "li.jobs-search__results-list > div, \
         div.base-card, \
         li.result-card",
    )
    .unwrap();
    let title_sel =
        Selector::parse("h3.base-search-card__title, h3.result-card__title, span.screen-reader-text").unwrap();
    let company_sel =
        Selector::parse("h4.base-search-card__subtitle, a.result-card__subtitle-link").unwrap();
    let location_sel =
        Selector::parse("span.job-search-card__location, span.result-card__location").unwrap();
    let link_sel = Selector::parse("a.base-card__full-link, a.result-card__full-card-link").unwrap();
    let date_sel = Selector::parse("time").unwrap();

    for card in document.select(&card_sel) {
        let title = card
            .select(&title_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        if title.is_empty() {
            continue;
        }

        let url = card
            .select(&link_sel)
            .next()
            .and_then(|el| el.value().attr("href"))
            .map(|h| h.split('?').next().unwrap_or(h).to_string())
            .unwrap_or_default();

        if url.is_empty() {
            continue;
        }

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
            .and_then(|el| el.value().attr("datetime"))
            .map(|s| s.to_string());

        jobs.push(JobListing {
            id: make_id("linkedin", &url),
            title,
            company,
            location,
            url,
            source: "LinkedIn".to_string(),
            posted_at,
            scraped_at: Utc::now(),
            description_snippet: None,
        });
    }

    jobs
}
