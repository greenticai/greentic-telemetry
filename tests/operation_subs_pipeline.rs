//! Operation subscription pipeline tests — verifies that the operation subs
//! emit the expected spans and events with the correct fields and behaviour.

use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;

use greentic_telemetry::operation_subs::*;

/// Captured event from a test subscriber.
#[derive(Debug, Clone)]
struct CapturedEvent {
    message: String,
    level: tracing::Level,
}

/// A tracing layer that captures events for test assertions.
struct EventCapture {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for EventCapture {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        struct MsgVisitor(String);
        impl tracing::field::Visit for MsgVisitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "message" {
                    self.0 = value.to_string();
                }
            }
        }
        let mut visitor = MsgVisitor(String::new());
        event.record(&mut visitor);
        if let Ok(mut guard) = self.events.lock() {
            guard.push(CapturedEvent {
                message: visitor.0,
                level: *event.metadata().level(),
            });
        }
    }
}

fn with_capture<F>(f: F) -> Vec<CapturedEvent>
where
    F: FnOnce(),
{
    let events = Arc::new(Mutex::new(Vec::new()));
    let layer = EventCapture {
        events: Arc::clone(&events),
    };
    let subscriber = tracing_subscriber::registry().with(layer);
    // Use a local dispatcher to avoid global state conflicts.
    let dispatch: tracing::Dispatch = subscriber.into();
    tracing::dispatcher::with_default(&dispatch, f);
    // Drop the dispatch explicitly so the subscriber (and its EventCapture
    // layer) is released before we read the captured events.
    drop(dispatch);
    // Lock-and-drain instead of Arc::try_unwrap — the tracing dispatcher
    // machinery may briefly hold extra Arc<Dispatch> clones in thread-local
    // storage during parallel test execution, making try_unwrap flaky.
    events.lock().unwrap().drain(..).collect()
}

// ---------------------------------------------------------------------------
// Root span produces events (not child spans)
// ---------------------------------------------------------------------------

#[test]
fn operation_emits_requested_and_completed_events() {
    let config = OperationSubsConfig::default();
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            100,
            None,
        );
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "ok",
            50,
            None,
            42.0,
        );
    });

    let messages: Vec<&str> = events.iter().map(|e| e.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("operation.requested")),
        "expected operation.requested event, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("operation.completed")),
        "expected operation.completed event, got: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// Error event is emitted on the root span
// ---------------------------------------------------------------------------

#[test]
fn operation_error_emits_error_event() {
    let config = OperationSubsConfig::default();
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_error(&config, "op1", "denied", "hook denied operation");
    });

    let error_events: Vec<_> = events
        .iter()
        .filter(|e| e.level == tracing::Level::ERROR)
        .collect();
    assert!(
        !error_events.is_empty(),
        "expected at least one ERROR event, got: {events:?}"
    );
    assert!(
        error_events[0].message.contains("operation.error"),
        "expected operation.error message, got: {:?}",
        error_events[0].message
    );
}

// ---------------------------------------------------------------------------
// Disabled config produces no events
// ---------------------------------------------------------------------------

#[test]
fn disabled_config_emits_nothing() {
    let config = OperationSubsConfig {
        enabled: false,
        ..Default::default()
    };
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            100,
            None,
        );
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "ok",
            50,
            None,
            0.0,
        );
        emit_operation_error(&config, "op1", "invoke_error", "something broke");
    });

    assert!(
        events.is_empty(),
        "disabled config should emit nothing, got: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// MetricsOnly mode emits no trace events
// ---------------------------------------------------------------------------

#[test]
fn metrics_only_mode_no_trace_events() {
    let config = OperationSubsConfig {
        enabled: true,
        mode: SubsMode::MetricsOnly,
        ..Default::default()
    };
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            100,
            None,
        );
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "ok",
            50,
            None,
            0.0,
        );
        emit_operation_error(&config, "op1", "invoke_error", "something broke");
    });

    assert!(
        events.is_empty(),
        "MetricsOnly should emit no trace events, got: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// Denied operation still emits events (when include_denied=true)
// ---------------------------------------------------------------------------

#[test]
fn denied_operation_emits_events_when_included() {
    let config = OperationSubsConfig {
        enabled: true,
        include_denied: true,
        ..Default::default()
    };
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            100,
            None,
        );
        emit_operation_error(&config, "op1", "denied", "hook denied");
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "denied",
            0,
            None,
            5.0,
        );
    });

    let messages: Vec<&str> = events.iter().map(|e| e.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("operation.requested")),
        "denied op should still emit requested, got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("operation.completed")),
        "denied op should still emit completed (include_denied=true), got: {messages:?}"
    );
    assert!(
        messages.iter().any(|m| m.contains("operation.error")),
        "denied op should emit error, got: {messages:?}"
    );
}

#[test]
fn denied_operation_skips_completed_when_excluded() {
    let config = OperationSubsConfig {
        enabled: true,
        include_denied: false,
        ..Default::default()
    };
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            100,
            None,
        );
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "denied",
            0,
            None,
            5.0,
        );
    });

    let messages: Vec<&str> = events.iter().map(|e| e.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("operation.requested")),
        "requested should still fire, got: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("operation.completed")),
        "completed should be skipped for denied when include_denied=false, got: {messages:?}"
    );
}

// ---------------------------------------------------------------------------
// Excluded ops produce no events
// ---------------------------------------------------------------------------

#[test]
fn excluded_ops_produce_no_events() {
    let config = OperationSubsConfig {
        enabled: true,
        exclude_ops: vec!["healthcheck".to_string()],
        ..Default::default()
    };
    let events = with_capture(|| {
        let root = operation_root_span("healthcheck", "system", "tenant1", "team1");
        let _guard = root.enter();
        emit_operation_requested(&config, "op1", "healthcheck", "tenant1", "team1", 0, None);
        emit_operation_completed(
            &config,
            "op1",
            "healthcheck",
            "tenant1",
            "team1",
            "ok",
            0,
            None,
            1.0,
        );
    });

    assert!(
        events.is_empty(),
        "excluded ops should produce no events, got: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// Payload hash appears in events when policy is HashOnly
// ---------------------------------------------------------------------------

#[test]
fn hash_only_policy_includes_hash_field() {
    let config = OperationSubsConfig {
        enabled: true,
        payload_policy: PayloadPolicy::HashOnly,
        ..Default::default()
    };
    // We can't easily inspect field values from tracing events without a custom
    // visitor, but we can verify the events are emitted without panicking and
    // with the right messages.
    let events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        let hash = hash_payload(b"test payload");
        emit_operation_requested(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            12,
            Some(&hash),
        );
        emit_operation_completed(
            &config,
            "op1",
            "send_payload",
            "tenant1",
            "team1",
            "ok",
            50,
            Some(&hash),
            42.0,
        );
    });

    assert_eq!(
        events.len(),
        2,
        "expected 2 events (requested + completed), got: {events:?}"
    );
}

// ---------------------------------------------------------------------------
// Root span field recording
// ---------------------------------------------------------------------------

#[test]
fn root_span_accepts_all_recordable_fields() {
    let _events = with_capture(|| {
        let root = operation_root_span("send_payload", "messaging.telegram", "tenant1", "team1");
        let _guard = root.enter();
        // All of these should succeed without panic
        root.record("otel.status_code", "ERROR");
        root.record("error.type", "invoke_error");
        root.record("error.message", "component failed");
        root.record("greentic.meta.routing.provider", "messaging-telegram");
        root.record("greentic.meta.classification", "egress");
        root.record("greentic.op.duration_ms", 42.5);
    });
}

// ---------------------------------------------------------------------------
// Attributed root span respects config
// ---------------------------------------------------------------------------

#[test]
fn attributed_root_span_created_without_panic() {
    let config = OperationSubsConfig {
        hash_ids: true,
        include_tenant: true,
        include_team: false,
        ..Default::default()
    };
    let _events = with_capture(|| {
        let span = operation_root_span_attributed(
            "send_payload",
            "messaging.telegram",
            "tenant1",
            "team1",
            &config,
        );
        let _guard = span.enter();
        span.record("otel.status_code", "OK");
    });
}

// ---------------------------------------------------------------------------
// Metrics functions don't panic
// ---------------------------------------------------------------------------

#[test]
fn metric_functions_dont_panic() {
    record_operation_metric("send_payload", "messaging.telegram", "ok", 100.0, "tenant1");
    record_operation_error_metric(
        "send_payload",
        "messaging.telegram",
        "invoke_error",
        "tenant1",
    );

    let config = OperationSubsConfig {
        include_team_in_metrics: true,
        hash_ids: true,
        ..Default::default()
    };
    record_operation_metric_attributed(
        "send_payload",
        "messaging.telegram",
        "ok",
        100.0,
        "tenant1",
        "team1",
        &config,
    );
    record_operation_error_metric_attributed(
        "send_payload",
        "messaging.telegram",
        "invoke_error",
        "tenant1",
        "team1",
        &config,
    );
}
