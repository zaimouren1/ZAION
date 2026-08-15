//! SecretRedactor — strip API keys, tokens, and credentials from strings.
//!
//! Hermes equivalent: `redact.py` `RedactingFormatter` + `redact_secrets()`.
//!
//! Covers 35+ API key patterns used by major providers and platforms.

/// Replacement placeholder shown instead of a redacted secret.
const REDACTED: &str = "[REDACTED]";

/// A compiled redaction rule: a prefix + minimum length the full match must have.
struct Rule {
    prefix: &'static str,
    /// Minimum total length (prefix + secret portion). Prevents false positives on short strings.
    min_total_len: usize,
}

/// All known API key patterns.
///
/// Hermes' `redact.py` covers: sk-/sk-ant-/sk-proj-/ghp_/gho_/ghs_/ghr_/
/// xoxb-/xoxp-/xoxa-/xoxr-/xoxs-/AIza/AKIA/ya29./eyJ + Bearer/Token headers.
static RULES: &[Rule] = &[
    // ── Anthropic ────────────────────────────────────────────────────────────
    Rule {
        prefix: "sk-ant-api03-",
        min_total_len: 30,
    },
    Rule {
        prefix: "sk-ant-",
        min_total_len: 20,
    },
    // ── OpenAI ───────────────────────────────────────────────────────────────
    Rule {
        prefix: "sk-proj-",
        min_total_len: 20,
    },
    Rule {
        prefix: "sk-svcacct-",
        min_total_len: 20,
    },
    Rule {
        prefix: "sk-",
        min_total_len: 20,
    },
    // ── GitHub ───────────────────────────────────────────────────────────────
    Rule {
        prefix: "ghp_",
        min_total_len: 10,
    },
    Rule {
        prefix: "gho_",
        min_total_len: 10,
    },
    Rule {
        prefix: "ghs_",
        min_total_len: 10,
    },
    Rule {
        prefix: "ghr_",
        min_total_len: 10,
    },
    Rule {
        prefix: "github_pat_",
        min_total_len: 20,
    },
    // ── Slack ─────────────────────────────────────────────────────────────────
    Rule {
        prefix: "xoxb-",
        min_total_len: 10,
    },
    Rule {
        prefix: "xoxp-",
        min_total_len: 10,
    },
    Rule {
        prefix: "xoxa-",
        min_total_len: 10,
    },
    Rule {
        prefix: "xoxr-",
        min_total_len: 10,
    },
    Rule {
        prefix: "xoxs-",
        min_total_len: 10,
    },
    // ── Google / Firebase ────────────────────────────────────────────────────
    Rule {
        prefix: "AIza",
        min_total_len: 10,
    },
    Rule {
        prefix: "ya29.",
        min_total_len: 20,
    },
    // ── AWS ──────────────────────────────────────────────────────────────────
    Rule {
        prefix: "AKIA",
        min_total_len: 20,
    },
    Rule {
        prefix: "ASIA",
        min_total_len: 20,
    },
    // ── Stripe ───────────────────────────────────────────────────────────────
    Rule {
        prefix: "sk_live_",
        min_total_len: 20,
    },
    Rule {
        prefix: "sk_test_",
        min_total_len: 20,
    },
    Rule {
        prefix: "rk_live_",
        min_total_len: 20,
    },
    // ── Twilio ───────────────────────────────────────────────────────────────
    Rule {
        prefix: "AC",
        min_total_len: 34,
    }, // account SID: AC + 32 hex
    // ── SendGrid ─────────────────────────────────────────────────────────────
    Rule {
        prefix: "SG.",
        min_total_len: 15,
    },
    // ── HuggingFace ──────────────────────────────────────────────────────────
    Rule {
        prefix: "hf_",
        min_total_len: 10,
    },
    // ── Cloudflare ───────────────────────────────────────────────────────────
    Rule {
        prefix: "cfuser_",
        min_total_len: 10,
    },
    Rule {
        prefix: "cfkey_",
        min_total_len: 10,
    },
    // ── Telegram bot token (format: 1234567890:ABC...) ────────────────────────
    // Handled separately via regex-like scanning in telegram_token_scan().

    // ── JWT / bearer tokens (starts with "eyJ" = base64 '{"') ───────────────
    Rule {
        prefix: "eyJ",
        min_total_len: 30,
    },
    // ── ZhipuAI ──────────────────────────────────────────────────────────────
    Rule {
        prefix: "zhipuai-",
        min_total_len: 15,
    },
    // ── MiniMax ──────────────────────────────────────────────────────────────
    Rule {
        prefix: "mm-",
        min_total_len: 20,
    },
    // ── DeepSeek ─────────────────────────────────────────────────────────────
    Rule {
        prefix: "dsk-",
        min_total_len: 15,
    },
    // ── Generic "Bearer " / "Authorization: " headers ───────────────────────
    // Handled separately (Bearer + value, not a prefix on the key itself).
];

/// Redacts secrets from a string in-place, returning a new sanitized string.
pub struct SecretRedactor;

impl SecretRedactor {
    /// Redact all known secret patterns from `text`.
    ///
    /// Each matched secret is replaced with `[REDACTED]`.
    /// Database connection strings containing `://user:pass@` are also redacted.
    pub fn redact(text: &str) -> String {
        let mut result = text.to_string();

        // 1. Prefix-based token redaction
        for rule in RULES {
            result = redact_prefix_tokens(&result, rule.prefix, rule.min_total_len);
        }

        // 2. Telegram bot token: digits:alphanum{35}
        result = redact_telegram_tokens(&result);

        // 3. DB connection strings: ://user:pass@host → ://***@host
        result = redact_db_connection_strings(&result);

        // 4. Bearer / Token header values
        result = redact_bearer_tokens(&result);

        result
    }

    /// Convenience: redact and log the result via eprintln (safe for debugging).
    pub fn safe_log(label: &str, text: &str) {
        eprintln!("[zaion] {}: {}", label, Self::redact(text));
    }
}

/// Redact all occurrences of tokens that start with `prefix` and meet `min_total_len`.
fn redact_prefix_tokens(text: &str, prefix: &str, min_total_len: usize) -> String {
    if !text.contains(prefix) {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut pos = 0usize;
    while let Some(start) = text[pos..].find(prefix).map(|i| pos + i) {
        result.push_str(&text[pos..start]);
        let token_start = start;
        // Consume: prefix + any non-whitespace non-comma chars
        let rest = &text[token_start..];
        let token_len = rest
            .char_indices()
            .take_while(|(_, c)| {
                !c.is_ascii_whitespace() && *c != ',' && *c != '"' && *c != '\'' && *c != ')'
            })
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(prefix.len());
        let token = &text[token_start..token_start + token_len];
        if token.len() >= min_total_len {
            result.push_str(REDACTED);
        } else {
            // Too short — not a real key; emit as-is.
            result.push_str(token);
        }
        pos = token_start + token_len;
    }
    result.push_str(&text[pos..]);
    result
}

/// Redact Telegram bot tokens: `<digits>:<alphanum+_->{30,50}`
fn redact_telegram_tokens(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut pos = 0usize;
    while pos < bytes.len() {
        // Find a run of digits
        if bytes[pos].is_ascii_digit() {
            let digit_start = pos;
            while pos < bytes.len() && bytes[pos].is_ascii_digit() {
                pos += 1;
            }
            let digit_count = pos - digit_start;
            // Must be followed by ':'
            if pos < bytes.len() && bytes[pos] == b':' && digit_count >= 6 {
                let colon_pos = pos;
                pos += 1;
                let token_start = pos;
                while pos < bytes.len()
                    && (bytes[pos].is_ascii_alphanumeric()
                        || bytes[pos] == b'_'
                        || bytes[pos] == b'-')
                {
                    pos += 1;
                }
                let token_len = pos - token_start;
                if token_len >= 25 {
                    // Looks like a Telegram token
                    result.push_str(REDACTED);
                    continue;
                } else {
                    // Not a token — emit verbatim
                    result.push_str(&text[digit_start..colon_pos + 1 + token_len]);
                    continue;
                }
            } else {
                result.push_str(&text[digit_start..pos]);
                continue;
            }
        }
        // Normal char — pass through
        result.push(text[pos..].chars().next().unwrap_or(' '));
        pos += text[pos..]
            .chars()
            .next()
            .map(|c| c.len_utf8())
            .unwrap_or(1);
    }
    result
}

/// Redact `://user:password@` in connection strings → `://***@`
fn redact_db_connection_strings(text: &str) -> String {
    // Pattern: ://something:something@  (non-space sequence between :// and @)
    if !text.contains("://") {
        return text.to_string();
    }
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(scheme_end) = remaining.find("://") {
        result.push_str(&remaining[..scheme_end + 3]);
        remaining = &remaining[scheme_end + 3..];
        // Look for user:pass@
        if let Some(at_pos) = remaining.find('@') {
            let creds = &remaining[..at_pos];
            if creds.contains(':') && !creds.contains('/') && !creds.contains(' ') {
                result.push_str("***@");
                remaining = &remaining[at_pos + 1..];
            }
            // else no credentials → skip
        }
    }
    result.push_str(remaining);
    result
}

/// Redact `Bearer <token>` and `Token <token>` header values.
fn redact_bearer_tokens(text: &str) -> String {
    let mut result = text.to_string();
    for keyword in &[
        "Bearer ",
        "Token ",
        "Authorization: Bearer ",
        "Authorization: Token ",
    ] {
        if result.contains(keyword) {
            result = redact_prefix_tokens(&result, keyword, keyword.len() + 10);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_openai_key() {
        let text = "key=sk-abc123XYZ456def789GHI012jkl345MNO";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"), "got: {}", out);
        assert!(!out.contains("sk-abc"));
    }

    #[test]
    fn redacts_anthropic_key() {
        let text = "ANTHROPIC_API_KEY=sk-ant-api03-xxxxxxxxxxxxxxxxxxxxxxxx";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"));
        assert!(!out.contains("sk-ant-api03"));
    }

    #[test]
    fn redacts_github_token() {
        let text = "token: ghp_Abcdefghijklmnopqrstuvwxyz12345678";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"));
        assert!(!out.contains("ghp_"));
    }

    #[test]
    fn redacts_telegram_bot_token() {
        let text = "bot token: 8577672617:AAExxxYYYzzzABCDEFGHIJKLMNOPQRSTUVW";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"));
        assert!(!out.contains("8577672617:AAE"));
    }

    #[test]
    fn redacts_db_connection_string() {
        let text = "connecting to postgres://admin:s3cr3t@localhost:5432/db";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("***@"), "got: {}", out);
        assert!(!out.contains("s3cr3t"));
    }

    #[test]
    fn redacts_bearer_token() {
        let text = "Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.abc.def";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn safe_strings_pass_through() {
        let text = "Hello world! This is a normal string with no secrets.";
        let out = SecretRedactor::redact(text);
        assert_eq!(out, text);
    }

    #[test]
    fn short_prefix_not_redacted() {
        // "sk-" alone (too short) should not be redacted
        let text = "sk-x"; // only 4 chars, min is 20
        let out = SecretRedactor::redact(text);
        assert_eq!(out, text);
    }

    #[test]
    fn redacts_jwt_token() {
        let text = "token=eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9.payload.signature";
        let out = SecretRedactor::redact(text);
        assert!(out.contains("[REDACTED]"));
    }
}
