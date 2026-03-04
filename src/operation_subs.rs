use serde::{Deserialize, Serialize};

use crate::provider::TelemetryProviderConfig;

/// Mode for operation subscription emission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubsMode {
    MetricsOnly,
    TracesOnly,
    MetricsAndTraces,
}

/// Policy for including payload data in telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayloadPolicy {
    /// No payload data in spans/metrics
    None,
    /// Include a hash of the payload only
    HashOnly,
}

/// Configuration for operation subscription emission.
#[derive(Clone, Debug)]
pub struct OperationSubsConfig {
    pub enabled: bool,
    pub mode: SubsMode,
    pub include_denied: bool,
    pub payload_policy: PayloadPolicy,
}

impl Default for OperationSubsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: SubsMode::MetricsAndTraces,
            include_denied: true,
            payload_policy: PayloadPolicy::None,
        }
    }
}

/// Build an [`OperationSubsConfig`] from a [`TelemetryProviderConfig`].
pub fn subs_config_from_provider(config: &TelemetryProviderConfig) -> OperationSubsConfig {
    let mode = config
        .operation_subs_mode
        .as_deref()
        .map(|m| match m.to_ascii_lowercase().as_str() {
            "metrics_only" => SubsMode::MetricsOnly,
            "traces_only" => SubsMode::TracesOnly,
            _ => SubsMode::MetricsAndTraces,
        })
        .unwrap_or(SubsMode::MetricsAndTraces);

    let payload_policy = config
        .payload_policy
        .as_deref()
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "hash_only" => PayloadPolicy::HashOnly,
            _ => PayloadPolicy::None,
        })
        .unwrap_or(PayloadPolicy::None);

    OperationSubsConfig {
        enabled: config.enable_operation_subs,
        mode,
        include_denied: config.include_denied_ops,
        payload_policy,
    }
}

/// Emit a structured "operation requested" event on the current tracing context.
///
/// When `payload_policy` is [`PayloadPolicy::None`], payload size is omitted from the span.
/// When [`PayloadPolicy::HashOnly`], payload size is included (callers should provide
/// size rather than raw content).
pub fn emit_operation_requested(
    config: &OperationSubsConfig,
    op_id: &str,
    op_name: &str,
    tenant: &str,
    team: &str,
    payload_size: usize,
) {
    if !config.enabled {
        return;
    }
    if matches!(config.mode, SubsMode::MetricsOnly) {
        return;
    }
    match config.payload_policy {
        PayloadPolicy::None => {
            tracing::info_span!("greentic.op.requested",
                greentic.op.id = %op_id,
                greentic.op.name = %op_name,
                greentic.tenant.id = %tenant,
                greentic.team.id = %team,
            )
            .in_scope(|| {
                tracing::info!("operation.requested");
            });
        }
        PayloadPolicy::HashOnly => {
            tracing::info_span!("greentic.op.requested",
                greentic.op.id = %op_id,
                greentic.op.name = %op_name,
                greentic.tenant.id = %tenant,
                greentic.team.id = %team,
                greentic.payload.size_bytes = payload_size,
            )
            .in_scope(|| {
                tracing::info!("operation.requested");
            });
        }
    }
}

/// Emit a structured "operation completed" event on the current tracing context.
///
/// When `payload_policy` is [`PayloadPolicy::None`], result size is omitted from the span.
/// When [`PayloadPolicy::HashOnly`], result size is included.
pub fn emit_operation_completed(
    config: &OperationSubsConfig,
    op_id: &str,
    op_name: &str,
    tenant: &str,
    team: &str,
    status: &str,
    result_size: usize,
) {
    if !config.enabled {
        return;
    }
    if !config.include_denied && status == "denied" {
        return;
    }
    if matches!(config.mode, SubsMode::MetricsOnly) {
        return;
    }
    match config.payload_policy {
        PayloadPolicy::None => {
            tracing::info_span!("greentic.op.completed",
                greentic.op.id = %op_id,
                greentic.op.name = %op_name,
                greentic.op.status = %status,
                greentic.tenant.id = %tenant,
                greentic.team.id = %team,
            )
            .in_scope(|| {
                tracing::info!("operation.completed status={status}");
            });
        }
        PayloadPolicy::HashOnly => {
            tracing::info_span!("greentic.op.completed",
                greentic.op.id = %op_id,
                greentic.op.name = %op_name,
                greentic.op.status = %status,
                greentic.tenant.id = %tenant,
                greentic.team.id = %team,
                greentic.result.size_bytes = result_size,
            )
            .in_scope(|| {
                tracing::info!("operation.completed status={status}");
            });
        }
    }
}

/// Create a root span for an operation. The caller enters/exits this span
/// to correlate all sub-events (requested, completed, component invocations).
pub fn operation_root_span(
    op_name: &str,
    provider_type: &str,
    tenant: &str,
    team: &str,
) -> tracing::Span {
    tracing::info_span!(
        "greentic.op",
        greentic.op.name = %op_name,
        "greentic.provider.type" = %provider_type,
        greentic.tenant.id = %tenant,
        greentic.team.id = %team,
        otel.status_code = tracing::field::Empty,
    )
}

// ---------------------------------------------------------------------------
// Operation metrics (counter + histogram)
// ---------------------------------------------------------------------------

#[cfg(feature = "otlp")]
mod metrics_impl {
    use once_cell::sync::Lazy;
    use opentelemetry::{KeyValue, global};

    static OP_DURATION: Lazy<opentelemetry::metrics::Histogram<f64>> = Lazy::new(|| {
        global::meter("greentic-telemetry")
            .f64_histogram("greentic.operation.duration_ms")
            .with_description("Operation end-to-end duration in milliseconds")
            .build()
    });

    static OP_COUNT: Lazy<opentelemetry::metrics::Counter<u64>> = Lazy::new(|| {
        global::meter("greentic-telemetry")
            .u64_counter("greentic.operation.count")
            .with_description("Total number of operations")
            .build()
    });

    pub fn record(op_name: &str, provider_type: &str, status: &str, duration_ms: f64) {
        let attrs = [
            KeyValue::new("greentic.op.name", op_name.to_string()),
            KeyValue::new("greentic.provider.type", provider_type.to_string()),
            KeyValue::new("greentic.op.status", status.to_string()),
        ];
        OP_DURATION.record(duration_ms, &attrs);
        OP_COUNT.add(1, &attrs);
    }
}

/// Record operation duration and count metrics (no-op when `otlp` feature is disabled).
#[cfg(feature = "otlp")]
pub fn record_operation_metric(op_name: &str, provider_type: &str, status: &str, duration_ms: f64) {
    metrics_impl::record(op_name, provider_type, status, duration_ms);
}

#[cfg(not(feature = "otlp"))]
pub fn record_operation_metric(_op_name: &str, _provider_type: &str, _status: &str, _duration_ms: f64) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_enables_everything() {
        let config = OperationSubsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.mode, SubsMode::MetricsAndTraces);
        assert!(config.include_denied);
        assert_eq!(config.payload_policy, PayloadPolicy::None);
    }

    #[test]
    fn subs_config_from_provider_defaults() {
        let provider = TelemetryProviderConfig::default();
        let subs = subs_config_from_provider(&provider);
        assert!(subs.enabled);
        assert_eq!(subs.mode, SubsMode::MetricsAndTraces);
        assert!(subs.include_denied);
    }

    #[test]
    fn subs_config_from_provider_custom() {
        let provider = TelemetryProviderConfig {
            enable_operation_subs: true,
            operation_subs_mode: Some("metrics_only".into()),
            include_denied_ops: false,
            payload_policy: Some("hash_only".into()),
            ..Default::default()
        };
        let subs = subs_config_from_provider(&provider);
        assert!(subs.enabled);
        assert_eq!(subs.mode, SubsMode::MetricsOnly);
        assert!(!subs.include_denied);
        assert_eq!(subs.payload_policy, PayloadPolicy::HashOnly);
    }

    #[test]
    fn subs_config_disabled() {
        let provider = TelemetryProviderConfig {
            enable_operation_subs: false,
            ..Default::default()
        };
        let subs = subs_config_from_provider(&provider);
        assert!(!subs.enabled);
    }

    #[test]
    fn emit_requested_noop_when_disabled() {
        let config = OperationSubsConfig {
            enabled: false,
            ..Default::default()
        };
        // Should not panic
        emit_operation_requested(&config, "op1", "send_payload", "tenant1", "team1", 100);
    }

    #[test]
    fn emit_completed_skips_denied_when_excluded() {
        let config = OperationSubsConfig {
            enabled: true,
            include_denied: false,
            ..Default::default()
        };
        // Should not panic; denied events are silently skipped
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "denied",
            0,
        );
    }

    #[test]
    fn emit_completed_allows_denied_when_included() {
        let config = OperationSubsConfig {
            enabled: true,
            include_denied: true,
            ..Default::default()
        };
        // Should not panic
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "denied",
            0,
        );
    }

    #[test]
    fn metrics_only_mode_skips_trace_events() {
        let config = OperationSubsConfig {
            enabled: true,
            mode: SubsMode::MetricsOnly,
            ..Default::default()
        };
        // Should not panic; no trace events emitted
        emit_operation_requested(&config, "op1", "send_payload", "tenant1", "team1", 100);
        emit_operation_completed(&config, "op1", "send_payload", "tenant1", "team1", "ok", 50);
    }

    #[test]
    fn traces_only_mode_emits_events() {
        let config = OperationSubsConfig {
            enabled: true,
            mode: SubsMode::TracesOnly,
            ..Default::default()
        };
        // TracesOnly should emit trace events (no panic)
        emit_operation_requested(&config, "op1", "send_payload", "tenant1", "team1", 100);
        emit_operation_completed(&config, "op1", "send_payload", "tenant1", "team1", "ok", 50);
    }

    #[test]
    fn payload_policy_none_omits_size() {
        let config = OperationSubsConfig {
            enabled: true,
            payload_policy: PayloadPolicy::None,
            ..Default::default()
        };
        // Should not panic; size fields are omitted from span
        emit_operation_requested(&config, "op1", "send_payload", "tenant1", "team1", 100);
        emit_operation_completed(&config, "op1", "send_payload", "tenant1", "team1", "ok", 50);
    }

    #[test]
    fn payload_policy_hash_only_includes_size() {
        let config = OperationSubsConfig {
            enabled: true,
            payload_policy: PayloadPolicy::HashOnly,
            ..Default::default()
        };
        // Should not panic; size fields are included in span
        emit_operation_requested(&config, "op1", "send_payload", "tenant1", "team1", 100);
        emit_operation_completed(&config, "op1", "send_payload", "tenant1", "team1", "ok", 50);
    }

    #[test]
    fn root_span_creates_without_panic() {
        // Without a subscriber installed, the span will be disabled — that's fine.
        // We just verify it can be created and entered without panicking.
        let span = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = span.enter();
    }
}
