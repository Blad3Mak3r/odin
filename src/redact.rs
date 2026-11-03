//! Masks credentials in free-form error text before it leaves the process.
//!
//! Failure messages are built from whatever an error chain says, which can
//! include a URL with credentials, a signed storage link or an RCON password
//! echoed back by a tool. They are stored in the activity feed and posted to
//! webhooks (Discord and the like), so they are cleaned first. This is a
//! safety net for the common shapes, not a guarantee: it cannot recognise a
//! secret that is not labelled as one.

use std::sync::LazyLock;

use regex::Regex;

static URL_CREDENTIALS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b([a-z][a-z0-9+.-]*://)[^/\s:@]+:[^/\s@]+@").unwrap());

static LABELLED_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)\b(password|passwd|pwd|token|secret|api[_-]?key|access[_-]?key|signature|x-amz-signature|x-amz-credential|x-amz-security-token|authorization)(["']?\s*[:=]\s*["']?)(?:Bearer\s+)?[^\s&"',;]+"#,
    )
    .unwrap()
});

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bBearer\s+[a-z0-9._~+/=-]+").unwrap());

pub const MASK: &str = "***";

pub fn secrets(text: &str) -> String {
    let text = URL_CREDENTIALS.replace_all(text, format!("${{1}}{MASK}@"));
    let text = BEARER.replace_all(&text, format!("Bearer {MASK}"));
    LABELLED_SECRET
        .replace_all(&text, format!("${{1}}${{2}}{MASK}"))
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_in_a_url_are_masked() {
        assert_eq!(
            secrets("GET https://admin:hunter2@example.com/hook failed"),
            "GET https://***@example.com/hook failed"
        );
    }

    #[test]
    fn labelled_values_are_masked_in_urls_and_prose() {
        assert_eq!(
            secrets(
                "PUT https://b.s3.amazonaws.com/o?X-Amz-Signature=abc123&X-Amz-Expires=60 returned 403"
            ),
            "PUT https://b.s3.amazonaws.com/o?X-Amz-Signature=***&X-Amz-Expires=60 returned 403"
        );
        assert_eq!(
            secrets("rcon password: hunter2 rejected"),
            "rcon password: *** rejected"
        );
        assert_eq!(secrets(r#"{"token":"abc.def"}"#), r#"{"token":"***"}"#);
        assert_eq!(secrets("api_key=xyz"), "api_key=***");
    }

    #[test]
    fn bearer_tokens_are_masked() {
        assert_eq!(
            secrets("Authorization: Bearer eyJhbGciOi.payload.sig"),
            "Authorization: ***"
        );
        assert_eq!(
            secrets("sent Bearer abc123 to server"),
            "sent Bearer *** to server"
        );
    }

    #[test]
    fn ordinary_messages_are_left_alone() {
        for text in [
            "key not found in save",
            "failed to create /var/lib/odin/backups: No space left on device",
            "container exited with status 137",
        ] {
            assert_eq!(secrets(text), text);
        }
    }
}
