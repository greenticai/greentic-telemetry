use std::collections::HashMap;

use anyhow::{Context, Result};

pub mod aws;
pub mod azure;
pub mod datadog;
pub mod elastic;
pub mod gcp;
pub mod grafana_tempo;
pub mod honeycomb;
pub mod jaeger;
pub mod loki;
pub mod newrelic;
pub mod otlp_grpc;
pub mod otlp_http;
pub mod stdout;
pub mod zipkin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CloudPreset {
    Aws = 0,
    Gcp = 1,
    Azure = 2,
    Datadog = 3,
    Loki = 4,
    Honeycomb = 6,
    NewRelic = 7,
    Elastic = 8,
    GrafanaTempo = 9,
    Jaeger = 10,
    Zipkin = 11,
    OtlpGrpc = 12,
    OtlpHttp = 13,
    Stdout = 14,
    None = 5,
}

#[derive(Debug, Default, Clone)]
#[non_exhaustive]
pub struct PresetConfig {
    pub export_mode: Option<crate::export::ExportMode>,
    pub otlp_endpoint: Option<String>,
    pub otlp_headers: HashMap<String, String>,
    /// Recommended sampling ratio for this preset. `None` = defer to user config.
    pub sampling_ratio: Option<f64>,
}

pub fn detect_from_env() -> Option<CloudPreset> {
    let value = std::env::var("CLOUD_PRESET").ok()?.to_ascii_lowercase();
    match value.as_str() {
        "aws" => Some(CloudPreset::Aws),
        "gcp" => Some(CloudPreset::Gcp),
        "azure" => Some(CloudPreset::Azure),
        "datadog" => Some(CloudPreset::Datadog),
        "loki" => Some(CloudPreset::Loki),
        "honeycomb" => Some(CloudPreset::Honeycomb),
        "newrelic" | "new_relic" | "new-relic" => Some(CloudPreset::NewRelic),
        "elastic" => Some(CloudPreset::Elastic),
        "grafana-tempo" | "grafana_tempo" => Some(CloudPreset::GrafanaTempo),
        "jaeger" => Some(CloudPreset::Jaeger),
        "zipkin" => Some(CloudPreset::Zipkin),
        "otlp-grpc" | "otlp_grpc" => Some(CloudPreset::OtlpGrpc),
        "otlp-http" | "otlp_http" => Some(CloudPreset::OtlpHttp),
        "stdout" => Some(CloudPreset::Stdout),
        "none" => Some(CloudPreset::None),
        other => {
            tracing::warn!("unknown CLOUD_PRESET value: {other}");
            None
        }
    }
}

pub fn load_preset(preset: CloudPreset) -> Result<PresetConfig> {
    match preset {
        CloudPreset::Aws => aws::config(),
        CloudPreset::Gcp => gcp::config(),
        CloudPreset::Azure => azure::config(),
        CloudPreset::Datadog => datadog::config(),
        CloudPreset::Loki => loki::config(),
        CloudPreset::Honeycomb => honeycomb::config(),
        CloudPreset::NewRelic => newrelic::config(),
        CloudPreset::Elastic => elastic::config(),
        CloudPreset::GrafanaTempo => grafana_tempo::config(),
        CloudPreset::Jaeger => jaeger::config(),
        CloudPreset::Zipkin => zipkin::config(),
        CloudPreset::OtlpGrpc => otlp_grpc::config(),
        CloudPreset::OtlpHttp => otlp_http::config(),
        CloudPreset::Stdout => stdout::config(),
        CloudPreset::None => Ok(PresetConfig::default()),
    }
}

pub fn parse_headers_from_env(headers: Option<String>) -> Result<HashMap<String, String>> {
    let headers = headers.unwrap_or_default();
    let mut map = HashMap::new();
    for pair in headers.split(',') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (key, value) = trimmed
            .split_once('=')
            .with_context(|| format!("invalid OTLP_HEADERS entry '{trimmed}'"))?;
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(map)
}
