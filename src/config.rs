use anyhow::Result;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub search: SearchConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub filter: FilterConfig,
}

/// Relevance filter applied to every scraped job before it reaches jobs.json.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FilterConfig {
    /// At least one of these strings must appear in the job title (case-insensitive).
    /// Leave empty to allow all titles through.
    #[serde(default)]
    pub title_contains_any: Vec<String>,

    /// If any of these strings appear in the job title the job is dropped (case-insensitive).
    #[serde(default)]
    pub title_excludes_any: Vec<String>,

    /// At least one of these strings must appear in the job location (case-insensitive).
    /// Leave empty to allow all locations through.
    #[serde(default)]
    pub location_contains_any: Vec<String>,
}

impl FilterConfig {
    pub fn is_relevant(&self, title: &str, location: &str) -> bool {
        let lower_title = title.to_lowercase();

        // Blocklist check — drop if any excluded term is found in title
        for term in &self.title_excludes_any {
            if lower_title.contains(&term.to_lowercase()) {
                return false;
            }
        }

        // Title allowlist — must match at least one required term (if list is non-empty)
        if !self.title_contains_any.is_empty()
            && !self
                .title_contains_any
                .iter()
                .any(|t| lower_title.contains(&t.to_lowercase()))
        {
            return false;
        }

        // Location filter — job must be in Switzerland (if list is non-empty)
        if !self.location_contains_any.is_empty() {
            let lower_loc = location.to_lowercase();
            return self
                .location_contains_any
                .iter()
                .any(|l| lower_loc.contains(&l.to_lowercase()));
        }

        true
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct OutputConfig {
    /// Write all scraped jobs to this JSON file after each run.
    /// GitHub Actions commits this file so Vercel can serve it.
    pub json_path: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SearchConfig {
    /// Keyword combos, e.g. ["clinical operations Basel", "global study manager Switzerland"]
    pub keywords: Vec<String>,
    /// Location filter appended to searches
    pub location: Option<String>,
    /// Workday-hosted company career portals to search.
    #[serde(default)]
    pub workday_companies: Vec<WorkdayCompany>,
    /// SmartRecruiters-hosted company career portals to search.
    #[serde(default)]
    pub smartrecruiters_companies: Vec<SmartRecruitersCompany>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WorkdayCompany {
    /// Workday subdomain, e.g. "roche"
    pub company_id: String,
    /// Portal path segment, e.g. "roche-ext"
    pub portal: String,
    /// Human-readable label shown on the web page
    pub display_name: String,
    /// Workday instance version — most companies use "wd3", J&J/Abbott use "wd5"
    #[serde(default = "default_wd_instance")]
    pub wd_instance: String,
}

fn default_wd_instance() -> String {
    "wd3".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct SmartRecruitersCompany {
    /// SmartRecruiters company identifier, e.g. "StraumannGroup1"
    pub company_id: String,
    /// Human-readable label shown on the web page
    pub display_name: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}
