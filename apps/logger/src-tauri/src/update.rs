//! update.rs — app info + release update check (CONTRACTS.md §7.4).
//!
//! Owner: polisher. `get_app_info` reports version/os/arch; `check_for_update`
//! GETs the GitHub Releases `latest.json` and compares versions. Notice-only in
//! v0.1.0 — no auto-update until signing. EVERY network/parse failure maps to
//! `Ok(None)`: an update check must never surface as a user-facing error.

use std::cmp::Ordering;
use std::time::Duration;

use crate::error::LoggerError;
use crate::model::{AppInfoDto, UpdateInfoDto};

/// GitHub Releases `latest.json` endpoint (Tauri updater feed shape:
/// `{ version, notes?, pub_date?, platforms{ … } }`).
///
/// PLACEHOLDER — the real `<org>/<repo>` is set when the public repo is
/// extracted (repo-extraction task); keep the
/// `…/releases/latest/download/latest.json` shape.
pub const LATEST_JSON_URL: &str =
    "https://github.com/DropTime-Software/droptime-logger/releases/latest/download/latest.json";

/// The human-facing releases page we send people to for a notice-only update.
pub const RELEASES_PAGE_URL: &str = "https://github.com/DropTime-Software/droptime-logger/releases";

/// How long to wait for the feed before giving up (connect + read).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

/// Version from the Tauri config, os/arch from the build target.
pub fn app_info(app: &tauri::AppHandle) -> AppInfoDto {
    AppInfoDto {
        version: app.package_info().version.to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    }
}

/// Check `LATEST_JSON_URL` for a newer release.
///
/// GETs the feed with a 3s timeout via `ureq`; maps EVERY network/parse failure
/// to `Ok(None)`. On success returns the DTO with `isNewer` set by a numeric
/// `x.y.z` comparison against `current_version`.
pub fn check_for_update(current_version: &str) -> Result<Option<UpdateInfoDto>, LoggerError> {
    let body = match fetch_latest_json(LATEST_JSON_URL) {
        Some(b) => b,
        None => return Ok(None),
    };
    Ok(evaluate_update(current_version, &body))
}

/// Network side of the check — isolated so `evaluate_update` stays pure/testable.
fn fetch_latest_json(url: &str) -> Option<String> {
    let resp = ureq::get(url).timeout(REQUEST_TIMEOUT).call().ok()?;
    resp.into_string().ok()
}

/// Pure: parse the `latest.json` body and compare versions. Any missing field
/// or malformed JSON → `None` (caller turns that into `Ok(None)`).
fn evaluate_update(current_version: &str, body: &str) -> Option<UpdateInfoDto> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let latest_raw = json.get("version")?.as_str()?.trim();
    let latest = latest_raw.trim_start_matches('v');
    let current = current_version.trim().trim_start_matches('v');
    if latest.is_empty() {
        return None;
    }
    Some(UpdateInfoDto {
        current_version: current.to_string(),
        latest_version: latest.to_string(),
        url: RELEASES_PAGE_URL.to_string(),
        is_newer: is_version_newer(current, latest),
    })
}

/// True iff `latest` is strictly a newer version than `current`.
fn is_version_newer(current: &str, latest: &str) -> bool {
    cmp_semver(latest, current) == Ordering::Greater
}

/// Parse `x.y.z` into a numeric triple. Pre-release/build metadata after `-`/`+`
/// is dropped; missing or non-numeric components count as 0.
fn parse_semver(v: &str) -> (u64, u64, u64) {
    let core = v.split(['-', '+']).next().unwrap_or(v);
    let mut it = core
        .split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0));
    (
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
        it.next().unwrap_or(0),
    )
}

fn cmp_semver(a: &str, b: &str) -> Ordering {
    parse_semver(a).cmp(&parse_semver(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_dotted_versions_with_prerelease_and_build() {
        assert_eq!(parse_semver("0.1.0"), (0, 1, 0));
        // parse_semver does NOT strip a leading `v` — that is evaluate_update's job.
        assert_eq!(parse_semver("v1.2.3"), (0, 2, 3));
        assert_eq!(parse_semver("1.2.3-beta.4"), (1, 2, 3));
        assert_eq!(parse_semver("2.0.0+build.9"), (2, 0, 0));
        assert_eq!(parse_semver("1.2"), (1, 2, 0));
        assert_eq!(parse_semver("7"), (7, 0, 0));
        assert_eq!(parse_semver("garbage"), (0, 0, 0));
    }

    #[test]
    fn newer_comparison_is_strict_and_numeric() {
        assert!(is_version_newer("0.1.0", "0.2.0"));
        assert!(is_version_newer("0.1.0", "0.1.1"));
        assert!(is_version_newer("0.9.9", "1.0.0"));
        // 10 > 9 numerically (a lexical compare would get this wrong)
        assert!(is_version_newer("0.9.0", "0.10.0"));
        assert!(!is_version_newer("0.1.0", "0.1.0"));
        assert!(!is_version_newer("0.2.0", "0.1.0"));
        // a pre-release of the same core is NOT newer than the release
        assert!(!is_version_newer("0.1.0", "0.1.0-beta.1"));
    }

    #[test]
    fn evaluate_update_reports_newer_release() {
        let body = r#"{ "version": "0.4.0", "notes": "shiny", "pub_date": "2026-07-10T00:00:00Z",
                        "platforms": { "darwin-aarch64": { "url": "x", "signature": "y" } } }"#;
        let dto = evaluate_update("0.1.0", body).expect("well-formed feed parses");
        assert_eq!(dto.current_version, "0.1.0");
        assert_eq!(dto.latest_version, "0.4.0");
        assert!(dto.is_newer);
        assert_eq!(dto.url, RELEASES_PAGE_URL);
    }

    #[test]
    fn evaluate_update_strips_leading_v_and_handles_up_to_date() {
        let dto = evaluate_update("v0.5.0", r#"{ "version": "v0.5.0" }"#).unwrap();
        assert_eq!(dto.current_version, "0.5.0");
        assert_eq!(dto.latest_version, "0.5.0");
        assert!(!dto.is_newer, "same version must not be flagged newer");
    }

    #[test]
    fn evaluate_update_rejects_malformed_or_incomplete_feeds() {
        assert!(evaluate_update("0.1.0", "not json").is_none());
        assert!(
            evaluate_update("0.1.0", "{}").is_none(),
            "missing version → None"
        );
        assert!(
            evaluate_update("0.1.0", r#"{ "version": 4 }"#).is_none(),
            "non-string version → None"
        );
        assert!(
            evaluate_update("0.1.0", r#"{ "version": "" }"#).is_none(),
            "empty version → None"
        );
    }
}
