use anyhow::Result;

use super::{PresetConfig, parse_headers_from_env};
use crate::export::ExportMode;

/// Honeycomb OTLP preset.
///
/// Default endpoint: `https://api.honeycomb.io:443`
/// Auth header: `x-honeycomb-team` (set via `HONEYCOMB_API_KEY` env or provider secrets)
pub fn config() -> Result<PresetConfig> {
    let endpoint = std::env::var("OTLP_ENDPOINT")
        .ok()
        .filter(|ep| !ep.is_empty())
        .or_else(|| Some(String::from("https://api.honeycomb.io:443")));

    let mut headers = parse_headers_from_env(std::env::var("OTLP_HEADERS").ok())?;
    if let Some(api_key) = std::env::var("HONEYCOMB_API_KEY")
        .ok()
        .filter(|v| !v.is_empty())
    {
        headers.entry("x-honeycomb-team".into()).or_insert(api_key);
    }

    Ok(PresetConfig {
        export_mode: Some(ExportMode::OtlpGrpc),
        otlp_endpoint: endpoint,
        otlp_headers: headers,
    })
}
