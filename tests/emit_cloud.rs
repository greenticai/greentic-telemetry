use greentic_telemetry::{TelemetryConfig, init_telemetry, shutdown};
use std::time::Duration;
use tracing::{Level, info, span};

#[tokio::test]
async fn emit_marker_to_cloud() -> anyhow::Result<()> {
    let service = std::env::var("SERVICE_NAME").unwrap_or_else(|_| "greentic-telemetry-ci".into());
    let marker =
        std::env::var("TEST_MARKER").unwrap_or_else(|_| format!("marker-{}", uuid::Uuid::new_v4()));

    init_telemetry(TelemetryConfig {
        service_name: service,
    })?;

    let span = span!(Level::INFO, "ci_emit", marker = %marker);
    let _guard = span.enter();
    info!("CI emitting telemetry with marker={}", marker);

    tokio::time::sleep(Duration::from_millis(500)).await;
    shutdown();
    Ok(())
}

/// Direct Azure App Insights exporter test.
///
/// Run with:
/// ```sh
/// APPLICATIONINSIGHTS_CONNECTION_STRING="InstrumentationKey=...;IngestionEndpoint=..." \
///   cargo test --features azure emit_azure_direct -- --nocapture --ignored
/// ```
#[cfg(feature = "azure")]
#[tokio::test]
#[ignore = "requires APPLICATIONINSIGHTS_CONNECTION_STRING"]
async fn emit_azure_direct() -> anyhow::Result<()> {
    use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
    use greentic_telemetry::init_telemetry_from_config;
    use std::collections::HashMap;

    // Skip if no connection string configured
    if std::env::var("APPLICATIONINSIGHTS_CONNECTION_STRING").is_err() {
        eprintln!("APPLICATIONINSIGHTS_CONNECTION_STRING not set — skipping");
        return Ok(());
    }

    let marker = std::env::var("TEST_MARKER")
        .unwrap_or_else(|_| format!("azure-direct-{}", uuid::Uuid::new_v4()));

    let export = ExportConfig {
        mode: ExportMode::AzureAppInsights,
        endpoint: None, // connection string has everything
        headers: HashMap::new(),
        sampling: Sampling::AlwaysOn,
        compression: None,
        resource_attributes: {
            let mut m = HashMap::new();
            m.insert("test.marker".into(), marker.clone());
            m
        },
        tls_config: None,
    };

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-azure-test".into(),
        },
        export,
    )?;

    eprintln!("Azure exporter initialized — emitting spans with marker={marker}");

    // Emit several spans to increase visibility in Azure Portal
    for i in 0..3 {
        let span = span!(Level::INFO, "azure_smoke", marker = %marker, iteration = i);
        let _guard = span.enter();
        info!(marker = %marker, iteration = i, "Azure direct exporter test span");
    }

    // Give batch exporter time to flush
    eprintln!("Waiting for batch flush...");
    tokio::time::sleep(Duration::from_secs(6)).await;

    shutdown();
    eprintln!("Shutdown complete. Check Azure Portal → App Insights → Transaction search for marker: {marker}");
    Ok(())
}

/// Direct Azure exporter init-only test (no real credentials needed).
/// Verifies the code path compiles and the exporter constructor rejects bad input.
#[cfg(feature = "azure")]
#[tokio::test]
async fn azure_exporter_rejects_missing_connection_string() {
    use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
    use greentic_telemetry::init_telemetry_from_config;
    use std::collections::HashMap;

    // Ensure no env var leaks
    // SAFETY: test runs single-threaded; no concurrent env access.
    unsafe { std::env::remove_var("APPLICATIONINSIGHTS_CONNECTION_STRING") };

    let export = ExportConfig {
        mode: ExportMode::AzureAppInsights,
        endpoint: None,
        headers: HashMap::new(),
        sampling: Sampling::AlwaysOn,
        compression: None,
        resource_attributes: HashMap::new(),
        tls_config: None,
    };

    let result = init_telemetry_from_config(
        TelemetryConfig {
            service_name: "test-azure-reject".into(),
        },
        export,
    );

    assert!(result.is_err(), "should fail without connection string");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("connection string"),
        "error should mention connection string, got: {err_msg}"
    );
}
