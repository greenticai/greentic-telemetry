//! Guest-side telemetry shim.
//!
//! With the `wit-guest` feature on a `wasm32` target, `log` / `span_start` /
//! `span_end` forward to the host through the `greentic:telemetry/logging` WIT
//! import (see `wit/greentic-telemetry.wit`). Otherwise — native builds, or
//! `wasm32` without `wit-guest` — they fall back to stdout, so this module is
//! always usable for local development.

#[derive(Clone, Copy, Debug)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct Field<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

pub fn log(level: Level, message: &str, fields: &[Field<'_>]) {
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        host::log(level, message, fields);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        fallback_log(level, message, fields);
    }
}

pub fn span_start(name: &str, fields: &[Field<'_>]) -> u64 {
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        return host::span_start(name, fields);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        fallback_log(Level::Debug, &format!("span-start: {name}"), fields);
        0
    }
}

pub fn span_end(id: u64) {
    #[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
    {
        host::span_end(id);
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
    {
        let _ = id; // silence unused warnings
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wit-guest"))]
mod host {
    use super::{Field, Level};

    wit_bindgen::generate!({
        path: "wit",
        world: "guest-telemetry",
    });

    use greentic::telemetry::logging::{self as wit, Fields, Level as WitLevel};

    fn to_wit_level(level: Level) -> WitLevel {
        match level {
            Level::Trace => WitLevel::Trace,
            Level::Debug => WitLevel::Debug,
            Level::Info => WitLevel::Info,
            Level::Warn => WitLevel::Warn,
            Level::Error => WitLevel::Error,
        }
    }

    fn to_wit_fields(fields: &[Field<'_>]) -> Fields {
        Fields {
            entries: fields
                .iter()
                .map(|f| (f.key.to_string(), f.value.to_string()))
                .collect(),
        }
    }

    pub fn log(level: Level, message: &str, fields: &[Field<'_>]) {
        wit::log(to_wit_level(level), message, &to_wit_fields(fields));
    }

    pub fn span_start(name: &str, fields: &[Field<'_>]) -> u64 {
        wit::span_start(name, &to_wit_fields(fields))
    }

    pub fn span_end(id: u64) {
        wit::span_end(id);
    }
}

#[cfg(not(all(target_arch = "wasm32", feature = "wit-guest")))]
fn fallback_log(level: Level, message: &str, fields: &[Field<'_>]) {
    let lvl = match level {
        Level::Trace => "TRACE",
        Level::Debug => "DEBUG",
        Level::Info => "INFO",
        Level::Warn => "WARN",
        Level::Error => "ERROR",
    };

    if fields.is_empty() {
        println!("[{lvl}] {message}");
    } else {
        let serialized = fields
            .iter()
            .map(|f| format!("{}={}", f.key, f.value))
            .collect::<Vec<_>>()
            .join(", ");
        println!("[{lvl}] {message} [{serialized}]");
    }
}
