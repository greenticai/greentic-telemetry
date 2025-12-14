use anyhow::{Result, anyhow};
use once_cell::sync::OnceCell;
#[cfg(feature = "otlp")]
use opentelemetry::global;
#[cfg(feature = "otlp")]
use opentelemetry_otlp::{
    MetricExporter, SpanExporter, WithExportConfig, WithHttpConfig, WithTonicConfig,
};
#[cfg(feature = "otlp")]
use opentelemetry_sdk::{
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    resource::Resource,
    trace::{BatchSpanProcessor, Sampler, SdkTracerProvider},
};
#[cfg(feature = "otlp")]
use std::collections::HashMap;
#[cfg(feature = "dev")]
use tracing_appender::rolling;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp"))]
use tracing_subscriber::EnvFilter;
#[cfg(feature = "otlp")]
use tracing_subscriber::Registry;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use tracing_subscriber::fmt;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp"))]
use tracing_subscriber::prelude::*;

#[cfg(feature = "otlp")]
use crate::export::{Compression, Sampling};
use crate::export::{ExportConfig, ExportMode};
use crate::redaction;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use crate::redaction::RedactingFormatFields;

static INITED: OnceCell<()> = OnceCell::new();
#[cfg(feature = "otlp")]
static TRACER_PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();
#[cfg(feature = "otlp")]
static METER_PROVIDER: OnceCell<SdkMeterProvider> = OnceCell::new();
#[cfg(feature = "otlp")]
static INIT_GUARD: OnceCell<()> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    /// e.g. "greentic-telemetry" or caller crate name
    pub service_name: String,
}

pub fn init_telemetry(cfg: TelemetryConfig) -> Result<()> {
    redaction::init_from_env();

    if INITED.get().is_some() {
        return Ok(());
    }

    #[cfg(any(feature = "dev", feature = "prod-json"))]
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    #[cfg(feature = "dev")]
    {
        let filter = filter.clone();
        let file_appender = rolling::daily(".dev-logs", format!("{}.log", cfg.service_name));
        let (nb, _guard) = tracing_appender::non_blocking(file_appender);

        let layer_stdout = fmt::layer()
            .with_target(true)
            .fmt_fields(RedactingFormatFields)
            .pretty()
            .with_ansi(atty::is(atty::Stream::Stdout));
        let layer_file = fmt::layer()
            .with_writer(nb)
            .with_ansi(false)
            .fmt_fields(RedactingFormatFields)
            .json();

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(layer_stdout)
            .with(layer_file)
            .try_init();
    }

    #[cfg(all(not(feature = "dev"), feature = "prod-json"))]
    {
        let filter = filter;
        let layer_json = fmt::layer()
            .with_target(true)
            .with_span_list(true)
            .fmt_fields(RedactingFormatFields);
        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(layer_json)
            .try_init();
    }

    #[cfg(feature = "dev-console")]
    {
        if std::env::var_os("TOKIO_CONSOLE").is_some()
            && std::panic::catch_unwind(console_subscriber::init).is_err()
        {
            tracing::warn!(
                "dev-console feature enabled but tokio_unstable not set; skipping console subscriber init"
            );
        }
    }

    configure_otlp(&cfg.service_name)?;

    let _ = INITED.set(());
    Ok(())
}

#[cfg(feature = "otlp")]
fn configure_otlp(service_name: &str) -> Result<()> {
    global::set_text_map_propagator(TraceContextPropagator::new());

    if let Ok(endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
        let resource = Resource::builder()
            .with_service_name(service_name.to_string())
            .build();
        install_otlp(&endpoint, resource)?;
    }

    Ok(())
}

#[cfg(not(feature = "otlp"))]
fn configure_otlp(service_name: &str) -> Result<()> {
    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
        tracing::warn!(
            service = %service_name,
            "otlp feature disabled; ignoring OTEL_EXPORTER_OTLP_ENDPOINT"
        );
    }
    Ok(())
}

#[cfg(feature = "otlp")]
fn install_otlp(endpoint: &str, resource: Resource) -> Result<()> {
    let mut span_exporter_builder = SpanExporter::builder().with_tonic();
    span_exporter_builder = span_exporter_builder.with_endpoint(endpoint.to_string());
    let span_exporter = redaction::wrap_span_exporter(span_exporter_builder.build()?);

    let span_processor = BatchSpanProcessor::builder(span_exporter).build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_span_processor(span_processor)
        .build();
    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    let mut metric_exporter_builder = MetricExporter::builder().with_tonic();
    metric_exporter_builder = metric_exporter_builder.with_endpoint(endpoint.to_string());
    let metric_exporter = metric_exporter_builder.build()?;
    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER_PROVIDER.set(meter_provider);

    Ok(())
}

#[cfg(feature = "otlp")]
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
    if let Some(provider) = METER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

#[cfg(not(feature = "otlp"))]
pub fn shutdown() {}

#[cfg(feature = "otlp")]
fn serialize_headers(headers: &HashMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for (key, value) in headers {
        if key.trim().is_empty() {
            continue;
        }
        parts.push(format!("{key}={value}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

#[cfg(feature = "otlp")]
fn install_otlp_from_export(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    if INIT_GUARD.get().is_some() {
        return Ok(());
    }

    let endpoint = export.endpoint.unwrap_or_else(|| match export.mode {
        ExportMode::OtlpHttp => "http://localhost:4318".into(),
        _ => "http://localhost:4317".into(),
    });

    let resource = Resource::builder()
        .with_service_name(cfg.service_name)
        .build();

    let sampler = match export.sampling {
        Sampling::TraceIdRatio(ratio) if (0.0..1.0).contains(&ratio) && ratio < 1.0 => {
            Sampler::TraceIdRatioBased(ratio)
        }
        Sampling::AlwaysOff => Sampler::AlwaysOff,
        _ => Sampler::AlwaysOn,
    };

    let span_exporter = if matches!(export.mode, ExportMode::OtlpHttp) {
        let mut builder = SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint.clone());
        if !export.headers.is_empty() {
            builder = builder.with_headers(export.headers.clone());
        }
        if let Some(compression) = export.compression {
            builder = builder.with_compression(map_compression(compression));
        }
        builder.build().map_err(|e| anyhow!(e.to_string()))?
    } else {
        if let Some(serialized) = serialize_headers(&export.headers) {
            unsafe {
                std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", &serialized);
                std::env::set_var("OTEL_EXPORTER_OTLP_TRACES_HEADERS", &serialized);
                std::env::set_var("OTEL_EXPORTER_OTLP_METRICS_HEADERS", serialized.clone());
            }
        }
        let mut builder = SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone());
        if let Some(compression) = export.compression {
            builder = builder.with_compression(map_compression(compression));
        }
        builder.build().map_err(|e| anyhow!(e.to_string()))?
    };
    let span_exporter = redaction::wrap_span_exporter(span_exporter);

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_sampler(sampler)
        .with_resource(resource.clone())
        .build();
    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    let metric_exporter = if matches!(export.mode, ExportMode::OtlpHttp) {
        let mut builder = MetricExporter::builder()
            .with_http()
            .with_endpoint(endpoint.clone());
        if !export.headers.is_empty() {
            builder = builder.with_headers(export.headers.clone());
        }
        if let Some(compression) = export.compression {
            builder = builder.with_compression(map_compression(compression));
        }
        builder.build().map_err(|e| anyhow!(e.to_string()))?
    } else {
        let mut builder = MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone());
        if let Some(compression) = export.compression {
            builder = builder.with_compression(map_compression(compression));
        }
        builder.build().map_err(|e| anyhow!(e.to_string()))?
    };

    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER_PROVIDER.set(meter_provider);

    {
        let tracer = global::tracer("greentic-telemetry");

        let subscriber = Registry::default()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .with(tracing_opentelemetry::layer().with_tracer(tracer));

        let _ = subscriber.try_init();
    }

    let _ = INIT_GUARD.set(());

    Ok(())
}
#[cfg(feature = "otlp")]
fn map_compression(c: Compression) -> opentelemetry_otlp::Compression {
    match c {
        Compression::Gzip => opentelemetry_otlp::Compression::Gzip,
    }
}

/// Auto-configure telemetry based on env/preset-driven export settings.
pub fn init_telemetry_auto(cfg: TelemetryConfig) -> Result<()> {
    redaction::init_from_env();

    let export = ExportConfig::from_env()?;
    match export.mode {
        ExportMode::JsonStdout => init_telemetry(cfg),
        ExportMode::OtlpGrpc | ExportMode::OtlpHttp => {
            #[cfg(feature = "otlp")]
            {
                install_otlp_from_export(cfg, export)
            }
            #[cfg(not(feature = "otlp"))]
            {
                Err(anyhow!(
                    "otlp feature disabled; cannot install OTLP exporter from auto-config"
                ))
            }
        }
    }
}
