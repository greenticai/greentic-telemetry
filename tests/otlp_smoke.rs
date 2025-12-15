#![cfg(feature = "otlp")]

use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
use greentic_telemetry::{TelemetryConfig, init_telemetry_from_config};
use std::collections::HashMap;

#[tokio::test(flavor = "current_thread")]
async fn otlp_pipeline_initializes() {
    let export = ExportConfig {
        mode: ExportMode::OtlpGrpc,
        endpoint: Some("http://localhost:4317".into()),
        headers: HashMap::new(),
        sampling: Sampling::TraceIdRatio(1.0),
        compression: None,
    };

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-test".into(),
        },
        export,
    )
    .expect("otlp init succeeds");
}
