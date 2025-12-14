use std::io::Write;
use std::sync::{Arc, Mutex};

use greentic_telemetry::redaction::RedactingFormatFields;
use greentic_telemetry::redaction::wrap_span_exporter;
use greentic_telemetry::secrets::{SecretOp, SecretResult, record_secret_attrs, secret_span};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::trace::{SdkTracerProvider, SpanData, SpanExporter};
use tracing::{Level, subscriber};
use tracing_subscriber::prelude::*;

#[derive(Clone)]
struct BufferWriter(Arc<Mutex<Vec<u8>>>);

impl Write for BufferWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self.0.lock().unwrap();
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn fmt_redacts_secretish_fields() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let writer = {
        let captured = captured.clone();
        move || BufferWriter(captured.clone())
    };

    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .with_writer(writer)
            .with_ansi(false)
            .fmt_fields(RedactingFormatFields),
    );
    let _guard = subscriber::set_default(subscriber);

    let span = secret_span(
        SecretOp::Get,
        "my/secret",
        "dev",
        "tenant-a",
        Some("team-1"),
    );
    let _enter = span.enter();
    record_secret_attrs(
        SecretOp::Get,
        "my/secret",
        "dev",
        "tenant-a",
        Some("team-1"),
        SecretResult::Ok,
        None::<String>,
    );
    tracing::info!(
        authorization = "Bearer abc.def.ghi",
        client_secret = "supersecret",
        "fetching secret"
    );
    drop(_enter);
    drop(span);

    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(output.contains("secrets.key"));
    assert!(output.contains("secrets.op"));
    assert!(!output.contains("abc.def.ghi"));
    assert!(!output.contains("supersecret"));
}

#[derive(Clone, Debug)]
struct TestExporter {
    spans: Arc<Mutex<Vec<SpanData>>>,
}

impl SpanExporter for TestExporter {
    fn export(
        &self,
        batch: Vec<SpanData>,
    ) -> impl std::future::Future<Output = OTelSdkResult> + Send {
        let spans = self.spans.clone();
        async move {
            spans.lock().unwrap().extend(batch);
            Ok(())
        }
    }
}

#[test]
fn otlp_export_redacts_sensitive_values() {
    let exported = Arc::new(Mutex::new(Vec::new()));
    let exporter = wrap_span_exporter(TestExporter {
        spans: exported.clone(),
    });
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();
    let tracer = provider.tracer("redaction-test");

    let subscriber =
        tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
    let _guard = subscriber::set_default(subscriber);

    let span = secret_span(SecretOp::Put, "api/key", "prod", "runtime", None::<&str>);
    let _enter = span.enter();
    record_secret_attrs(
        SecretOp::Put,
        "api/key",
        "prod",
        "runtime",
        None::<&str>,
        SecretResult::Error,
        Some("host_error"),
    );
    tracing::event!(
        Level::INFO,
        authorization = "Bearer abc.def.ghi",
        client_secret = "supersecret",
        message = "storing secret"
    );
    drop(_enter);
    drop(span);
    let _ = provider.force_flush();
    let _ = provider.shutdown();

    let spans = exported.lock().unwrap();
    let span = spans.last().expect("span exported");

    fn attrs_to_string<'a>(attrs: impl Iterator<Item = &'a opentelemetry::KeyValue>) -> String {
        attrs
            .map(|kv| format!("{}:{:?}", kv.key, kv.value))
            .collect::<Vec<_>>()
            .join(",")
    }

    let attrs = attrs_to_string(span.attributes.iter());
    assert!(attrs.contains("secrets.key"));
    assert!(attrs.contains("secrets.op"));
    assert!(!attrs.contains("abc.def.ghi"));
    assert!(!attrs.contains("supersecret"));

    for event in span.events.iter() {
        let event_attrs = attrs_to_string(event.attributes.iter());
        assert!(!event_attrs.contains("abc.def.ghi"));
        assert!(!event_attrs.contains("supersecret"));
    }
}
