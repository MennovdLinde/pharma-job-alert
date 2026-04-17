mod config;
mod models;
mod output;
mod scrapers;

use anyhow::Result;
use models::JobListing;
use scrapers::{
    Scraper,
    pharmiweb::PharmiwebScraper,
    jobs_ch::JobsChScraper,
    workday::WorkdayScraper,
    bayer::BayerScraper,
    csl_vifor::CslViforScraper,
};
use std::collections::HashSet;
use tokio::task::JoinSet;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    info!("Loading config from {config_path}");
    let config = config::Config::load(&config_path)?;

    let keywords = config.search.keywords.clone();
    let location = config.search.location.clone();

    // ── Build scraper list ────────────────────────────────────────────────
    // General boards (LinkedIn, Indeed, BioSpace) removed — they return global results.
    // All sources below are either Swiss job boards or company-direct career pages.
    let mut scrapers: Vec<Box<dyn Scraper + Send + Sync + 'static>> = vec![
        Box::new(PharmiwebScraper),   // pharma-specific; location=Switzerland passed
        Box::new(JobsChScraper),      // Swiss job board
        Box::new(BayerScraper),       // company-direct (Bayer, large Basel site)
        Box::new(CslViforScraper),    // CSL Vifor, Glattbrugg ZH
    ];

    for company in &config.search.workday_companies {
        scrapers.push(Box::new(WorkdayScraper {
            company_id:   company.company_id.clone(),
            portal:       company.portal.clone(),
            display_name: company.display_name.clone(),
            wd_instance:  company.wd_instance.clone(),
        }));
    }

    info!("Running {} scrapers for {} keyword(s)...", scrapers.len(), keywords.len());

    // ── Run all scrapers concurrently ─────────────────────────────────────
    let mut set: JoinSet<Result<Vec<JobListing>>> = JoinSet::new();
    for scraper in scrapers {
        let kws = keywords.clone();
        let loc = location.clone();
        set.spawn(async move { scraper.scrape(&kws, loc.as_deref()).await });
    }

    let mut all_jobs: Vec<JobListing> = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok(Ok(jobs)) => all_jobs.extend(jobs),
            Ok(Err(e)) => tracing::warn!("Scraper error: {e}"),
            Err(e) => tracing::warn!("Scraper task panicked: {e}"),
        }
    }

    info!("Total raw results: {}", all_jobs.len());

    // ── Deduplicate within this run ───────────────────────────────────────
    let mut seen_this_run: HashSet<String> = HashSet::new();
    all_jobs.retain(|j| seen_this_run.insert(j.id.clone()));

    // ── Relevance filter ──────────────────────────────────────────────────
    // Company-direct scrapers (Workday portals, Bayer, CSL Vifor): title filter only —
    // the source domain already guarantees a pharma company, so location is trusted.
    // General boards (Pharmiweb, jobs.ch): full title + location filter.
    let before = all_jobs.len();
    all_jobs.retain(|j| {
        let company_direct = j.source.ends_with("(Careers)")
            || j.source == "Bayer"
            || j.source == "CSL Vifor";
        if company_direct {
            config.filter.is_title_relevant(&j.title)
        } else {
            config.filter.is_relevant(&j.title, &j.location)
        }
    });
    let dropped = before - all_jobs.len();
    if dropped > 0 {
        info!("Relevance filter removed {dropped} irrelevant jobs ({} remaining)", all_jobs.len());
    }

    // ── Write all jobs to JSON for the web page ───────────────────────────
    if let Some(json_path) = config.output.json_path.as_deref() {
        if let Err(e) = output::write_jobs_json(json_path, &all_jobs) {
            tracing::warn!("Failed to write jobs JSON: {e}");
        }
    }

    info!("Done. {} jobs written.", all_jobs.len());
    Ok(())
}
