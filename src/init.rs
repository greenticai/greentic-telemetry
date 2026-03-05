use anyhow::{Result, anyhow};
use once_cell::sync::OnceCell;
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
use opentelemetry::{KeyValue, global};
#[cfg(feature = "otlp")]
use opentelemetry_otlp::{
    MetricExporter, SpanExporter, WithExportConfig, WithHttpConfig, WithTonicConfig,
};
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
use opentelemetry_sdk::{
    metrics::SdkMeterProvider,
    propagation::TraceContextPropagator,
    resource::Resource,
    trace::{Sampler, SdkTracerProvider},
};
#[cfg(feature = "otlp")]
use std::collections::HashMap;
#[cfg(feature = "dev")]
use std::io::IsTerminal;
#[cfg(feature = "dev")]
use tracing_appender::rolling;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp", feature = "azure", feature = "gcp"))]
use tracing_subscriber::EnvFilter;
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
use tracing_subscriber::Registry;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use tracing_subscriber::fmt;
#[cfg(any(feature = "dev", feature = "prod-json", feature = "otlp", feature = "azure", feature = "gcp"))]
use tracing_subscriber::prelude::*;

#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
use crate::export::Sampling;
#[cfg(feature = "otlp")]
use crate::export::Compression;
use crate::export::{ExportConfig, ExportMode};
use crate::redaction;
#[cfg(any(feature = "dev", feature = "prod-json"))]
use crate::redaction::RedactingFormatFields;

static INITED: OnceCell<()> = OnceCell::new();
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
static TRACER_PROVIDER: OnceCell<SdkTracerProvider> = OnceCell::new();
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
static METER_PROVIDER: OnceCell<SdkMeterProvider> = OnceCell::new();
#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
static INIT_GUARD: OnceCell<()> = OnceCell::new();

#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    /// e.g. "greentic-telemetry" or caller crate name
    pub service_name: String,
}

fn init_fmt_layers(_cfg: &TelemetryConfig) -> Result<()> {
    #[cfg(any(feature = "dev", feature = "prod-json"))]
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    #[cfg(feature = "dev")]
    {
        let cfg = _cfg;
        let filter = filter.clone();
        let file_appender = rolling::daily(".dev-logs", format!("{}.log", cfg.service_name));
        let (nb, _guard) = tracing_appender::non_blocking(file_appender);
        let stdout_is_tty = std::io::stdout().is_terminal();

        let layer_stdout = fmt::layer()
            .with_target(true)
            .fmt_fields(RedactingFormatFields)
            .pretty()
            .with_ansi(stdout_is_tty);
        let layer_file = fmt::layer()
            .with_writer(nb)
            .with_ansi(false)
            .fmt_fields(RedactingFormatFields)
            .json();

        #[cfg(feature = "otlp")]
        {
            let otel_layer = TRACER_PROVIDER.get().map(|provider| {
                use opentelemetry::trace::TracerProvider as _;
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer("greentic-telemetry"))
            });
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer_stdout)
                .with(layer_file)
                .with(otel_layer)
                .try_init();
        }
        #[cfg(not(feature = "otlp"))]
        {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer_stdout)
                .with(layer_file)
                .try_init();
        }
    }

    #[cfg(all(not(feature = "dev"), feature = "prod-json"))]
    {
        let filter = filter;
        let layer_json = fmt::layer()
            .with_target(true)
            .with_span_list(true)
            .fmt_fields(RedactingFormatFields);
        #[cfg(feature = "otlp")]
        {
            let otel_layer = TRACER_PROVIDER.get().map(|provider| {
                use opentelemetry::trace::TracerProvider as _;
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer("greentic-telemetry"))
            });
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer_json)
                .with(otel_layer)
                .try_init();
        }
        #[cfg(not(feature = "otlp"))]
        {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(layer_json)
                .try_init();
        }
    }

    // When neither dev nor prod-json is enabled but otlp is,
    // create a subscriber with just the OTel layer.
    #[cfg(all(not(feature = "dev"), not(feature = "prod-json"), feature = "otlp"))]
    {
        if let Some(provider) = TRACER_PROVIDER.get() {
            use opentelemetry::trace::TracerProvider as _;
            let filter = EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"));
            let tracer = provider.tracer("greentic-telemetry");
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_opentelemetry::layer().with_tracer(tracer))
                .try_init();
        }
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

    Ok(())
}

pub fn init_telemetry(cfg: TelemetryConfig) -> Result<()> {
    redaction::init_from_env();

    if INITED.get().is_some() {
        return Ok(());
    }

    // Set up OTLP providers FIRST so init_fmt_layers can compose the OTel layer
    // into the same subscriber as the fmt layers (tracing only allows ONE global subscriber).
    configure_otlp(&cfg.service_name)?;

    init_fmt_layers(&cfg)?;

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
    // Tonic/hyper gRPC exporters require a Tokio runtime for the underlying
    // HTTP/2 connection.  When called from a plain `fn main()` (no runtime),
    // we spin up a lightweight current-thread runtime just for the builder
    // calls and keep it alive for the background batch export tasks.
    if tokio::runtime::Handle::try_current().is_err() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| anyhow!("failed to create tokio runtime for OTLP init: {e}"))?;
        let _guard = rt.enter();
        install_otlp_inner(endpoint, resource)?;
        // Leak the runtime so the worker thread keeps driving batch exporters.
        std::mem::forget(rt);
        return Ok(());
    }
    install_otlp_inner(endpoint, resource)
}

#[cfg(feature = "otlp")]
fn install_otlp_inner(endpoint: &str, resource: Resource) -> Result<()> {
    let mut span_exporter_builder = SpanExporter::builder().with_tonic();
    span_exporter_builder = span_exporter_builder.with_endpoint(endpoint.to_string());
    let span_exporter = redaction::wrap_span_exporter(span_exporter_builder.build()?);

    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(span_exporter)
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

    // NOTE: The OTel tracing layer is composed into the subscriber by
    // init_fmt_layers() (which reads TRACER_PROVIDER). Do NOT create a
    // separate subscriber here — tracing only allows one global subscriber.

    Ok(())
}

#[cfg(any(feature = "otlp", feature = "azure", feature = "gcp"))]
pub fn shutdown() {
    if let Some(provider) = TRACER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
    if let Some(provider) = METER_PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

#[cfg(not(any(feature = "otlp", feature = "azure", feature = "gcp")))]
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

    // Ensure a Tokio runtime is available for tonic/hyper gRPC builders.
    if tokio::runtime::Handle::try_current().is_err() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| anyhow!("failed to create tokio runtime for OTLP init: {e}"))?;
        let _guard = rt.enter();
        let result = install_otlp_from_export_inner(cfg, export);
        // Leak the runtime so the worker thread keeps driving batch exporters.
        std::mem::forget(rt);
        return result;
    }
    install_otlp_from_export_inner(cfg, export)
}

#[cfg(feature = "otlp")]
fn install_otlp_from_export_inner(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    let endpoint = export.endpoint.unwrap_or_else(|| match export.mode {
        ExportMode::OtlpHttp => "http://localhost:4318".into(),
        _ => "http://localhost:4317".into(),
    });

    let mut resource_builder = Resource::builder().with_service_name(cfg.service_name);
    for (key, value) in &export.resource_attributes {
        resource_builder =
            resource_builder.with_attribute(KeyValue::new(key.clone(), value.clone()));
    }
    let resource = resource_builder.build();

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
        use opentelemetry::trace::TracerProvider as _;
        let provider = TRACER_PROVIDER.get().unwrap();
        let tracer = provider.tracer("greentic-telemetry");

        let subscriber = Registry::default()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .with(tracing_opentelemetry::layer().with_tracer(tracer));

        if let Err(e) = subscriber.try_init() {
            eprintln!("warn: failed to init OTLP tracing subscriber: {e}");
        }
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

// ---------------------------------------------------------------------------
// Azure Application Insights direct exporter
// ---------------------------------------------------------------------------

#[cfg(feature = "azure")]
fn install_azure_appinsights(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    if INIT_GUARD.get().is_some() {
        return Ok(());
    }

    // Always create a dedicated runtime for the Azure exporter.
    // The SDK's BatchSpanProcessor spawns plain OS threads that need a tokio
    // reactor for reqwest HTTP calls (hyper-util DNS resolution calls
    // Handle::current()). Reusing a caller's runtime doesn't help because
    // the background threads don't inherit the thread-local handle.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| anyhow!("failed to create tokio runtime for Azure init: {e}"))?;
    let handle = rt.handle().clone();
    let _guard = rt.enter();
    let result = install_azure_appinsights_inner(cfg, export, handle);
    // Leak the runtime so the batch processor threads keep working.
    std::mem::forget(rt);
    result
}

#[cfg(feature = "azure")]
fn install_azure_appinsights_inner(
    cfg: TelemetryConfig,
    mut export: ExportConfig,
    rt_handle: tokio::runtime::Handle,
) -> Result<()> {
    use opentelemetry::trace::TracerProvider as _;

    // Extract connection string: prefer header injected by telemetry provider WASM,
    // then env var, then reconstruct from endpoint + ikey header.
    let conn_str = export
        .headers
        .remove("_azure_connection_string")
        .or_else(|| std::env::var("APPLICATIONINSIGHTS_CONNECTION_STRING").ok())
        .or_else(|| {
            // Reconstruct from endpoint + instrumentation key if available
            let ep = export.endpoint.as_deref()?;
            let ikey = export.headers.get("x-ms-instrumentation-key")?;
            Some(format!("InstrumentationKey={ikey};IngestionEndpoint={ep}"))
        })
        .ok_or_else(|| {
            anyhow!(
                "Azure App Insights requires a connection string. \
                 Set APPLICATIONINSIGHTS_CONNECTION_STRING or configure azure_connection_string in secrets"
            )
        })?;

    global::set_text_map_propagator(TraceContextPropagator::new());

    let http_client = reqwest::Client::new();

    // Build resource
    let mut resource_builder = Resource::builder().with_service_name(cfg.service_name);
    for (key, value) in &export.resource_attributes {
        resource_builder =
            resource_builder.with_attribute(KeyValue::new(key.clone(), value.clone()));
    }
    let resource = resource_builder.build();

    let sampler = match export.sampling {
        Sampling::TraceIdRatio(ratio) if (0.0..1.0).contains(&ratio) && ratio < 1.0 => {
            Sampler::TraceIdRatioBased(ratio)
        }
        Sampling::AlwaysOff => Sampler::AlwaysOff,
        _ => Sampler::AlwaysOn,
    };

    // Trace exporter — wrap with runtime binding so the batch processor
    // thread (plain OS thread) can use reqwest for HTTP calls.
    let trace_exporter =
        opentelemetry_application_insights::Exporter::new_from_connection_string(
            &conn_str,
            http_client.clone(),
        )
        .map_err(|e| anyhow!("Azure App Insights trace exporter init failed: {e}"))?;
    let trace_exporter = RuntimeBoundSpanExporter {
        inner: trace_exporter,
        rt: rt_handle.clone(),
    };
    let trace_exporter = redaction::wrap_span_exporter(trace_exporter);

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(trace_exporter)
        .with_sampler(sampler)
        .with_resource(resource.clone())
        .build();
    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    // Metric exporter — same runtime binding.
    let metric_exporter =
        opentelemetry_application_insights::Exporter::new_from_connection_string(
            &conn_str,
            http_client,
        )
        .map_err(|e| anyhow!("Azure App Insights metric exporter init failed: {e}"))?;
    let metric_exporter = RuntimeBoundMetricExporter {
        inner: metric_exporter,
        rt: rt_handle,
    };

    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER_PROVIDER.set(meter_provider);

    // Subscriber with OTel layer
    {
        let provider = TRACER_PROVIDER.get().unwrap();
        let tracer = provider.tracer("greentic-telemetry");
        let subscriber = Registry::default()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .with(tracing_opentelemetry::layer().with_tracer(tracer));
        if let Err(e) = subscriber.try_init() {
            eprintln!("warn: failed to init Azure App Insights tracing subscriber: {e}");
        }
    }

    let _ = INIT_GUARD.set(());
    Ok(())
}

// ---------------------------------------------------------------------------
// Runtime-bound exporter wrappers
//
// The OTel SDK's BatchSpanProcessor and PeriodicReader spawn plain OS threads
// that lack a tokio runtime. reqwest (via hyper-util) needs a reactor for DNS
// resolution. These wrappers use Handle::block_on to drive the async export
// within the dedicated runtime we created above.
// ---------------------------------------------------------------------------

#[cfg(feature = "azure")]
#[derive(Debug)]
struct RuntimeBoundSpanExporter<E> {
    inner: E,
    rt: tokio::runtime::Handle,
}

#[cfg(feature = "azure")]
impl<E: opentelemetry_sdk::trace::SpanExporter + 'static>
    opentelemetry_sdk::trace::SpanExporter for RuntimeBoundSpanExporter<E>
{
    fn export(
        &self,
        batch: Vec<opentelemetry_sdk::trace::SpanData>,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        let result = self.rt.block_on(self.inner.export(batch));
        std::future::ready(result)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

#[cfg(feature = "azure")]
#[derive(Debug)]
struct RuntimeBoundMetricExporter<E> {
    inner: E,
    rt: tokio::runtime::Handle,
}

#[cfg(feature = "azure")]
impl<E: opentelemetry_sdk::metrics::exporter::PushMetricExporter>
    opentelemetry_sdk::metrics::exporter::PushMetricExporter for RuntimeBoundMetricExporter<E>
{
    fn export(
        &self,
        metrics: &opentelemetry_sdk::metrics::data::ResourceMetrics,
    ) -> impl std::future::Future<Output = opentelemetry_sdk::error::OTelSdkResult> + Send {
        let result = self.rt.block_on(self.inner.export(metrics));
        std::future::ready(result)
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn temporality(&self) -> opentelemetry_sdk::metrics::Temporality {
        self.inner.temporality()
    }
}

// ---------------------------------------------------------------------------
// AWS X-Ray exporter (OTLP transport + X-Ray ID generator / propagator)
// ---------------------------------------------------------------------------

#[cfg(feature = "aws")]
fn install_aws_xray(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    if INIT_GUARD.get().is_some() {
        return Ok(());
    }

    if tokio::runtime::Handle::try_current().is_err() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| anyhow!("failed to create tokio runtime for AWS X-Ray init: {e}"))?;
        let _guard = rt.enter();
        let result = install_aws_xray_inner(cfg, export);
        std::mem::forget(rt);
        return result;
    }
    install_aws_xray_inner(cfg, export)
}

#[cfg(feature = "aws")]
fn install_aws_xray_inner(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    use opentelemetry::trace::TracerProvider as _;

    // AWS X-Ray uses its own propagator for trace context and ID generator
    // for X-Ray compatible trace IDs (time-based first 32 bits).
    global::set_text_map_propagator(
        opentelemetry_aws::trace::XrayPropagator::default(),
    );

    let endpoint = export.endpoint.unwrap_or_else(|| "http://localhost:4317".into());

    let mut resource_builder = Resource::builder().with_service_name(cfg.service_name);
    for (key, value) in &export.resource_attributes {
        resource_builder =
            resource_builder.with_attribute(KeyValue::new(key.clone(), value.clone()));
    }
    let resource = resource_builder.build();

    let sampler = match export.sampling {
        Sampling::TraceIdRatio(ratio) if (0.0..1.0).contains(&ratio) && ratio < 1.0 => {
            Sampler::TraceIdRatioBased(ratio)
        }
        Sampling::AlwaysOff => Sampler::AlwaysOff,
        _ => Sampler::AlwaysOn,
    };

    // Build OTLP span exporter — AWS X-Ray accepts OTLP gRPC natively.
    // Set auth headers (x-api-key) via env vars for tonic.
    if !export.headers.is_empty() {
        if let Some(serialized) = serialize_headers(&export.headers) {
            unsafe {
                std::env::set_var("OTEL_EXPORTER_OTLP_HEADERS", &serialized);
                std::env::set_var("OTEL_EXPORTER_OTLP_TRACES_HEADERS", &serialized);
                std::env::set_var("OTEL_EXPORTER_OTLP_METRICS_HEADERS", serialized.clone());
            }
        }
    }
    let mut span_builder = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.clone());
    if let Some(compression) = export.compression {
        span_builder = span_builder.with_compression(map_compression(compression));
    }
    let span_exporter = span_builder.build().map_err(|e| anyhow!(e.to_string()))?;
    let span_exporter = redaction::wrap_span_exporter(span_exporter);

    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_sampler(sampler)
        .with_id_generator(opentelemetry_aws::trace::XrayIdGenerator::default())
        .with_resource(resource.clone())
        .build();
    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    // Metric exporter (same OTLP transport to AWS)
    let mut metric_builder = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint);
    if let Some(compression) = export.compression {
        metric_builder = metric_builder.with_compression(map_compression(compression));
    }
    let metric_exporter = metric_builder.build().map_err(|e| anyhow!(e.to_string()))?;

    let meter_provider = SdkMeterProvider::builder()
        .with_resource(resource)
        .with_periodic_exporter(metric_exporter)
        .build();
    global::set_meter_provider(meter_provider.clone());
    let _ = METER_PROVIDER.set(meter_provider);

    // Subscriber with OTel layer
    {
        let provider = TRACER_PROVIDER.get().unwrap();
        let tracer = provider.tracer("greentic-telemetry");
        let subscriber = Registry::default()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .with(tracing_opentelemetry::layer().with_tracer(tracer));
        if let Err(e) = subscriber.try_init() {
            eprintln!("warn: failed to init AWS X-Ray tracing subscriber: {e}");
        }
    }

    let _ = INIT_GUARD.set(());
    Ok(())
}

// ---------------------------------------------------------------------------
// GCP Cloud Trace direct exporter
// ---------------------------------------------------------------------------

#[cfg(feature = "gcp")]
fn install_gcp_cloud_trace(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    if INIT_GUARD.get().is_some() {
        return Ok(());
    }

    if tokio::runtime::Handle::try_current().is_err() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map_err(|e| anyhow!("failed to create tokio runtime for GCP init: {e}"))?;
        let _guard = rt.enter();
        let result = install_gcp_cloud_trace_inner(cfg, export);
        std::mem::forget(rt);
        return result;
    }
    install_gcp_cloud_trace_inner(cfg, export)
}

#[cfg(feature = "gcp")]
fn install_gcp_cloud_trace_inner(cfg: TelemetryConfig, mut export: ExportConfig) -> Result<()> {
    use opentelemetry::trace::TracerProvider as _;

    // Extract GCP project ID from headers (set by telemetry provider component),
    // then environment variables.
    let project_id = export
        .headers
        .remove("_gcp_project_id")
        .or_else(|| std::env::var("GOOGLE_CLOUD_PROJECT").ok())
        .or_else(|| std::env::var("GCP_PROJECT_ID").ok())
        .or_else(|| std::env::var("GCLOUD_PROJECT").ok())
        .ok_or_else(|| {
            anyhow!(
                "GCP Cloud Trace requires a project ID. \
                 Set GOOGLE_CLOUD_PROJECT or configure gcp_project_id in secrets"
            )
        })?;

    global::set_text_map_propagator(TraceContextPropagator::new());

    let mut resource_builder = Resource::builder().with_service_name(cfg.service_name);
    for (key, value) in &export.resource_attributes {
        resource_builder =
            resource_builder.with_attribute(KeyValue::new(key.clone(), value.clone()));
    }
    let resource = resource_builder.build();

    let sampler = match export.sampling {
        Sampling::TraceIdRatio(ratio) if (0.0..1.0).contains(&ratio) && ratio < 1.0 => {
            Sampler::TraceIdRatioBased(ratio)
        }
        Sampling::AlwaysOff => Sampler::AlwaysOff,
        _ => Sampler::AlwaysOn,
    };

    // GCP Cloud Trace exporter creation is async (sets up gRPC channel).
    let handle = tokio::runtime::Handle::current();
    let tracer_provider: SdkTracerProvider = handle.block_on(async {
        let gcp_builder =
            opentelemetry_gcloud_trace::GcpCloudTraceExporterBuilder::new(project_id);

        gcp_builder
            .create_provider_from_builder(
                SdkTracerProvider::builder()
                    .with_sampler(sampler)
                    .with_resource(resource),
            )
            .await
            .map_err(|e| anyhow!("GCP Cloud Trace provider creation failed: {e}"))
    })?;

    global::set_tracer_provider(tracer_provider.clone());
    let _ = TRACER_PROVIDER.set(tracer_provider);

    // GCP Cloud Trace handles traces only; metrics are not exported.
    // Use a separate OTLP metrics pipeline if needed.

    // Subscriber with OTel layer
    {
        let provider = TRACER_PROVIDER.get().unwrap();
        let tracer = provider.tracer("greentic-telemetry");
        let subscriber = Registry::default()
            .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
            .with(tracing_opentelemetry::layer().with_tracer(tracer));
        if let Err(e) = subscriber.try_init() {
            eprintln!("warn: failed to init GCP Cloud Trace tracing subscriber: {e}");
        }
    }

    let _ = INIT_GUARD.set(());
    Ok(())
}

/// Auto-configure telemetry based on env/preset-driven export settings.
pub fn init_telemetry_auto(cfg: TelemetryConfig) -> Result<()> {
    let export = ExportConfig::from_env()?;
    init_telemetry_from_config(cfg, export)
}

/// Initialize telemetry from an explicit, already-resolved config. No env/preset merging is performed here.
pub fn init_telemetry_from_config(cfg: TelemetryConfig, export: ExportConfig) -> Result<()> {
    redaction::init_from_env();

    // For OTLP modes, always proceed even if a previous init (e.g. json-stdout)
    // already ran — this allows the capability provider to upgrade the pipeline.
    // For json-stdout, honour the once-guard to avoid duplicate fmt layers.
    match export.mode {
        ExportMode::JsonStdout => {
            if INITED.get().is_some() {
                return Ok(());
            }
            init_fmt_layers(&cfg)?;
        }
        ExportMode::OtlpGrpc | ExportMode::OtlpHttp => {
            #[cfg(feature = "otlp")]
            {
                install_otlp_from_export(cfg, export)?
            }
            #[cfg(not(feature = "otlp"))]
            {
                return Err(anyhow!(
                    "otlp feature disabled; cannot install OTLP exporter from config"
                ));
            }
        }
        ExportMode::AzureAppInsights => {
            #[cfg(feature = "azure")]
            {
                install_azure_appinsights(cfg, export)?
            }
            #[cfg(not(feature = "azure"))]
            {
                return Err(anyhow!(
                    "azure feature disabled; cannot install Azure App Insights exporter"
                ));
            }
        }
        ExportMode::AwsXRay => {
            #[cfg(feature = "aws")]
            {
                install_aws_xray(cfg, export)?
            }
            #[cfg(not(feature = "aws"))]
            {
                return Err(anyhow!(
                    "aws feature disabled; cannot install AWS X-Ray exporter"
                ));
            }
        }
        ExportMode::GcpCloudTrace => {
            #[cfg(feature = "gcp")]
            {
                install_gcp_cloud_trace(cfg, export)?
            }
            #[cfg(not(feature = "gcp"))]
            {
                return Err(anyhow!(
                    "gcp feature disabled; cannot install GCP Cloud Trace exporter"
                ));
            }
        }
    }

    let _ = INITED.set(());
    Ok(())
}
