use anyhow::Result;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub email: EmailConfig,
    pub search: SearchConfig,
    pub db_path: Option<String>,
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
    /// Portal path segment, e.g. "Roche-Careers"
    pub portal: String,
    /// Human-readable label used in the email digest
    pub display_name: String,
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
