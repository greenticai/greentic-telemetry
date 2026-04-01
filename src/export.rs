use std::collections::HashMap;
use std::env;

use anyhow::{Context, Result, anyhow};

use crate::presets::{self, CloudPreset, PresetConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExportMode {
    JsonStdout,
    OtlpGrpc,
    OtlpHttp,
    AzureAppInsights,
    AwsXRay,
    GcpCloudTrace,
}

#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    not(any(feature = "otlp-grpc", feature = "otlp-http")),
    allow(dead_code)
)]
pub enum Sampling {
    Parent,
    TraceIdRatio(f64),
    AlwaysOn,
    AlwaysOff,
}

#[cfg_attr(
    not(any(feature = "otlp-grpc", feature = "otlp-http")),
    allow(dead_code)
)]
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ExportConfig {
    pub mode: ExportMode,
    pub endpoint: Option<String>,
    pub headers: HashMap<String, String>,
    pub sampling: Sampling,
    pub compression: Option<Compression>,
    /// Additional OTel resource attributes attached to the tracer/meter provider.
    pub resource_attributes: HashMap<String, String>,
    /// TLS configuration for mTLS connections.
    pub tls_config: Option<crate::provider::TlsConfig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    Gzip,
}

impl ExportConfig {
    pub fn json_default() -> Self {
        Self {
            mode: ExportMode::JsonStdout,
            endpoint: None,
            headers: HashMap::new(),
            sampling: Sampling::Parent,
            compression: None,
            resource_attributes: HashMap::new(),
            tls_config: None,
        }
    }

    pub fn from_env() -> Result<Self> {
        let preset = presets::detect_from_env().and_then(|preset| match preset {
            CloudPreset::None => None,
            other => Some(other),
        });

        let preset_config = if let Some(preset) = preset {
            presets::load_preset(preset)?
        } else {
            PresetConfig::default()
        };

        let explicit_export = env::var("TELEMETRY_EXPORT").ok();
        let mode = match explicit_export
            .clone()
            .unwrap_or_else(|| "json-stdout".to_string())
            .to_ascii_lowercase()
            .as_str()
        {
            "json-stdout" => ExportMode::JsonStdout,
            "otlp-grpc" => ExportMode::OtlpGrpc,
            "otlp-http" => ExportMode::OtlpHttp,
            "azure-appinsights" => ExportMode::AzureAppInsights,
            "aws-xray" => ExportMode::AwsXRay,
            "gcp-cloud-trace" => ExportMode::GcpCloudTrace,
            other => {
                return Err(anyhow!(
                    "unsupported TELEMETRY_EXPORT value: {other}. expected one of \
                     json-stdout, otlp-grpc, otlp-http, azure-appinsights, aws-xray, gcp-cloud-trace"
                ));
            }
        };

        let mut endpoint = env::var("OTLP_ENDPOINT").ok().filter(|s| !s.is_empty());
        if endpoint.is_none() {
            endpoint = preset_config.otlp_endpoint;
        }

        let mut headers = parse_headers(env::var("OTLP_HEADERS").ok().as_deref())?;
        if headers.is_empty() {
            headers = preset_config.otlp_headers;
        }

        let sampling = parse_sampling(env::var("TELEMETRY_SAMPLING").ok().as_deref())?;
        let compression = parse_compression(env::var("OTLP_COMPRESSION").ok().as_deref());

        let inferred_mode = if explicit_export.is_none() {
            preset_config.export_mode.unwrap_or(match preset {
                Some(CloudPreset::Loki) => ExportMode::JsonStdout,
                _ => mode,
            })
        } else {
            mode
        };

        Ok(Self {
            mode: inferred_mode,
            endpoint,
            headers,
            sampling,
            compression,
            resource_attributes: HashMap::new(),
            tls_config: None,
        })
    }
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self::json_default()
    }
}

fn parse_headers(value: Option<&str>) -> Result<HashMap<String, String>> {
    let mut headers = HashMap::new();

    let Some(value) = value else {
        return Ok(headers);
    };

    for pair in value.split(',') {
        let trimmed = pair.trim();
        if trimmed.is_empty() {
            continue;
        }

        let (key, val) = trimmed
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid OTLP_HEADERS entry '{trimmed}', expected key=value"))?;

        if key.trim().is_empty() {
            return Err(anyhow!(
                "invalid OTLP_HEADERS entry '{trimmed}', key cannot be empty"
            ));
        }

        headers.insert(key.trim().to_string(), val.trim().to_string());
    }

    Ok(headers)
}

fn parse_sampling(value: Option<&str>) -> Result<Sampling> {
    let Some(value) = value else {
        return Ok(Sampling::Parent);
    };

    let normalized = value.to_ascii_lowercase();
    match normalized.as_str() {
        "parent" => return Ok(Sampling::Parent),
        "always_on" | "always-on" => return Ok(Sampling::AlwaysOn),
        "always_off" | "always-off" => return Ok(Sampling::AlwaysOff),
        _ => {}
    }

    if let Some(rest) = normalized.strip_prefix("traceidratio:") {
        let ratio: f64 = rest
            .parse()
            .with_context(|| format!("invalid traceidratio value '{rest}'"))?;
        if !(0.0..=1.0).contains(&ratio) {
            return Err(anyhow!(
                "traceidratio must be between 0.0 and 1.0 inclusive, got {ratio}"
            ));
        }
        return Ok(Sampling::TraceIdRatio(ratio));
    }

    Err(anyhow!(
        "unsupported TELEMETRY_SAMPLING '{value}', expected parent, always_on, always_off, or traceidratio:<ratio>"
    ))
}

fn parse_compression(value: Option<&str>) -> Option<Compression> {
    let value = value?.to_ascii_lowercase();
    match value.as_str() {
        "gzip" => Some(Compression::Gzip),
        _ => None,
    }
}
