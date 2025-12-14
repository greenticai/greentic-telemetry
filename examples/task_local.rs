use greentic_telemetry::{
    TelemetryConfig, TelemetryCtx, init_telemetry_auto, set_current_telemetry_ctx, with_task_local,
};

#[tokio::main]
async fn main() {
    with_task_local(async {
        unsafe {
            std::env::set_var("TELEMETRY_EXPORT", "json-stdout");
        }
        let _ = init_telemetry_auto(TelemetryConfig {
            service_name: "telemetry-demo".into(),
        });

        set_current_telemetry_ctx(
            TelemetryCtx::new("acme")
                .with_session("s1")
                .with_flow("onboard")
                .with_node("qa-1"),
        );

        tracing::info!("hello with tenant-aware fields");
    })
    .await;
}
