use crate::config::EmailConfig;
use crate::models::JobListing;
use anyhow::{bail, Result};
use chrono::Utc;

/// Sends the daily digest email. Tries SendGrid API first, falls back to SMTP.
pub async fn send_digest(config: &EmailConfig, jobs: &[JobListing]) -> Result<()> {
    let subject = format!(
        "{} {} new pharma job{}",
        config.subject_prefix.as_deref().unwrap_or("[PharmaAlert]"),
        jobs.len(),
        if jobs.len() == 1 { "" } else { "s" }
    );

    let html_body = build_html(jobs);
    let text_body = build_text(jobs);

    if let Some(api_key) = &config.sendgrid_api_key {
        send_via_sendgrid(api_key, config, &subject, &html_body, &text_body).await
    } else if config.smtp_host.is_some() {
        send_via_smtp(config, &subject, &html_body, &text_body).await
    } else {
        bail!("No email transport configured. Set SENDGRID_API_KEY or smtp_host in config.");
    }
}

async fn send_via_sendgrid(
    api_key: &str,
    config: &EmailConfig,
    subject: &str,
    html: &str,
    text: &str,
) -> Result<()> {
    let payload = serde_json::json!({
        "personalizations": [{"to": [{"email": config.to_address}]}],
        "from": {"email": config.from_address},
        "subject": subject,
        "content": [
            {"type": "text/plain", "value": text},
            {"type": "text/html",  "value": html}
        ]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.sendgrid.com/v3/mail/send")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await?;

    if resp.status().is_success() {
        tracing::info!("Email sent via SendGrid to {}", config.to_address);
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("SendGrid error {status}: {body}");
    }
}

async fn send_via_smtp(
    config: &EmailConfig,
    subject: &str,
    html: &str,
    _text: &str,
) -> Result<()> {
    use lettre::message::{header::ContentType, MultiPart, SinglePart};
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

    let email = Message::builder()
        .from(config.from_address.parse()?)
        .to(config.to_address.parse()?)
        .subject(subject)
        .multipart(MultiPart::alternative().singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(html.to_string()),
        ))?;

    let host = config.smtp_host.as_deref().unwrap_or("smtp.gmail.com");
    let port = config.smtp_port.unwrap_or(587);

    let creds = Credentials::new(
        config.smtp_username.clone().unwrap_or_default(),
        config.smtp_password.clone().unwrap_or_default(),
    );

    let mailer: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::relay(host)?
            .port(port)
            .credentials(creds)
            .build();

    mailer.send(email).await?;
    tracing::info!("Email sent via SMTP to {}", config.to_address);
    Ok(())
}

fn build_html(jobs: &[JobListing]) -> String {
    let today = Utc::now().format("%A, %B %d %Y");
    let rows: String = jobs
        .iter()
        .map(|j| {
            let posted = j.posted_at.as_deref().unwrap_or("—");
            let snippet = j.description_snippet.as_deref().unwrap_or("");
            format!(
                r#"<tr>
  <td style="padding:12px 8px;border-bottom:1px solid #eee;">
    <a href="{url}" style="font-size:15px;font-weight:600;color:#0a66c2;text-decoration:none;">{title}</a>
    {snippet_html}
  </td>
  <td style="padding:12px 8px;border-bottom:1px solid #eee;white-space:nowrap;">{company}</td>
  <td style="padding:12px 8px;border-bottom:1px solid #eee;white-space:nowrap;">{location}</td>
  <td style="padding:12px 8px;border-bottom:1px solid #eee;white-space:nowrap;color:#666;">{source}</td>
  <td style="padding:12px 8px;border-bottom:1px solid #eee;white-space:nowrap;color:#999;font-size:12px;">{posted}</td>
</tr>"#,
                url = j.url,
                title = html_escape(&j.title),
                company = html_escape(&j.company),
                location = html_escape(&j.location),
                source = html_escape(&j.source),
                posted = html_escape(posted),
                snippet_html = if snippet.is_empty() {
                    String::new()
                } else {
                    format!(r#"<br><span style="font-size:12px;color:#555;">{}</span>"#, html_escape(snippet))
                },
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><title>Pharma Job Alert</title></head>
<body style="font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:0;padding:20px;background:#f5f5f5;">
  <div style="max-width:900px;margin:0 auto;background:#fff;border-radius:8px;padding:24px;box-shadow:0 2px 8px rgba(0,0,0,.08);">
    <h1 style="margin:0 0 4px;font-size:22px;color:#1a1a1a;">Daily Pharma Job Alert</h1>
    <p style="margin:0 0 20px;color:#666;font-size:14px;">{today} — {count} new listing{plural}</p>
    <table width="100%" cellpadding="0" cellspacing="0" style="border-collapse:collapse;">
      <thead>
        <tr style="background:#f0f4f8;text-align:left;font-size:12px;color:#666;text-transform:uppercase;">
          <th style="padding:8px;">Position</th>
          <th style="padding:8px;white-space:nowrap;">Company</th>
          <th style="padding:8px;white-space:nowrap;">Location</th>
          <th style="padding:8px;white-space:nowrap;">Source</th>
          <th style="padding:8px;white-space:nowrap;">Posted</th>
        </tr>
      </thead>
      <tbody>
        {rows}
      </tbody>
    </table>
    <p style="margin:24px 0 0;font-size:12px;color:#aaa;">
      Scraped automatically · <a href="https://github.com/your-username/pharma-job-alert" style="color:#aaa;">pharma-job-alert</a>
    </p>
  </div>
</body>
</html>"#,
        today = today,
        count = jobs.len(),
        plural = if jobs.len() == 1 { "" } else { "s" },
        rows = rows,
    )
}

fn build_text(jobs: &[JobListing]) -> String {
    let today = Utc::now().format("%Y-%m-%d");
    let mut out = format!("Daily Pharma Job Alert — {today}\n{}\n\n", "=".repeat(40));
    for j in jobs {
        out.push_str(&format!(
            "• {title}\n  {company} | {location} | {source}\n  {url}\n\n",
            title = j.title,
            company = j.company,
            location = j.location,
            source = j.source,
            url = j.url,
        ));
    }
    out
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
