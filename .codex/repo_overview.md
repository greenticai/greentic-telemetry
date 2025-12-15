# Repository Overview

## 1. High-Level Purpose
- Rust crate providing tenant-aware telemetry utilities for Greentic services built on `tracing` and OpenTelemetry. Supplies task-local context injection, OTLP wiring, and a lightweight client that can emit spans/metrics via OTLP or structured JSON.
- Secondary pieces cover WASM bridging and environment-driven export presets; some legacy/experimental modules are present but not wired into the public API.

## 2. Main Components and Functionality
- **Path:** `src/context.rs`, `src/tasklocal.rs`, `src/layer.rs`  
  **Role:** Core telemetry context and tracing integration.  
  **Key functionality:** `TelemetryCtx` holds `{tenant, session, flow, node, provider}`; task-local storage for the context; tracing layer (`layer_from_task_local`/`layer_with_provider`) that copies the context into span extensions and records the fields on span entry.
- **Path:** `src/init.rs`  
  **Role:** Telemetry initialization and shutdown.  
  **Key functionality:** `init_telemetry` installs tracing subscribers with optional fmt/dev/prod-json layers and OTLP exporters when `OTEL_EXPORTER_OTLP_ENDPOINT` is set (otlp feature); `init_telemetry_auto` derives export mode/endpoints/headers/compression/sampling from env/preset config; new `init_telemetry_from_config` treats a provided `ExportConfig` as authoritative (no env merging) to align with greentic-config; PII redaction is initialised from env; global shutdown helpers.
- **Path:** `src/client.rs`, `src/host_bridge.rs`  
  **Role:** Lightweight client for emitting telemetry independently of the main initialization path.  
  **Key functionality:** `client::init` sets up OTLP exporters (spans + metrics) when an endpoint is provided, otherwise JSON stdout logging; helpers to emit one-off spans (`span`), metrics (`metric`), and pin a trace id (`set_trace_id`); `host_bridge::emit_span` parses host-provided JSON spans and augments them with standard labels before forwarding to the client.
- **Path:** `src/testutil.rs`, `tests/`, `examples/`  
  **Role:** Testing and examples.  
  **Key functionality:** `CaptureLayer`/`span_recorder` capture closed spans for assertions; tests cover task-local context injection, OTLP initialization smoke tests, and emitting telemetry markers; examples show basic OTLP setup and task-local context usage; `examples/collector` provides a sample OTEL collector compose file.
- **Path:** `src/wasm_guest.rs`, `src/wasm_host.rs`, `wit/greentic-telemetry.wit`  
  **Role:** WASM guest/host telemetry bridge.  
  **Key functionality:** Guest-side logging/span APIs delegating to generated WIT bindings when on wasm32 (fallbacks to stdout otherwise); host-side span/event forwarding with simple span stack management for guest spans; WIT definitions expose logging and span lifecycle functions.
- **Path:** `src/export.rs`, `src/presets/`  
  **Role:** Env/preset export configuration helpers now public.  
  **Key functionality:** Env-driven export configuration inferred from env vars or cloud presets (AWS/GCP/Azure/Datadog/Loki) with sampling/header parsing and gzip compression selection; used by `init_telemetry_auto`.
- **Path:** `src/redaction.rs`, `src/init.rs`  
  **Role:** Global redaction and init wiring.  
  **Key functionality:** Configurable redaction modes plus hard-coded secret redaction applied to fmt output and OTLP exporters (sensitive key names masked, bearer tokens rewritten); OTLP exporters are wrapped to scrub span/event attributes before sending.
- **Path:** `src/secrets.rs`  
  **Role:** Secrets telemetry contract helpers.  
  **Key functionality:** Constants for `secrets.*` attribute keys, enums for `SecretOp`/`SecretResult`, helper to record secret attrs on the current span and emit a structured event, and a `secret_span` constructor to avoid stringly-typed usage.
- **Path:** `dev/docker-compose.elastic.yml`, `examples/collector/`  
  **Role:** Dev telemetry stacks.  
  **Key functionality:** Compose files to spin up Elastic/Kibana/OpenTelemetry Collector for local inspection of spans/logs.

## 3. Work In Progress, TODOs, and Stubs
- **Location:** `src/export.rs:4-103` and `src/presets/*` — Status: partial. Auto-config drives OTLP gRPC/HTTP with headers, sampling (parent/ratio/always-on/off), and gzip compression; other compression algorithms and richer docs remain optional follow-ups.
- **Location:** `src/redaction.rs` — Status: partial. Heuristic PII + secret masking improved and now applied globally, but deeper pattern coverage/configurability remains optional follow-up.
- **Location:** `src/wasm_guest.rs` / `src/wasm_host.rs` — Status: exposed and documented in README; integration examples and end-to-end coverage still to be added if WASM consumers are expected.
- **Location:** OTLP entrypoints (`src/init.rs` `init_telemetry`/`init_telemetry_auto`, `src/client.rs`) — Status: consolidated; `init_otlp` removed in favour of `init_telemetry_auto`.

## 4. Broken, Failing, or Conflicting Areas
- None observed; `cargo test --all-features -- --nocapture` and `ci/local_check.sh` pass (cloud smokes skipped offline).

## 5. Notes for Future Work
- Optionally expand auto-config (more compression algorithms, clearer sampling semantics) and strengthen PII redaction with deeper matching and a documented downstream API.
- Add integration examples/docs for the WASM bridge to validate guest/host spans in real use.
