//! LanguageTool grammar and spelling checks for note prose.
//!
//! The frontend sends LanguageTool's `data` annotation (text plus markup segments for code,
//! links and other non-prose), so match offsets map back to editor positions. The server URL
//! and language always come from the saved settings, never from the request.
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::AppState;

pub const DEFAULT_URL: &str = "https://api.languagetool.org";
/// Languages offered in Settings. `auto` lets the server detect the language.
pub const LANGUAGES: [&str; 5] = ["auto", "es", "ca-ES", "en-US", "en-GB"];
/// Variants used when the language is detected automatically.
const PREFERRED_VARIANTS: &str = "en-US,ca-ES";
/// The public API accepts up to 20 KB per request; self-hosted servers accept more.
const MAX_REQUEST_BYTES: usize = 100 * 1024;
const MAX_MATCHES: usize = 500;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LtMatch {
    pub offset: usize,
    pub length: usize,
    pub message: String,
    pub short_message: String,
    pub replacements: Vec<String>,
    pub rule_id: String,
    /// LanguageTool issue type, e.g. `misspelling`, `grammar`, `style`, `typographical`.
    pub issue_type: String,
}

#[derive(Deserialize)]
struct CheckResponse {
    #[serde(default)]
    matches: Vec<RawMatch>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawMatch {
    offset: usize,
    length: usize,
    #[serde(default)]
    message: String,
    #[serde(default)]
    short_message: String,
    #[serde(default)]
    replacements: Vec<RawReplacement>,
    rule: Option<RawRule>,
}

#[derive(Deserialize)]
struct RawReplacement {
    value: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    issue_type: String,
}

/// Normalize a per-note language code from frontmatter (`ca`, `en_GB`, `es-ES`, ...).
/// Short codes get the variant LanguageTool needs for spell checking. Returns `None`
/// for anything that does not look like a language code.
pub fn normalize_language(code: &str) -> Option<String> {
    let code = code.trim().replace('_', "-");
    let mut parts = code.split('-');
    let primary = parts.next()?.to_ascii_lowercase();
    if primary == "auto" && parts.clone().next().is_none() {
        return Some("auto".into());
    }
    if !(2..=3).contains(&primary.len()) || !primary.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    if rest.len() > 2
        || rest
            .iter()
            .any(|p| !(2..=8).contains(&p.len()) || !p.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return None;
    }
    if rest.is_empty() {
        let variant = match primary.as_str() {
            "ca" => "ca-ES",
            "en" => "en-US",
            "de" => "de-DE",
            "pt" => "pt-PT",
            _ => return Some(primary),
        };
        return Some(variant.into());
    }
    // LanguageTool codes: region upper-case (en-GB), longer subtags lower-case (ca-ES-valencia).
    let rest = rest.iter().map(|p| {
        if p.len() == 2 {
            p.to_ascii_uppercase()
        } else {
            p.to_ascii_lowercase()
        }
    });
    Some(
        std::iter::once(primary)
            .chain(rest)
            .collect::<Vec<_>>()
            .join("-"),
    )
}

fn check_url(base: &str) -> String {
    let base = base.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v2/check").unwrap_or(base);
    let base = base.strip_suffix("/v2").unwrap_or(base);
    format!("{}/v2/check", base.trim_end_matches('/'))
}

fn validate_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "Invalid LanguageTool server URL")?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("The LanguageTool server URL must start with http:// or https://".into());
    }
    Ok(())
}

fn parse_matches(body: &str) -> Result<Vec<LtMatch>, String> {
    let response: CheckResponse =
        serde_json::from_str(body).map_err(|_| "LanguageTool returned an invalid response")?;
    Ok(response
        .matches
        .into_iter()
        .take(MAX_MATCHES)
        .filter(|m| m.length > 0)
        .map(|m| {
            let rule = m.rule.unwrap_or(RawRule {
                id: String::new(),
                issue_type: String::new(),
            });
            LtMatch {
                offset: m.offset,
                length: m.length,
                message: m.message,
                short_message: m.short_message,
                replacements: m
                    .replacements
                    .into_iter()
                    .filter_map(|r| r.value)
                    .take(5)
                    .collect(),
                rule_id: rule.id,
                issue_type: rule.issue_type,
            }
        })
        .collect())
}

fn http_error(status: reqwest::StatusCode) -> String {
    match status.as_u16() {
        413 => "The note is too long for this LanguageTool server.".into(),
        429 => "LanguageTool rate limit reached. Wait a minute or use your own server.".into(),
        code => format!("LanguageTool returned HTTP {code}."),
    }
}

#[tauri::command]
pub fn set_languagetool_settings(
    state: State<'_, AppState>,
    enabled: bool,
    url: Option<String>,
    language: String,
) -> Result<(), String> {
    let url = url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    if let Some(url) = &url {
        if url.len() > 2048 {
            return Err("The LanguageTool server URL is too long".into());
        }
        validate_url(url)?;
    }
    if !LANGUAGES.contains(&language.as_str()) {
        return Err("Unsupported LanguageTool language".into());
    }
    let mut current = state.config.lock().map_err(|e| e.to_string())?;
    let mut config = current.clone();
    config.languagetool_enabled = enabled;
    config.languagetool_url = url;
    config.languagetool_language = language;
    crate::commands::save_app_config_to(&config, &crate::commands::app_config_path()?)?;
    *current = config;
    Ok(())
}

/// Set or clear a note's LanguageTool language (`lang` frontmatter key).
#[tauri::command]
pub async fn set_note_language(
    state: State<'_, AppState>,
    vault_path: String,
    path: String,
    language: Option<String>,
    expected_raw: String,
) -> Result<(), String> {
    state.validate_vault(&vault_path)?;
    let language = match language.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(code) => Some(normalize_language(code).ok_or("Invalid note language")?),
    };
    let guard = state.note_save.clone().lock_owned().await;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        crate::vault::operations::set_note_language(
            &vault_path,
            &path,
            language.as_deref(),
            &expected_raw,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Check one batch of prose. `data` is LanguageTool's annotation JSON; `language` is an
/// optional per-note override (from frontmatter) of the language saved in Settings.
#[tauri::command]
pub async fn languagetool_check(
    state: State<'_, AppState>,
    data: String,
    language: Option<String>,
) -> Result<Vec<LtMatch>, String> {
    let note_language = match language.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(code) => Some(normalize_language(code).ok_or("Invalid note language")?),
    };
    if data.len() > MAX_REQUEST_BYTES {
        return Err("Text is too long to check in one request".into());
    }
    serde_json::from_str::<serde_json::Value>(&data)
        .ok()
        .filter(|v| v["annotation"].is_array())
        .ok_or("Invalid LanguageTool request")?;
    let (url, language) = {
        let config = state.config.lock().map_err(|e| e.to_string())?;
        if !config.languagetool_enabled {
            return Err("LanguageTool is disabled".into());
        }
        let base = config.languagetool_url.as_deref().unwrap_or(DEFAULT_URL);
        let language = note_language.unwrap_or_else(|| config.languagetool_language.clone());
        (check_url(base), language)
    };
    validate_url(&url)?;

    let mut form = vec![("data", data), ("language", language.clone())];
    if language == "auto" {
        form.push(("preferredVariants", PREFERRED_VARIANTS.into()));
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "Could not initialize the LanguageTool connection")?;
    let response = client
        .post(&url)
        .form(&form)
        .send()
        .await
        // reqwest errors can contain the configured URL; keep messages generic.
        .map_err(|_| "Could not reach the LanguageTool server")?;
    if !response.status().is_success() {
        return Err(http_error(response.status()));
    }
    let body = response
        .text()
        .await
        .map_err(|_| "Could not read the LanguageTool response")?;
    parse_matches(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_check_url_from_any_base_form() {
        for base in [
            "https://api.languagetool.org",
            "https://api.languagetool.org/",
            "https://api.languagetool.org/v2",
            "https://api.languagetool.org/v2/check",
        ] {
            assert_eq!(check_url(base), "https://api.languagetool.org/v2/check");
        }
        assert_eq!(
            check_url("http://localhost:8081/"),
            "http://localhost:8081/v2/check"
        );
    }

    #[test]
    fn normalizes_frontmatter_language_codes() {
        let cases = [
            ("ca", "ca-ES"),
            ("en", "en-US"),
            ("es", "es"),
            ("ES", "es"),
            ("en_gb", "en-GB"),
            ("es-ES", "es-ES"),
            ("ca-es-VALENCIA", "ca-ES-valencia"),
            ("fr", "fr"),
            ("auto", "auto"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                normalize_language(input).as_deref(),
                Some(expected),
                "{input}"
            );
        }
        for bad in ["", "c", "català", "en-", "en/US", "a-b-c-d", "e1"] {
            assert_eq!(normalize_language(bad), None, "{bad}");
        }
    }

    #[test]
    fn rejects_non_http_urls() {
        assert!(validate_url("http://localhost:8081/v2/check").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("localhost:8081").is_err());
    }

    #[test]
    fn parses_matches_and_limits_replacements() {
        let body = r#"{"matches":[
            {"offset":8,"length":5,"message":"Accent","shortMessage":"",
             "replacements":[{"value":"sabía"},{"value":"a"},{"value":"b"},{"value":"c"},{"value":"d"},{"value":"e"}],
             "rule":{"id":"SABIA","issueType":"misspelling"}},
            {"offset":1,"length":0,"message":"empty","replacements":[]}
        ]}"#;
        let matches = parse_matches(body).unwrap();
        assert_eq!(matches.len(), 1, "zero-length matches are dropped");
        assert_eq!(matches[0].replacements.len(), 5);
        assert_eq!(matches[0].replacements[0], "sabía");
        assert_eq!(matches[0].issue_type, "misspelling");
        assert!(parse_matches("not json").is_err());
    }
}
