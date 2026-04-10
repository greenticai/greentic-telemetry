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

    let mut export = ExportConfig::default();
    export.mode = ExportMode::AzureAppInsights;
    export.sampling = Sampling::AlwaysOn;
    export.resource_attributes = {
        let mut m = HashMap::new();
        m.insert("test.marker".into(), marker.clone());
        m
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
    eprintln!(
        "Shutdown complete. Check Azure Portal → App Insights → Transaction search for marker: {marker}"
    );
    Ok(())
}

/// Direct GCP Cloud Trace exporter test.
///
/// Run with:
/// ```sh
/// GOOGLE_CLOUD_PROJECT="greentic-489320" \
///   cargo test --features gcp emit_gcp_direct -- --nocapture --ignored
/// ```
#[cfg(feature = "gcp")]
#[tokio::test]
#[ignore = "requires GOOGLE_CLOUD_PROJECT"]
async fn emit_gcp_direct() -> anyhow::Result<()> {
    use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
    use greentic_telemetry::init_telemetry_from_config;
    use std::collections::HashMap;

    let project_id = match std::env::var("GOOGLE_CLOUD_PROJECT") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("GOOGLE_CLOUD_PROJECT not set — skipping");
            return Ok(());
        }
    };

    let marker = std::env::var("TEST_MARKER")
        .unwrap_or_else(|_| format!("gcp-direct-{}", uuid::Uuid::new_v4()));

    let mut export = ExportConfig::default();
    export.mode = ExportMode::GcpCloudTrace;
    export.headers = {
        let mut h = HashMap::new();
        h.insert("_gcp_project_id".into(), project_id.clone());
        h
    };
    export.sampling = Sampling::AlwaysOn;
    export.resource_attributes = {
        let mut m = HashMap::new();
        m.insert("test.marker".into(), marker.clone());
        m
    };

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-gcp-test".into(),
        },
        export,
    )?;

    eprintln!(
        "GCP exporter initialized (project={project_id}) — emitting spans with marker={marker}"
    );

    for i in 0..3 {
        let span = span!(Level::INFO, "gcp_smoke", marker = %marker, iteration = i);
        let _guard = span.enter();
        info!(marker = %marker, iteration = i, "GCP Cloud Trace test span");
    }

    // Give batch exporter time to flush
    eprintln!("Waiting for batch flush...");
    tokio::time::sleep(Duration::from_secs(6)).await;

    shutdown();
    eprintln!("Shutdown complete. Check GCP Console → Trace Explorer for marker: {marker}");
    Ok(())
}

/// Direct AWS X-Ray exporter test (no ADOT collector needed).
///
/// Run with:
/// ```sh
/// AWS_ACCESS_KEY_ID="..." AWS_SECRET_ACCESS_KEY="..." AWS_REGION="eu-west-1" \
///   cargo test --features aws emit_aws_direct -- --nocapture --ignored
/// ```
#[cfg(feature = "aws")]
#[tokio::test]
#[ignore = "requires AWS credentials"]
async fn emit_aws_direct() -> anyhow::Result<()> {
    use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
    use greentic_telemetry::init_telemetry_from_config;
    use std::collections::HashMap;

    // Skip if no credentials
    if std::env::var("AWS_ACCESS_KEY_ID").is_err() {
        eprintln!("AWS_ACCESS_KEY_ID not set — skipping");
        return Ok(());
    }

    let region = std::env::var("AWS_REGION")
        .or_else(|_| std::env::var("AWS_DEFAULT_REGION"))
        .unwrap_or_else(|_| "eu-west-1".into());

    let marker = std::env::var("TEST_MARKER")
        .unwrap_or_else(|_| format!("aws-direct-{}", uuid::Uuid::new_v4()));

    // No endpoint → direct mode (OTLP/HTTP to X-Ray with SigV4)
    let mut export = ExportConfig::default();
    export.mode = ExportMode::AwsXRay;
    export.sampling = Sampling::AlwaysOn;
    export.resource_attributes = {
        let mut m = HashMap::new();
        m.insert("test.marker".into(), marker.clone());
        m
    };

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-telemetry-aws-test".into(),
        },
        export,
    )?;

    eprintln!("AWS X-Ray direct exporter initialized (region={region}) — marker={marker}");

    for i in 0..3 {
        let span = span!(Level::INFO, "aws_smoke", marker = %marker, iteration = i);
        let _guard = span.enter();
        info!(marker = %marker, iteration = i, "AWS X-Ray direct test span");
    }

    eprintln!("Waiting for batch flush...");
    tokio::time::sleep(Duration::from_secs(8)).await;

    shutdown();
    eprintln!("Shutdown complete. Check AWS X-Ray console for marker: {marker}");
    Ok(())
}

/// Direct Azure exporter init-only test (no real credentials needed).
/// Verifies the code path compiles and the exporter constructor rejects bad input.
#[cfg(feature = "azure")]
#[tokio::test]
async fn azure_exporter_rejects_missing_connection_string() {
    use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
    use greentic_telemetry::init_telemetry_from_config;

    // Ensure no env var leaks
    // SAFETY: test runs single-threaded; no concurrent env access.
    unsafe { std::env::remove_var("APPLICATIONINSIGHTS_CONNECTION_STRING") };

    let mut export = ExportConfig::default();
    export.mode = ExportMode::AzureAppInsights;
    export.sampling = Sampling::AlwaysOn;

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
