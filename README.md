# greentic-telemetry

Tenant-aware telemetry utilities for Greentic services built on top of [`tracing`] and [`opentelemetry`].

[`tracing`]: https://github.com/tokio-rs/tracing
[`opentelemetry`]: https://opentelemetry.io/

## Highlights

- `TelemetryCtx`: lightweight context carrying `{tenant, session, flow, node, provider}`.
- `layer_from_task_local`: grab the context from a Tokio task-local without wiring closures.
- `CtxLayer` (`layer_with`): legacy closure-based path kept for backwards compatibility.
- `init_telemetry_auto`: env/preset-driven setup (OTLP gRPC/HTTP with headers/compression/sampling) or stdout fallback.
- Utilities for integration testing (`testutil::span_recorder`) and task-local helpers.

## Quickstart (auto-config)

```rust
use greentic_telemetry::{TelemetryConfig, init_telemetry_auto};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Configure via env:
    // TELEMETRY_EXPORT=json-stdout|otlp-grpc|otlp-http
    // OTLP_ENDPOINT=http://localhost:4317 (gRPC) or http://localhost:4318 (HTTP)
    // OTLP_HEADERS=authorization=Bearer%20abc (comma-separated, url-decoded)
    // TELEMETRY_SAMPLING=parent|traceidratio:0.5|always_on|always_off
    // OTLP_COMPRESSION=gzip
    init_telemetry_auto(TelemetryConfig {
        service_name: "greentic-telemetry".into(),
    })?;

    tracing::info!("Hello from auto-configured telemetry");
    greentic_telemetry::shutdown();
    Ok(())
}
```

## OTLP wiring

`init_telemetry_auto` installs a `tracing` subscriber composed of:

- `tracing_opentelemetry::layer` connected to an OTLP exporter (gRPC or HTTP, based on env)
- Optional gzip compression, headers, and sampling wired from env/preset config
- `service.name` populated from `TelemetryConfig`

The subscriber becomes the global default; use `opentelemetry::global::shutdown_tracer_provider()` during graceful shutdown to flush spans. The legacy `init_otlp` path has been removed; use `init_telemetry_auto`.

## Secrets attribute contract (telemetry)

- Attribute keys (never store secret values): `secrets.op`, `secrets.key`, `secrets.scope.env`, `secrets.scope.tenant`, `secrets.scope.team` (optional), `secrets.result`, `secrets.error_kind` (optional, structured like `host_error`, `io`, `policy`, `serde`).
- Allowed values:
  - `secrets.op`: `get | put | delete | list`
  - `secrets.result`: `ok | not_found | denied | invalid | error`
  - `secrets.key`: the logical secret key (string), never bytes.
- Redaction is global for logs and OTLP export: any fields named like `secret`, `token`, `api_key`, `authorization`, `password`, `client_secret`, `access_token`, `refresh_token`, `bearer`, `x-api-key` (case-insensitive) are masked. Bearer tokens under auth-ish keys are replaced with `Bearer [REDACTED]`. No sizes/hashes/previews are emitted.
- Helper to avoid stringly-typed attrs:

```rust
use greentic_telemetry::secrets::{record_secret_attrs, SecretOp, SecretResult, secret_span};

let span = secret_span(SecretOp::Get, "db/password", "dev", "tenant-a", None);
let _enter = span.enter();
record_secret_attrs(
    SecretOp::Get,
    "db/password",
    "dev",
    "tenant-a",
    None::<&str>,
    SecretResult::Ok,
    None::<&str>,
);
```

## WASM guest/host bridge

- Guest side (`wasm32`): use `greentic_telemetry::wasm_guest::{log, span_start, span_end}` to emit logs/spans; falls back to stdout when not on wasm.
- Host side: use `greentic_telemetry::wasm_host::{log, span_start, span_end}` to forward guest events into the native tracing pipeline; spans/events are tagged with `runtime=wasm`.
- Minimal host integration example:

```rust
use greentic_telemetry::{TelemetryConfig, init_telemetry_auto};

fn main() -> anyhow::Result<()> {
    init_telemetry_auto(TelemetryConfig { service_name: "wasm-host".into() })?;
    // forward guest events using wasm_host::{log, span_start, span_end}
    Ok(())
}
```

See `src/wasm_guest.rs`, `src/wasm_host.rs`, and `wit/greentic-telemetry.wit` for details.

## Upgrading from legacy `init_otlp`

- Replace calls to `init_otlp` with `init_telemetry_auto(TelemetryConfig { service_name })`.
- Set export behaviour via env: `TELEMETRY_EXPORT`, `OTLP_ENDPOINT`, `OTLP_HEADERS`, `TELEMETRY_SAMPLING`, `OTLP_COMPRESSION`.
- If you previously layered `layer_from_task_local`, continue to do so when building your subscriber or pass it in `init_telemetry` as an extra layer.
- Remove any direct dependencies on `OtlpConfig`/`TelemetryError`; these types are no longer exported.

## Testing utilities

`testutil::span_recorder()` returns a `(CaptureLayer, Arc<Mutex<Vec<RecordedSpan>>>)` pair for asserting that spans carry `TelemetryCtx`. See `tests/context_propagation.rs` for an end-to-end example exercising propagation across nested spans.

## Dev Elastic bundle

A ready-to-run Elastic/Kibana/OpenTelemetry Collector stack lives in `dev/elastic-compose/`.

```bash
docker compose -f dev/elastic-compose/docker-compose.yml up -d
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

Then open Kibana at <http://localhost:5601/>. The default collector config writes spans/metrics to stdout for quick inspection—customise `otel-config.yaml` if you want to forward to Elastic APM.

The existing `dev/docker-compose.elastic.yml` + Filebeat setup remains available if you need the legacy log ingestion pipeline.

## Verification

This crate must pass:

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

## Local CI checks

Run `ci/local_check.sh` before pushing to mirror the GitHub Actions matrix locally. The script is offline by default; opt in to extra checks via:

- `LOCAL_CHECK_ONLINE=1` — run networked steps (cargo publish dry-run, cloud telemetry loops, schema curls).
- `LOCAL_CHECK_STRICT=1` — treat skipped steps as failures and require every optional tool/env to be present.
- `LOCAL_CHECK_VERBOSE=1` — echo each command for easier debugging.

The generated `.git/hooks/pre-push` hook invokes the script automatically; remove or edit it if you prefer to run the checks manually.
