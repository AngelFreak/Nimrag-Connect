//! Upload one FIT file (an activity, or any other file the watch produced) to Garmin Connect.
//!
//! `POST {connectapi}/upload-service/upload`, multipart field `file`, bearer
//! token + native headers (python-garminconnect 0.3.20 `api/activities.py:
//! 200-254`; garth-ng does the same). Answers per `docs/research/garmin-forward.md`
//! section 3.4-3.5: duplicate = HTTP 409 with `detailedImportResult.failures[0].
//! messages[0].code == 202`, for every file type. Verified live on 2026-11-22: monitoring
//! (128/32), 41 and 58 files get a 2xx with no activity id (Garmin Connect shows their data);
//! sub types 44, 49, 68 and 72 that Garmin already had get the duplicate answer. Any 2xx counts as
//! uploaded; the activity id is read from `detailedImportResult.successes[0].internalId` when
//! present (an activity file only).

use super::auth::{api_headers, Tokens};
use super::http::{self, GarminHttp, Request};
use super::GarminConfig;
use serde_json::Value;

#[derive(Debug, PartialEq, Eq)]
pub enum UploadOutcome {
    /// Accepted. The id is `None` while Garmin is still processing an activity, and for every other file.
    Uploaded { garmin_activity_id: Option<i64> },
    /// Garmin already has this file (409, code 202). Counts as done.
    Duplicate,
    /// Garmin refused the file for good (other 4xx); the reason is short and secret-free.
    Rejected(String),
    /// 401: the access token was not accepted.
    AuthExpired,
    /// 429, with `Retry-After` seconds when Garmin sent them.
    RateLimited { retry_after_s: Option<u64> },
    /// Network error, 5xx, 403: try again later.
    Transient(String),
}

pub async fn upload_file(
    cfg: &GarminConfig,
    api: &dyn GarminHttp,
    tokens: &Tokens,
    filename: &str,
    bytes: &[u8],
) -> UploadOutcome {
    let url = format!("{}/upload-service/upload", cfg.urls.connectapi);
    let req = Request::post(url, http::multipart_file("file", filename, bytes))
        .headers(api_headers(tokens));
    let resp = match api.send(req).await {
        Ok(r) => r,
        Err(e) => return UploadOutcome::Transient(e.to_string()),
    };
    let json = resp.json();
    match resp.status {
        200..=299 => UploadOutcome::Uploaded {
            garmin_activity_id: json.as_ref().and_then(success_id),
        },
        409 => {
            if json.as_ref().is_some_and(is_duplicate) {
                UploadOutcome::Duplicate
            } else {
                UploadOutcome::Rejected(format!(
                    "conflict (409): {}",
                    reason(json.as_ref()).unwrap_or_default()
                ))
            }
        }
        401 => UploadOutcome::AuthExpired,
        429 => UploadOutcome::RateLimited {
            retry_after_s: resp
                .header("retry-after")
                .and_then(|v| v.trim().parse().ok()),
        },
        403 => UploadOutcome::Transient("forbidden (403): token refused or request blocked".into()),
        s if s >= 500 => UploadOutcome::Transient(format!("Garmin server error ({s})")),
        s => UploadOutcome::Rejected(format!(
            "refused ({s}){}",
            reason(json.as_ref())
                .map(|r| format!(": {r}"))
                .unwrap_or_default()
        )),
    }
}

fn result(v: &Value) -> Option<&Value> {
    v.get("detailedImportResult")
}

fn success_id(v: &Value) -> Option<i64> {
    let id = result(v)?.pointer("/successes/0/internalId")?;
    id.as_i64().or_else(|| id.as_str()?.parse().ok())
}

fn is_duplicate(v: &Value) -> bool {
    let m = result(v).and_then(|r| r.pointer("/failures/0/messages/0"));
    m.is_some_and(|m| {
        m.get("code").and_then(Value::as_i64) == Some(202)
            || m.get("content")
                .and_then(Value::as_str)
                .is_some_and(|c| c.to_ascii_lowercase().contains("duplicate"))
    })
}

/// A short human reason from Garmin's error body (`message`, `content` or the
/// first import failure), capped; never the whole body.
fn reason(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let s = v
        .get("message")
        .or_else(|| v.get("content"))
        .or_else(|| result(v).and_then(|r| r.pointer("/failures/0/messages/0/content")))?
        .as_str()?;
    Some(s.chars().take(120).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn duplicate_detection() {
        let dup = json!({"detailedImportResult": {"failures": [{"messages": [{"code": 202, "content": "Duplicate Activity."}]}]}});
        assert!(is_duplicate(&dup));
        let other = json!({"detailedImportResult": {"failures": [{"messages": [{"code": 100, "content": "Bad"}]}]}});
        assert!(!is_duplicate(&other));
        assert_eq!(reason(Some(&other)).as_deref(), Some("Bad"));
    }

    #[test]
    fn success_id_forms() {
        let n = json!({"detailedImportResult": {"successes": [{"internalId": 123}]}});
        let s = json!({"detailedImportResult": {"successes": [{"internalId": "456"}]}});
        assert_eq!(success_id(&n), Some(123));
        assert_eq!(success_id(&s), Some(456));
        assert_eq!(success_id(&json!({})), None);
    }
}
