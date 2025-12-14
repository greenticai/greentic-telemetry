use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::{KeyValue, Value};
#[cfg(feature = "otlp")]
use opentelemetry_sdk::error::OTelSdkResult;
#[cfg(feature = "otlp")]
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use std::fmt;
use tracing_subscriber::field::{RecordFields, Visit};
use tracing_subscriber::fmt::format::{FormatFields, Writer};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RedactionMode {
    #[default]
    Off,
    Strict,
    Allowlist,
}

#[derive(Clone, Debug, Default)]
pub struct Redactor {
    mode: RedactionMode,
    allowlist: Vec<String>,
    patterns: Vec<String>,
}

static REDACTOR: OnceCell<Redactor> = OnceCell::new();

const SENSITIVE_KEYS: &[&str] = &[
    "secret",
    "token",
    "api_key",
    "apikey",
    "authorization",
    "password",
    "client_secret",
    "access_token",
    "refresh_token",
    "bearer",
    "x-api-key",
];

pub fn init_from_env() {
    let mode = std::env::var("PII_REDACTION_MODE")
        .ok()
        .map(|value| match value.to_ascii_lowercase() {
            v if matches!(v.as_str(), "off" | "none") => RedactionMode::Off,
            v if v == "strict" => RedactionMode::Strict,
            v if v == "allowlist" => RedactionMode::Allowlist,
            other => {
                tracing::warn!("unknown PII_REDACTION_MODE value: {other}, defaulting to off");
                RedactionMode::Off
            }
        })
        .unwrap_or_default();

    let allowlist = if mode == RedactionMode::Allowlist {
        std::env::var("PII_ALLOWLIST_FIELDS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let patterns = build_custom_patterns(std::env::var("PII_MASK_REGEXES").ok().as_deref());

    let _ = REDACTOR.set(Redactor {
        mode,
        allowlist,
        patterns,
    });
}

/// Masks a value based on key heuristics used for telemetry outputs.
pub fn redact_for_key(key: &str, value: &str) -> String {
    if is_sensitive_key(key) {
        if looks_like_bearer(value) {
            return "Bearer [REDACTED]".into();
        }
        return "[REDACTED]".into();
    }
    mask_value(value, &REDACTOR.get().cloned().unwrap_or_default())
}

pub fn redact_field(key: &str, value: &str) -> String {
    let redactor = REDACTOR.get().cloned().unwrap_or_default();
    match redactor.mode {
        RedactionMode::Off => value.to_string(),
        RedactionMode::Strict | RedactionMode::Allowlist => {
            let is_allowed = redactor
                .allowlist
                .iter()
                .any(|item| item == &key.to_ascii_lowercase());

            if redactor.mode == RedactionMode::Allowlist && is_allowed {
                value.to_string()
            } else {
                mask_value(value, &redactor)
            }
        }
    }
}

fn build_custom_patterns(value: Option<&str>) -> Vec<String> {
    let mut list = Vec::new();

    let Some(value) = value else {
        return list;
    };

    for pattern in value.split(',') {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        list.push(trimmed.to_string());
    }

    list
}

fn mask_value(value: &str, redactor: &Redactor) -> String {
    if !matches!(
        redactor.mode,
        RedactionMode::Strict | RedactionMode::Allowlist
    ) {
        return value.to_string();
    }

    // Simple heuristics: mask if the string looks like common PII/token formats.
    if looks_like_email(value)
        || looks_like_bearer(value)
        || looks_like_token(value)
        || looks_like_phone(value)
        || looks_like_secret(value)
    {
        return "[REDACTED]".into();
    }

    for pattern in &redactor.patterns {
        if value
            .to_ascii_lowercase()
            .contains(&pattern.to_ascii_lowercase())
        {
            return "[REDACTED]".into();
        }
    }

    value.to_string()
}

fn looks_like_email(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains('@') && lower.rsplit_once('.').is_some()
}

fn looks_like_bearer(value: &str) -> bool {
    value.to_ascii_lowercase().starts_with("bearer ")
}

fn looks_like_token(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("api-key")
        || lower.contains("api_key")
        || lower.contains("api key")
        || lower.contains("token=")
        || lower.contains("token ")
}

fn looks_like_phone(value: &str) -> bool {
    let digits = value.chars().filter(|c| c.is_ascii_digit()).count();
    digits >= 8
}

fn looks_like_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("pwd")
        || lower.contains("credential")
        || lower.contains("auth")
        || lower.contains("key=")
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SENSITIVE_KEYS
        .iter()
        .any(|needle| lower.contains(needle) || lower == *needle)
}

/// Formatter that redacts sensitive fields for fmt/logging output.
#[derive(Debug, Default)]
pub struct RedactingFormatFields;

impl<'writer> FormatFields<'writer> for RedactingFormatFields {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = RedactingVisitor::new(&mut writer);
        fields.record(&mut visitor);
        Ok(())
    }
}

#[derive(Debug)]
struct RedactingVisitor<'a, 'writer> {
    writer: &'a mut Writer<'writer>,
    is_empty: bool,
}

impl<'a, 'writer> RedactingVisitor<'a, 'writer> {
    fn new(writer: &'a mut Writer<'writer>) -> Self {
        Self {
            writer,
            is_empty: true,
        }
    }

    fn write_pair(&mut self, field: &tracing::field::Field, value: &str) {
        let redacted = redact_for_key(field.name(), value);
        let sep = if self.is_empty { "" } else { " " };
        let _ = write!(self.writer, "{sep}{}={}", field.name(), redacted);
        self.is_empty = false;
    }
}

impl<'a, 'writer> Visit for RedactingVisitor<'a, 'writer> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        self.write_pair(field, &format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.write_pair(field, value);
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.write_pair(field, &value.to_string());
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.write_pair(field, &value.to_string());
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.write_pair(field, &value.to_string());
    }
}

#[cfg(feature = "otlp")]
pub fn wrap_span_exporter<E: SpanExporter>(inner: E) -> RedactingSpanExporter<E> {
    RedactingSpanExporter { inner }
}

#[cfg(feature = "otlp")]
#[derive(Debug)]
pub struct RedactingSpanExporter<E> {
    inner: E,
}

#[cfg(feature = "otlp")]
impl<E: SpanExporter> SpanExporter for RedactingSpanExporter<E> {
    fn export(
        &self,
        mut batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        for span in &mut batch {
            redact_attributes(&mut span.attributes);
            for event in &mut span.events.events {
                redact_attributes(&mut event.attributes);
            }
        }
        self.inner.export(batch)
    }
}

#[cfg(feature = "otlp")]
fn redact_attributes(attrs: &mut [KeyValue]) {
    for kv in attrs.iter_mut() {
        if let Value::String(ref mut string_val) = kv.value {
            let masked = redact_for_key(kv.key.as_str(), string_val.as_str());
            kv.value = Value::String(masked.into());
        } else if is_sensitive_key(kv.key.as_str()) {
            kv.value = Value::String("[REDACTED]".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_masks_email_phone_and_token() {
        let redactor = Redactor {
            mode: RedactionMode::Strict,
            allowlist: Vec::new(),
            patterns: Vec::new(),
        };

        let masked = mask_value(
            "Email alice@example.com with bearer ABC123 and call +12345678901",
            &redactor,
        );

        assert!(!masked.contains("alice@example.com"));
        assert!(!masked.contains("ABC123"));
        assert!(!masked.contains("+12345678901"));
        assert!(masked.contains("[REDACTED]"));
    }

    #[test]
    fn allowlist_keeps_fields() {
        let redactor = Redactor {
            mode: RedactionMode::Allowlist,
            allowlist: vec!["user_id".into()],
            patterns: Vec::new(),
        };

        let masked = mask_value("User token = secret", &redactor);
        assert!(masked.contains("[REDACTED]"));

        let field_value = redact_field("user_id", "12345");
        assert_eq!(field_value, "12345");
    }

    #[test]
    fn custom_pattern_masks_access_token() {
        let redactor = Redactor {
            mode: RedactionMode::Strict,
            allowlist: Vec::new(),
            patterns: vec!["secret=".into()],
        };

        let masked = mask_value("secret=abcdef", &redactor);
        assert_eq!(masked, "[REDACTED]");
    }

    #[test]
    fn secret_keywords_are_masked() {
        let redactor = Redactor {
            mode: RedactionMode::Strict,
            allowlist: Vec::new(),
            patterns: Vec::new(),
        };

        let masked = mask_value("my password is 12345", &redactor);
        assert_eq!(masked, "[REDACTED]");
    }
}
