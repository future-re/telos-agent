use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(2);
const CRATES_IO_URL: &str = "https://crates.io/api/v1/crates/telos-cli";
const PYPI_URL: &str = "https://pypi.org/pypi/telos-cli/json";
const USER_AGENT: &str = concat!("telos/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateStatus {
    pub current_version: String,
    pub crates_io: RegistryStatus,
    pub pypi: RegistryStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryStatus {
    Current { latest_version: String },
    Newer { latest_version: String },
    Unavailable,
}

impl RegistryStatus {
    pub fn current(version: impl Into<String>) -> Self {
        Self::Current { latest_version: version.into() }
    }

    pub fn newer(version: impl Into<String>) -> Self {
        Self::Newer { latest_version: version.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCheckCache {
    checked_at_unix_secs: u64,
    crates_io: CachedRegistry,
    pypi: CachedRegistry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum CachedRegistry {
    Available { latest_version: String },
    Unavailable,
}

#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    crate_info: CratesIoCrate,
}

#[derive(Debug, Deserialize)]
struct CratesIoCrate {
    max_version: String,
}

#[derive(Debug, Deserialize)]
struct PypiResponse {
    info: PypiInfo,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    version: String,
}

pub async fn maybe_print_update_notice(current_version: &str, quiet: bool) {
    if !should_check_updates(quiet, std::env::var("TELOS_DISABLE_UPDATE_CHECK").ok().as_deref()) {
        return;
    }

    let status = match load_cached_status(current_version) {
        Some(status) => status,
        None => match fetch_status(current_version).await {
            Some(status) => status,
            None => return,
        },
    };

    if let Some(notice) = format_update_notice(&status) {
        eprintln!("{notice}");
    }
}

pub fn format_update_notice(status: &UpdateStatus) -> Option<String> {
    let mut lines = Vec::new();

    if let RegistryStatus::Newer { latest_version } = &status.crates_io {
        lines
            .push(format!("  crates.io: {latest_version} (run `cargo install --force telos-cli`)"));
    }

    if let RegistryStatus::Newer { latest_version } = &status.pypi {
        lines.push(format!("  PyPI: {latest_version} (run `pip install -U telos-cli`)"));
    }

    if lines.is_empty() {
        return None;
    }

    let mut notice = format!("telos {} is not the latest version.\n", status.current_version);
    notice.push_str(&lines.join("\n"));
    Some(notice)
}

fn classify_registry_version(current_version: &str, latest_version: &str) -> RegistryStatus {
    match (parse_version(current_version), parse_version(latest_version)) {
        (Ok(current), Ok(latest)) if latest > current => RegistryStatus::newer(latest_version),
        _ => RegistryStatus::current(latest_version),
    }
}

fn parse_version(version: &str) -> Result<Version, semver::Error> {
    Version::parse(version.trim_start_matches('v'))
}

async fn fetch_status(current_version: &str) -> Option<UpdateStatus> {
    let client =
        reqwest::Client::builder().timeout(HTTP_TIMEOUT).user_agent(USER_AGENT).build().ok()?;

    let (crates_io, pypi) =
        tokio::join!(fetch_crates_io_latest(&client), fetch_pypi_latest(&client));
    let crates_io = crates_io
        .map(|latest| classify_registry_version(current_version, &latest))
        .unwrap_or(RegistryStatus::Unavailable);
    let pypi = pypi
        .map(|latest| classify_registry_version(current_version, &latest))
        .unwrap_or(RegistryStatus::Unavailable);

    let status = UpdateStatus { current_version: current_version.to_string(), crates_io, pypi };
    if status.has_registry_result() {
        let _ = save_cache(&status);
    }
    Some(status)
}

async fn fetch_crates_io_latest(client: &reqwest::Client) -> Result<String> {
    let body = client.get(CRATES_IO_URL).send().await?.error_for_status()?.text().await?;
    parse_crates_io_latest(&body)
}

async fn fetch_pypi_latest(client: &reqwest::Client) -> Result<String> {
    let body = client.get(PYPI_URL).send().await?.error_for_status()?.text().await?;
    parse_pypi_latest(&body)
}

fn parse_crates_io_latest(body: &str) -> Result<String> {
    let response: CratesIoResponse = serde_json::from_str(body)?;
    Ok(response.crate_info.max_version)
}

fn parse_pypi_latest(body: &str) -> Result<String> {
    let response: PypiResponse = serde_json::from_str(body)?;
    Ok(response.info.version)
}

fn load_cached_status(current_version: &str) -> Option<UpdateStatus> {
    let cache = read_cache().ok()?;
    let checked_at = UNIX_EPOCH + Duration::from_secs(cache.checked_at_unix_secs);
    if !is_cache_fresh(checked_at, SystemTime::now()) {
        return None;
    }

    Some(cache.to_status(current_version))
}

fn read_cache() -> Result<UpdateCheckCache> {
    let path = cache_path().context("could not determine update check cache path")?;
    let contents = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&contents)?)
}

fn save_cache(status: &UpdateStatus) -> Result<()> {
    let path = cache_path().context("could not determine update check cache path")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let cache = UpdateCheckCache::from_status(status);
    let contents = serde_json::to_string_pretty(&cache)?;
    std::fs::write(path, contents)?;
    Ok(())
}

fn cache_path() -> Option<PathBuf> {
    dirs::cache_dir().map(|base| base.join("telos").join("update-check.json"))
}

impl UpdateCheckCache {
    fn from_status(status: &UpdateStatus) -> Self {
        Self {
            checked_at_unix_secs: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            crates_io: CachedRegistry::from_status(&status.crates_io),
            pypi: CachedRegistry::from_status(&status.pypi),
        }
    }

    fn to_status(&self, current_version: &str) -> UpdateStatus {
        UpdateStatus {
            current_version: current_version.to_string(),
            crates_io: self.crates_io.to_status(current_version),
            pypi: self.pypi.to_status(current_version),
        }
    }
}

impl CachedRegistry {
    fn from_status(status: &RegistryStatus) -> Self {
        match status {
            RegistryStatus::Current { latest_version }
            | RegistryStatus::Newer { latest_version } => {
                Self::Available { latest_version: latest_version.clone() }
            }
            RegistryStatus::Unavailable => Self::Unavailable,
        }
    }

    fn to_status(&self, current_version: &str) -> RegistryStatus {
        match self {
            Self::Available { latest_version } => {
                classify_registry_version(current_version, latest_version)
            }
            Self::Unavailable => RegistryStatus::Unavailable,
        }
    }
}

impl UpdateStatus {
    fn has_registry_result(&self) -> bool {
        !matches!(self.crates_io, RegistryStatus::Unavailable)
            || !matches!(self.pypi, RegistryStatus::Unavailable)
    }
}

fn is_cache_fresh(checked_at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(checked_at).is_ok_and(|age| age < CHECK_INTERVAL)
}

fn should_check_updates(quiet: bool, disable_env: Option<&str>) -> bool {
    if quiet {
        return false;
    }

    !disable_env
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn notice_lists_each_registry_with_newer_version() {
        let status = UpdateStatus {
            current_version: "0.1.0".to_string(),
            crates_io: RegistryStatus::newer("0.2.0"),
            pypi: RegistryStatus::newer("0.3.0"),
        };

        let notice = format_update_notice(&status).expect("notice");

        assert!(notice.contains("telos 0.1.0 is not the latest version"));
        assert!(notice.contains("crates.io: 0.2.0"));
        assert!(notice.contains("cargo install --force telos-cli"));
        assert!(notice.contains("PyPI: 0.3.0"));
        assert!(notice.contains("pip install -U telos-cli"));
    }

    #[test]
    fn notice_is_none_when_no_registry_has_newer_version() {
        let status = UpdateStatus {
            current_version: "0.2.0".to_string(),
            crates_io: RegistryStatus::current("0.2.0"),
            pypi: RegistryStatus::current("0.1.9"),
        };

        assert!(format_update_notice(&status).is_none());
    }

    #[test]
    fn registry_failures_do_not_hide_other_updates() {
        let status = UpdateStatus {
            current_version: "0.1.0".to_string(),
            crates_io: RegistryStatus::Unavailable,
            pypi: RegistryStatus::newer("0.2.0"),
        };

        let notice = format_update_notice(&status).expect("notice");

        assert!(!notice.contains("crates.io:"));
        assert!(notice.contains("PyPI: 0.2.0"));
    }

    #[test]
    fn status_without_any_registry_result_is_not_cacheable() {
        let status = UpdateStatus {
            current_version: "0.1.0".to_string(),
            crates_io: RegistryStatus::Unavailable,
            pypi: RegistryStatus::Unavailable,
        };

        assert!(!status.has_registry_result());
    }

    #[test]
    fn cache_is_fresh_for_less_than_twenty_four_hours() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(24 * 60 * 60);
        let checked_at = now - Duration::from_secs(23 * 60 * 60);

        assert!(is_cache_fresh(checked_at, now));
    }

    #[test]
    fn cache_is_stale_at_twenty_four_hours() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(24 * 60 * 60);
        let checked_at = now - Duration::from_secs(24 * 60 * 60);

        assert!(!is_cache_fresh(checked_at, now));
    }

    #[test]
    fn startup_check_is_disabled_by_quiet_flag() {
        assert!(!should_check_updates(true, None));
    }

    #[test]
    fn startup_check_is_disabled_by_environment() {
        assert!(!should_check_updates(false, Some("1")));
        assert!(!should_check_updates(false, Some("true")));
    }

    #[test]
    fn startup_check_is_enabled_by_default() {
        assert!(should_check_updates(false, None));
    }

    #[test]
    fn parses_crates_io_max_version() {
        let body = r#"{"crate":{"id":"telos-cli","max_version":"0.2.0"}}"#;

        assert_eq!(parse_crates_io_latest(body).unwrap(), "0.2.0");
    }

    #[test]
    fn parses_pypi_info_version() {
        let body = r#"{"info":{"version":"0.3.0"}}"#;

        assert_eq!(parse_pypi_latest(body).unwrap(), "0.3.0");
    }

    #[test]
    fn classifies_leading_v_as_semver() {
        assert_eq!(classify_registry_version("0.1.0", "v0.2.0"), RegistryStatus::newer("v0.2.0"));
    }
}
