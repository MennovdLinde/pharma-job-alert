mod config;
mod dedup;
mod email;
mod models;
mod output;
mod scrapers;

use anyhow::Result;
use models::JobListing;
use scrapers::{
    Scraper,
    pharmiweb::PharmiwebScraper,
    linkedin::LinkedInScraper,
    indeed::IndeedScraper,
    jobs_ch::JobsChScraper,
    workday::WorkdayScraper,
    bayer::BayerScraper,
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

    let args: Vec<String> = std::env::args().collect();
    let dry_run = args.contains(&"--dry-run".to_string());
    if dry_run {
        info!("DRY RUN mode — results printed to stdout, no email sent, no DB writes");
    }

    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
    info!("Loading config from {config_path}");
    let config = config::Config::load(&config_path)?;

    let db_path = config.db_path.as_deref().unwrap_or("seen_jobs.db");
    let conn = dedup::open_db(db_path)?;

    let pruned = dedup::prune_old(&conn, 60)?;
    if pruned > 0 {
        info!("Pruned {pruned} stale job records from DB");
    }

    let seen_ids = dedup::load_seen_ids(&conn)?;
    info!("Loaded {} already-seen job IDs", seen_ids.len());

    let keywords = config.search.keywords.clone();
    let location = config.search.location.clone();

    // ── Build scraper list ────────────────────────────────────────────────
    // To add a new scraper: append one line here.
    let mut scrapers: Vec<Box<dyn Scraper + Send + Sync + 'static>> = vec![
        Box::new(PharmiwebScraper),
        Box::new(LinkedInScraper),
        Box::new(IndeedScraper),
        Box::new(JobsChScraper),
        Box::new(BayerScraper),
    ];

    // Workday companies loaded from config — add companies in config.toml, zero code changes needed
    for company in &config.search.workday_companies {
        scrapers.push(Box::new(WorkdayScraper {
            company_id: company.company_id.clone(),
            portal: company.portal.clone(),
            display_name: company.display_name.clone(),
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
    let before = all_jobs.len();
    all_jobs.retain(|j| config.filter.is_relevant(&j.title));
    let dropped = before - all_jobs.len();
    if dropped > 0 {
        info!("Relevance filter removed {dropped} irrelevant jobs ({} remaining)", all_jobs.len());
    }

    // ── Write all jobs to JSON for the web page ───────────────────────────
    // This happens on every run so Vercel always has fresh data.
    // It writes ALL deduplicated jobs, not just "new" ones.
    if let Some(json_path) = config.output.json_path.as_deref() {
        if let Err(e) = output::write_jobs_json(json_path, &all_jobs) {
            tracing::warn!("Failed to write jobs JSON: {e}");
        }
    }

    if dry_run {
        println!("\n{}", serde_json::to_string_pretty(&all_jobs)?);
        info!("Dry run complete — {} jobs found.", all_jobs.len());
        return Ok(());
    }

    // ── Email: only alert on jobs not seen before ─────────────────────────
    let new_jobs: Vec<_> = all_jobs
        .into_iter()
        .filter(|j| !seen_ids.contains(&j.id))
        .collect();

    info!("{} genuinely new jobs for email alert", new_jobs.len());

    if new_jobs.is_empty() {
        info!("No new jobs to email.");
        return Ok(());
    }

    // Persist before sending so a send failure doesn't cause re-alerts
    for job in &new_jobs {
        dedup::mark_seen(&conn, job)?;
    }

    email::send_digest(&config.email, &new_jobs).await?;
    info!("Done. Digest sent to {}", config.email.to_address);

    Ok(())
}
