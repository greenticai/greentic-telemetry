# SEMVER Fix Report

## Context
- Crate: `greentic-telemetry`
- Version: `0.4.3`
- Input failures: `constructible_struct_adds_field`, `enum_no_repr_variant_discriminant_changed`, `enum_variant_added`

## Fixes Applied

### 1) `constructible_struct_adds_field`
Issue:
- `PresetConfig` added `sampling_ratio`
- `ExportConfig` added `resource_attributes` and `tls_config`

Fix:
- Added `#[non_exhaustive]` to:
  - `PresetConfig` in `src/presets/mod.rs`
  - `ExportConfig` in `src/export.rs`

Rationale:
- Prevents external struct-literal construction from being semver-sensitive to added public fields.
- No runtime behavior changes.

### 2) `enum_no_repr_variant_discriminant_changed`
Issue:
- `CloudPreset::None` discriminant changed from `5` to `14`.

Fix:
- Added explicit discriminants to `CloudPreset` in `src/presets/mod.rs` to preserve old value stability for existing variants.
- Preserved `CloudPreset::None = 5` explicitly.

Assigned values:
- `Aws = 0`
- `Gcp = 1`
- `Azure = 2`
- `Datadog = 3`
- `Loki = 4`
- `None = 5`
- `Honeycomb = 6`
- `NewRelic = 7`
- `Elastic = 8`
- `GrafanaTempo = 9`
- `Jaeger = 10`
- `Zipkin = 11`
- `OtlpGrpc = 12`
- `OtlpHttp = 13`
- `Stdout = 14`

Rationale:
- Stabilizes numeric casts for previously existing variants.
- No logic changes.

### 3) `enum_variant_added`
Issue:
- New variants were added to exhaustive public enums:
  - `ExportMode`
  - `CloudPreset`

Fix:
- Added `#[non_exhaustive]` to:
  - `ExportMode` in `src/export.rs`
  - `CloudPreset` in `src/presets/mod.rs`

Rationale:
- Makes future variant additions semver-safe for downstream exhaustive matching.
- No runtime behavior changes.

## Match Wildcard Catch-All Review
- Reviewed internal matches on `CloudPreset`/`ExportMode`.
- No wildcard changes were required for this crate’s internal compilation model; behavior was left unchanged.

## Verification
- `cargo check` passed.
- `cargo semver-checks` could not be rerun in this CI sandbox due registry lock path permissions:
  - `attempted to take an exclusive lock on a read-only path` (`/home/runner/.cargo/.package-cache`)

## Files Changed
- `src/export.rs`
- `src/presets/mod.rs`
- `SEMVER_FIX_REPORT.md`
