//! Pack validation tests — verifies that `validate_telemetry_config()`
//! catches malformed telemetry offers and configuration issues.

use greentic_telemetry::provider::{
    TelemetryProviderConfig, TenantAttribution, TlsConfig, validate_telemetry_config,
};

// ---------------------------------------------------------------------------
// Malformed telemetry offer rejected
// ---------------------------------------------------------------------------

#[test]
fn malformed_export_mode_rejected() {
    let config = TelemetryProviderConfig {
        export_mode: "kafka-stream".into(),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("unknown export_mode")),
        "expected warning about unknown export mode, got: {warnings:?}"
    );
}

#[test]
fn malformed_subs_mode_rejected() {
    let config = TelemetryProviderConfig {
        operation_subs_mode: Some("all_events".into()),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("unknown operation_subs_mode")),
        "expected warning about unknown subs mode, got: {warnings:?}"
    );
}

#[test]
fn malformed_payload_policy_rejected() {
    let config = TelemetryProviderConfig {
        payload_policy: Some("full_body_dump".into()),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("unknown payload_policy")),
        "expected warning about unknown payload policy, got: {warnings:?}"
    );
}

#[test]
fn malformed_compression_rejected() {
    let config = TelemetryProviderConfig {
        compression: Some("brotli".into()),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("unknown compression")),
        "expected warning about unknown compression, got: {warnings:?}"
    );
}

#[test]
fn malformed_log_level_rejected() {
    let config = TelemetryProviderConfig {
        min_log_level: Some("verbose".into()),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("unknown min_log_level")),
        "expected warning about unknown log level, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Missing endpoint rejected (unless stdout/none or preset provides one)
// ---------------------------------------------------------------------------

#[test]
fn missing_endpoint_rejected_for_otlp_grpc() {
    let config = TelemetryProviderConfig {
        export_mode: "otlp-grpc".into(),
        endpoint: None,
        preset: None,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("requires an endpoint")),
        "expected endpoint-required warning, got: {warnings:?}"
    );
}

#[test]
fn missing_endpoint_rejected_for_otlp_http() {
    let config = TelemetryProviderConfig {
        export_mode: "otlp-http".into(),
        endpoint: None,
        preset: None,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("requires an endpoint")),
        "expected endpoint-required warning, got: {warnings:?}"
    );
}

#[test]
fn missing_endpoint_ok_for_json_stdout() {
    let config = TelemetryProviderConfig {
        export_mode: "json-stdout".into(),
        endpoint: None,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        !warnings.iter().any(|w| w.contains("requires an endpoint")),
        "json-stdout should not require endpoint, got: {warnings:?}"
    );
}

#[test]
fn missing_endpoint_ok_for_none() {
    let config = TelemetryProviderConfig {
        export_mode: "none".into(),
        endpoint: None,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        !warnings.iter().any(|w| w.contains("requires an endpoint")),
        "none mode should not require endpoint, got: {warnings:?}"
    );
}

#[test]
fn missing_endpoint_ok_when_preset_provides_one() {
    let config = TelemetryProviderConfig {
        export_mode: "otlp-grpc".into(),
        endpoint: None,
        preset: Some("jaeger".into()),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        !warnings.iter().any(|w| w.contains("requires an endpoint")),
        "preset should suppress endpoint warning, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Sensitive headers flagged
// ---------------------------------------------------------------------------

#[test]
fn sensitive_header_authorization_flagged() {
    let config = TelemetryProviderConfig {
        headers: {
            let mut h = std::collections::HashMap::new();
            h.insert("Authorization".into(), "Bearer xyz".into());
            h
        },
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("credentials")),
        "expected credentials warning, got: {warnings:?}"
    );
}

#[test]
fn non_sensitive_header_ok() {
    let config = TelemetryProviderConfig {
        headers: {
            let mut h = std::collections::HashMap::new();
            h.insert("x-correlation-id".into(), "abc".into());
            h
        },
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Sampling validation
// ---------------------------------------------------------------------------

#[test]
fn negative_sampling_ratio_warns() {
    let config = TelemetryProviderConfig {
        sampling_ratio: -0.1,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(warnings.iter().any(|w| w.contains("sampling_ratio")));
}

#[test]
fn over_one_sampling_ratio_warns() {
    let config = TelemetryProviderConfig {
        sampling_ratio: 1.5,
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(warnings.iter().any(|w| w.contains("sampling_ratio")));
}

// ---------------------------------------------------------------------------
// TLS validation
// ---------------------------------------------------------------------------

#[test]
fn tls_cert_without_key_warns() {
    let config = TelemetryProviderConfig {
        tls_config: Some(TlsConfig {
            ca_cert_pem: None,
            client_cert_pem: Some("cert".into()),
            client_key_pem: None,
        }),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("client_cert_pem without client_key_pem"))
    );
}

#[test]
fn tls_key_without_cert_warns() {
    let config = TelemetryProviderConfig {
        tls_config: Some(TlsConfig {
            ca_cert_pem: None,
            client_cert_pem: None,
            client_key_pem: Some("key".into()),
        }),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("client_cert_pem without client_key_pem"))
    );
}

#[test]
fn tls_complete_no_warnings() {
    let config = TelemetryProviderConfig {
        tls_config: Some(TlsConfig {
            ca_cert_pem: Some("ca".into()),
            client_cert_pem: Some("cert".into()),
            client_key_pem: Some("key".into()),
        }),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Redaction patterns
// ---------------------------------------------------------------------------

#[test]
fn empty_redaction_pattern_entry_warns() {
    let config = TelemetryProviderConfig {
        redaction_patterns: vec!["\\d+".into(), "".into(), "\\w+".into()],
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(warnings.iter().any(|w| w.contains("empty entry")));
}

// ---------------------------------------------------------------------------
// Tenant attribution validation
// ---------------------------------------------------------------------------

#[test]
fn hash_ids_without_includes_warns() {
    let config = TelemetryProviderConfig {
        tenant_attribution: Some(TenantAttribution {
            include_tenant: false,
            include_team: false,
            include_team_in_metrics: false,
            hash_ids: true,
        }),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.iter().any(|w| w.contains("hash_ids")),
        "expected hash_ids warning, got: {warnings:?}"
    );
}

#[test]
fn hash_ids_with_includes_no_warning() {
    let config = TelemetryProviderConfig {
        tenant_attribution: Some(TenantAttribution {
            include_tenant: true,
            include_team: false,
            include_team_in_metrics: false,
            hash_ids: true,
        }),
        ..Default::default()
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        !warnings.iter().any(|w| w.contains("hash_ids")),
        "unexpected hash_ids warning, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Default config passes all validation
// ---------------------------------------------------------------------------

#[test]
fn default_config_zero_warnings() {
    let config = TelemetryProviderConfig::default();
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.is_empty(),
        "default config should have zero warnings, got: {warnings:?}"
    );
}

// ---------------------------------------------------------------------------
// Comprehensive "good" config
// ---------------------------------------------------------------------------

#[test]
fn comprehensive_valid_config_zero_warnings() {
    let config = TelemetryProviderConfig {
        export_mode: "otlp-grpc".into(),
        endpoint: Some("http://collector:4317".into()),
        headers: {
            let mut h = std::collections::HashMap::new();
            h.insert("x-custom".into(), "val".into());
            h
        },
        sampling_ratio: 0.5,
        compression: Some("gzip".into()),
        service_name: Some("test-service".into()),
        preset: None,
        enable_operation_subs: true,
        operation_subs_mode: Some("metrics_and_traces".into()),
        include_denied_ops: true,
        payload_policy: Some("hash_only".into()),
        min_log_level: Some("info".into()),
        tls_config: None,
        exclude_ops: vec!["ping".into()],
        drop_payloads: false,
        resource_attributes: Default::default(),
        redaction_patterns: vec!["\\d{3}-\\d{2}-\\d{4}".into()],
        tenant_attribution: Some(TenantAttribution {
            include_tenant: true,
            include_team: true,
            include_team_in_metrics: false,
            hash_ids: false,
        }),
    };
    let warnings = validate_telemetry_config(&config);
    assert!(
        warnings.is_empty(),
        "valid config should have zero warnings, got: {warnings:?}"
    );
}
