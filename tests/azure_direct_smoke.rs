#![cfg(feature = "azure")]

//! Standalone Azure App Insights smoke test.
//! Bypasses the tracing subscriber and directly tests the exporter pipeline.
//!
//! Run:
//! ```sh
//! APPLICATIONINSIGHTS_CONNECTION_STRING="InstrumentationKey=...;IngestionEndpoint=..." \
//!   cargo test --features azure --test azure_direct_smoke -- --nocapture --ignored
//! ```

use opentelemetry::KeyValue;
use opentelemetry::trace::{SpanId, SpanKind, Status, TraceFlags, TraceId, TraceState};
use opentelemetry_sdk::resource::Resource;
use opentelemetry_sdk::trace::{SpanData, SpanExporter};
use std::borrow::Cow;
use std::time::SystemTime;

fn make_test_span(marker: &str, name: &str, kind: SpanKind) -> SpanData {
    let now = SystemTime::now();
    SpanData {
        span_context: opentelemetry::trace::SpanContext::new(
            TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap(),
            SpanId::from_hex("00f067aa0ba902b7").unwrap(),
            TraceFlags::SAMPLED,
            false,
            TraceState::default(),
        ),
        parent_span_id: SpanId::INVALID,
        name: Cow::Owned(name.to_string()),
        start_time: now,
        end_time: now,
        attributes: vec![
            KeyValue::new("test.marker", marker.to_string()),
            KeyValue::new("test.source", "greentic-telemetry"),
        ],
        events: Default::default(),
        links: Default::default(),
        status: Status::Ok,
        span_kind: kind,
        dropped_attributes_count: 0,
        parent_span_is_remote: false,
        instrumentation_scope: Default::default(),
    }
}

#[tokio::test]
#[ignore = "requires APPLICATIONINSIGHTS_CONNECTION_STRING"]
async fn export_span_directly() {
    let conn_str = match std::env::var("APPLICATIONINSIGHTS_CONNECTION_STRING") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("APPLICATIONINSIGHTS_CONNECTION_STRING not set — skipping");
            return;
        }
    };

    let marker = format!("direct-export-{}", uuid::Uuid::new_v4());
    eprintln!("Marker: {marker}");

    // Create exporter directly
    let client = reqwest::Client::new();
    let mut exporter =
        opentelemetry_application_insights::Exporter::new_from_connection_string(&conn_str, client)
            .expect("exporter init");

    // Set resource
    let resource = Resource::builder()
        .with_service_name("greentic-telemetry-azure-smoke")
        .with_attribute(KeyValue::new("test.marker", marker.clone()))
        .build();
    exporter.set_resource(&resource);

    // Export spans of different kinds to cover Azure's type mapping:
    // - Server → requests table
    // - Client/Internal → dependencies table
    // - Producer/Consumer → dependencies table
    let spans = vec![
        make_test_span(&marker, "smoke-server-span", SpanKind::Server),
        make_test_span(&marker, "smoke-internal-span", SpanKind::Internal),
        make_test_span(&marker, "smoke-client-span", SpanKind::Client),
    ];

    eprintln!("Exporting {} spans to Azure...", spans.len());
    let result = exporter.export(spans).await;
    match &result {
        Ok(()) => eprintln!("Export succeeded!"),
        Err(e) => eprintln!("Export failed: {e:?}"),
    }
    assert!(result.is_ok(), "export should succeed: {result:?}");

    eprintln!(
        "Done. Check Azure Portal for marker: {marker}\n\
         KQL queries to try:\n\
         1. requests | where timestamp > ago(15m) | where name contains 'smoke'\n\
         2. dependencies | where timestamp > ago(15m) | where name contains 'smoke'\n\
         3. union requests, dependencies | where timestamp > ago(15m) | take 20"
    );
}
