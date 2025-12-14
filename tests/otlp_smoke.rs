#![cfg(feature = "otlp")]

use greentic_telemetry::{TelemetryConfig, init_telemetry_auto};
use std::env;

#[tokio::test(flavor = "current_thread")]
async fn otlp_pipeline_initializes() {
    unsafe {
        env::set_var("TELEMETRY_EXPORT", "otlp-grpc");
        env::set_var("OTLP_ENDPOINT", "http://localhost:4317");
        env::set_var("TELEMETRY_SAMPLING", "traceidratio:1.0");
    }

    init_telemetry_auto(TelemetryConfig {
        service_name: "greentic-telemetry-test".into(),
    })
    .expect("otlp init succeeds");
}
