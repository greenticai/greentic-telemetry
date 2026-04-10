#![cfg(feature = "otlp")]

use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
use greentic_telemetry::{TelemetryConfig, init_telemetry_from_config};

#[tokio::test(flavor = "current_thread")]
async fn otlp_pipeline_initializes() {
    let mut export = ExportConfig::default();
    export.mode = ExportMode::OtlpGrpc;
    export.endpoint = Some("http://localhost:4317".into());
    export.sampling = Sampling::TraceIdRatio(1.0);

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-test".into(),
        },
        export,
    )
    .expect("otlp init succeeds");
}
