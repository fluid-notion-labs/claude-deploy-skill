//! GitHub App token generation — pure Rust, no bash dependency.
//!
//! Reads config from `~/.config/claude-deploy/config[-<org>]` (same format
//! as the bash `claude-deploy` script: shell-style key=value lines).
//!
//! Flow:
//!   1. Parse APP_ID + PEM_PATH from config file
//!   2. Mint a JWT (RS256, iat-60, exp+600, iss=APP_ID)
//!   3. GET /app/installations to find installation for org
//!   4. POST /app/installations/<id>/access_tokens → token + expiry
//!   5. GET /installation/repositories → list of repo full_names

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_id: String,
    pub pem_path: PathBuf,
    pub account_type: String, // "org" | "user"
    pub org: String,          // profile name
}

impl AppConfig {
    /// Load from `~/.config/claude-deploy/config[-<org>]`
    pub fn load(org: &str) -> Result<Self> {
        let config_dir = dirs_config();
        let filename = if org == "default" {
            "config".to_string()
        } else {
            format!("config-{}", org)
        };
        let path = config_dir.join(&filename);
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("read config {:?}", path))?;

        let mut kv: HashMap<String, String> = HashMap::new();
        for line in content.lines() {
            if let Some((k, v)) = line.split_once('=') {
                kv.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
            }
        }

        let app_id = kv.get("APP_ID").cloned()
            .with_context(|| format!("APP_ID missing in {:?}", path))?;
        let pem_path = kv.get("PEM_PATH")
            .map(|p| PathBuf::from(p.replace('~', &home_dir())))
            .with_context(|| "PEM_PATH missing in config")?;
        let account_type = kv.get("ACCOUNT_TYPE").cloned()
            .unwrap_or_else(|| "org".to_string());

        Ok(Self { app_id, pem_path, account_type, org: org.to_string() })
    }

    /// Discover all configured orgs by scanning config files.
    pub fn list_orgs() -> Vec<String> {
        let dir = dirs_config();
        let mut orgs = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "config" {
                    orgs.push("default".to_string());
                } else if let Some(org) = name.strip_prefix("config-") {
                    orgs.push(org.to_string());
                }
            }
        }
        orgs.sort();
        orgs
    }
}

// ---------------------------------------------------------------------------
// JWT
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct JwtClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

fn mint_jwt(config: &AppConfig) -> Result<String> {
    let pem = std::fs::read(&config.pem_path)
        .with_context(|| format!("read PEM {:?}", config.pem_path))?;
    let key = EncodingKey::from_rsa_pem(&pem)
        .context("parse RSA PEM")?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time")?
        .as_secs() as i64;

    let claims = JwtClaims {
        iat: now - 60,
        exp: now + 600,
        iss: config.app_id.clone(),
    };

    let mut header = Header::new(Algorithm::RS256);
    header.typ = Some("JWT".to_string());

    encode(&header, &claims, &key).context("encode JWT")
}

// ---------------------------------------------------------------------------
// GitHub API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Installation {
    id: u64,
    account: Account,
}

#[derive(Deserialize)]
struct Account {
    login: String,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    token: String,
    expires_at: String, // ISO 8601
}

#[derive(Deserialize)]
struct ReposResponse {
    repositories: Vec<Repo>,
}

#[derive(Deserialize)]
struct Repo {
    full_name: String,
}

// ---------------------------------------------------------------------------
// Public result
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GeneratedToken {
    pub org: String,
    pub token: String,
    pub expires: DateTime<Utc>,
    pub repos: Vec<String>,
    pub install_id: u64,
}

impl GeneratedToken {
    /// Render as a tok- file body for the sentinel branch.
    pub fn to_tok_file(&self) -> String {
        let repos = self.repos.iter()
            .map(|r| format!("  {}", r))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "type: token\norg: {}\ntoken: {}\nexpires: {}\ninstall-id: {}\nrepos:\n{}\n",
            self.org,
            self.token,
            self.expires.format("%Y-%m-%dT%H:%M:%SZ"),
            self.install_id,
            repos,
        )
    }
}

/// Generate a fresh GitHub App installation token for the given org.
pub fn generate_token(config: &AppConfig) -> Result<GeneratedToken> {
    let jwt = mint_jwt(config)?;

    let agent = ureq::AgentBuilder::new()
        .user_agent("claude-deploy-sentinel/0.1")
        .build();

    // Find installation for this org
    let installations: Vec<Installation> = agent
        .get("https://api.github.com/app/installations")
        .set("Authorization", &format!("Bearer {}", jwt))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .context("GET /app/installations")?
        .into_json()
        .context("parse installations")?;

    let install = installations.iter()
        .find(|i| i.account.login.to_lowercase() == config.org.to_lowercase())
        .with_context(|| format!("no installation found for org '{}'", config.org))?;

    let install_id = install.id;

    // Exchange for access token
    let tok_resp: AccessTokenResponse = agent
        .post(&format!("https://api.github.com/app/installations/{}/access_tokens", install_id))
        .set("Authorization", &format!("Bearer {}", jwt))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .context("POST /access_tokens")?
        .into_json()
        .context("parse access token response")?;

    let expires = DateTime::parse_from_rfc3339(&tok_resp.expires_at)
        .with_context(|| format!("parse expiry '{}'", tok_resp.expires_at))?
        .with_timezone(&Utc);

    // List repos
    let repos_resp: ReposResponse = agent
        .get("https://api.github.com/installation/repositories")
        .set("Authorization", &format!("token {}", tok_resp.token))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .context("GET /installation/repositories")?
        .into_json()
        .context("parse repos response")?;

    Ok(GeneratedToken {
        org: config.org.clone(),
        token: tok_resp.token,
        expires,
        repos: repos_resp.repositories.into_iter().map(|r| r.full_name).collect(),
        install_id,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn dirs_config() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(home_dir()).join(".config"))
        .join("claude-deploy")
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/root".to_string())
}
