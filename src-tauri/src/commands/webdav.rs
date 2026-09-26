//! WebDAV settings sync.
//!
//! The frontend assembles a backup payload (settings + dictionary + correction
//! rules, see `src/lib/backup-settings.ts`) and hands the JSON to these
//! commands. The password lives in the system credential vault under
//! `sync.webdav` and never round-trips through the renderer.

use crate::credentials::{CredentialSecretReader, SystemCredentialVault};
use serde::Serialize;

const WEBDAV_NAMESPACE: &str = "sync";
const WEBDAV_PROVIDER: &str = "webdav";
const WEBDAV_TIMEOUT_SECS: u64 = 30;
/// Guard against uploading an accidental giant payload (e.g. full history).
const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebDavTestResult {
    pub ok: bool,
    pub remote_exists: bool,
    pub message: String,
}

fn load_password() -> Result<String, String> {
    SystemCredentialVault
        .get_secret(WEBDAV_NAMESPACE, WEBDAV_PROVIDER)
        .map_err(|error| format!("Failed to read WebDAV password from the system vault: {error}"))?
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "WebDAV password is not set. Save it in Settings -> Sync.".to_string())
}

fn validate_target(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("WebDAV URL is empty".to_string());
    }
    let parsed = url::Url::parse(url).map_err(|error| format!("Invalid WebDAV URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("WebDAV URL must use http or https".to_string());
    }
    Ok(url.to_string())
}

fn build_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    username: &str,
    password: &str,
) -> reqwest::RequestBuilder {
    let mut request = client
        .request(method, url)
        .timeout(std::time::Duration::from_secs(WEBDAV_TIMEOUT_SECS));
    if !username.trim().is_empty() {
        request = request.basic_auth(username.trim(), Some(password));
    }
    request
}

/// Probe the remote target. `GET 404` means "connected, no backup yet" —
/// that is still a successful configuration check.
#[tauri::command]
pub async fn webdav_test_connection(
    url: String,
    username: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<WebDavTestResult, String> {
    let url = validate_target(&url)?;
    let password = load_password()?;

    let resp = build_request(&client, reqwest::Method::GET, &url, &username, &password)
        .send()
        .await
        .map_err(|error| format!("WebDAV request failed: {error}"))?;

    let status = resp.status();
    if status.is_success() {
        Ok(WebDavTestResult {
            ok: true,
            remote_exists: true,
            message: "Connected. A remote backup exists.".to_string(),
        })
    } else if status.as_u16() == 404 {
        Ok(WebDavTestResult {
            ok: true,
            remote_exists: false,
            message: "Connected. No remote backup yet.".to_string(),
        })
    } else if matches!(status.as_u16(), 401 | 403) {
        Err("Authentication failed. Check the WebDAV username and password.".to_string())
    } else {
        Err(format!("WebDAV server returned HTTP {}", status.as_u16()))
    }
}

/// Upload a backup payload (JSON string) to the remote target with `PUT`.
#[tauri::command]
pub async fn webdav_upload_backup(
    url: String,
    username: String,
    payload: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<(), String> {
    let url = validate_target(&url)?;
    let password = load_password()?;
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "Backup payload is too large ({} bytes, limit {MAX_PAYLOAD_BYTES})",
            payload.len()
        ));
    }

    let resp = build_request(&client, reqwest::Method::PUT, &url, &username, &password)
        .header("Content-Type", "application/json")
        .body(payload)
        .send()
        .await
        .map_err(|error| format!("WebDAV upload failed: {error}"))?;

    let status = resp.status();
    if status.is_success() {
        Ok(())
    } else if matches!(status.as_u16(), 401 | 403) {
        Err("Authentication failed. Check the WebDAV username and password.".to_string())
    } else {
        Err(format!("WebDAV upload failed: HTTP {}", status.as_u16()))
    }
}

/// Download the remote backup payload. Returns the raw JSON string.
#[tauri::command]
pub async fn webdav_download_backup(
    url: String,
    username: String,
    client: tauri::State<'_, reqwest::Client>,
) -> Result<String, String> {
    let url = validate_target(&url)?;
    let password = load_password()?;

    let resp = build_request(&client, reqwest::Method::GET, &url, &username, &password)
        .send()
        .await
        .map_err(|error| format!("WebDAV download failed: {error}"))?;

    let status = resp.status();
    if status.as_u16() == 404 {
        return Err("No remote backup found. Upload settings first.".to_string());
    }
    if matches!(status.as_u16(), 401 | 403) {
        return Err("Authentication failed. Check the WebDAV username and password.".to_string());
    }
    if !status.is_success() {
        return Err(format!("WebDAV download failed: HTTP {}", status.as_u16()));
    }

    let body = resp
        .text()
        .await
        .map_err(|error| format!("Failed to read WebDAV response: {error}"))?;
    if body.len() > MAX_PAYLOAD_BYTES {
        return Err(format!(
            "Remote backup is too large ({} bytes, limit {MAX_PAYLOAD_BYTES})",
            body.len()
        ));
    }
    serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|_| "Remote backup is not valid JSON".to_string())?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_and_non_http_targets() {
        assert!(validate_target("").is_err());
        assert!(validate_target("ftp://example.com/a.json").is_err());
        assert!(validate_target("not a url").is_err());
        assert!(validate_target("https://dav.example.com/a.json").is_ok());
        assert!(validate_target("http://localhost:8080/a.json").is_ok());
    }

    #[test]
    fn password_is_required_before_any_request() {
        // Without a usable vault entry the commands must fail fast instead of
        // sending unauthenticated requests. The exact error depends on the
        // host (missing entry vs. unavailable keyring), so only assert failure.
        assert!(load_password().is_err());
    }
}
