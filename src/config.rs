use anyhow::Result;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub email: EmailConfig,
    pub search: SearchConfig,
    pub db_path: Option<String>,
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
}

impl FilterConfig {
    pub fn is_relevant(&self, title: &str) -> bool {
        let lower = title.to_lowercase();

        // Blocklist check — drop if any excluded term is found
        for term in &self.title_excludes_any {
            if lower.contains(&term.to_lowercase()) {
                return false;
            }
        }

        // Allowlist check — must match at least one required term (if list is non-empty)
        if !self.title_contains_any.is_empty() {
            return self
                .title_contains_any
                .iter()
                .any(|t| lower.contains(&t.to_lowercase()));
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
pub struct EmailConfig {
    /// SendGrid API key (set via env var SENDGRID_API_KEY or in config)
    pub sendgrid_api_key: Option<String>,
    /// Fallback: plain SMTP (e.g. Gmail)
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_username: Option<String>,
    pub smtp_password: Option<String>,
    pub from_address: String,
    pub to_address: String,
    pub subject_prefix: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SearchConfig {
    /// Keyword combos, e.g. ["clinical operations Basel", "global study manager Switzerland"]
    pub keywords: Vec<String>,
    /// Location filter appended to searches
    pub location: Option<String>,
    /// Workday-hosted company career portals to search.
    /// Find the portal name from the company's Workday URL:
    ///   https://{company_id}.wd3.myworkdayjobs.com/{portal}/...
    #[serde(default)]
    pub workday_companies: Vec<WorkdayCompany>,
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

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;

        // Override with environment variables when available
        if let Ok(key) = std::env::var("SENDGRID_API_KEY") {
            config.email.sendgrid_api_key = Some(key);
        }
        if let Ok(smtp_pass) = std::env::var("SMTP_PASSWORD") {
            config.email.smtp_password = Some(smtp_pass);
        }
        if let Ok(to_addr) = std::env::var("ALERT_EMAIL_TO") {
            config.email.to_address = to_addr;
        }

        Ok(config)
    }
}
