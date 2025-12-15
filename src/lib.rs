#[cfg(feature = "otlp")]
pub mod client;
pub mod context;
pub mod export;
#[cfg(feature = "otlp")]
pub mod host_bridge;
pub mod init;
pub mod layer;
pub mod presets;
pub mod redaction;
pub mod secrets;
pub mod tasklocal;
pub mod testutil;
pub mod wasm_guest;
pub mod wasm_host;

#[cfg(feature = "otlp")]
pub use client::{init, metric, set_trace_id, span};
pub use context::TelemetryCtx;
#[cfg(feature = "otlp")]
pub use host_bridge::{HostContext, emit_span as emit_host_span};
pub use init::{
    TelemetryConfig, init_telemetry, init_telemetry_auto, init_telemetry_from_config, shutdown,
};
pub use layer::{layer_from_task_local, layer_with_provider};
pub use secrets::*;
pub use tasklocal::{set_current_telemetry_ctx, with_current_telemetry_ctx, with_task_local};
