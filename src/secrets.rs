use tracing::{Level, Span, event, span};

pub const SECRETS_OP: &str = "secrets.op";
pub const SECRETS_KEY: &str = "secrets.key";
pub const SECRETS_SCOPE_ENV: &str = "secrets.scope.env";
pub const SECRETS_SCOPE_TENANT: &str = "secrets.scope.tenant";
pub const SECRETS_SCOPE_TEAM: &str = "secrets.scope.team";
pub const SECRETS_RESULT: &str = "secrets.result";
pub const SECRETS_ERROR_KIND: &str = "secrets.error_kind";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretOp {
    Get,
    Put,
    Delete,
    List,
}

impl SecretOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretOp::Get => "get",
            SecretOp::Put => "put",
            SecretOp::Delete => "delete",
            SecretOp::List => "list",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretResult {
    Ok,
    NotFound,
    Denied,
    Invalid,
    Error,
}

impl SecretResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecretResult::Ok => "ok",
            SecretResult::NotFound => "not_found",
            SecretResult::Denied => "denied",
            SecretResult::Invalid => "invalid",
            SecretResult::Error => "error",
        }
    }
}

/// Record the standard secret attributes onto the current span.
pub fn record_secret_attrs(
    op: SecretOp,
    key: impl AsRef<str>,
    env: impl AsRef<str>,
    tenant: impl AsRef<str>,
    team: Option<impl AsRef<str>>,
    result: SecretResult,
    error_kind: Option<impl AsRef<str>>,
) {
    record_secret_attrs_on(
        &Span::current(),
        op,
        key,
        env,
        tenant,
        team,
        result,
        error_kind,
    );
}

/// Record the standard secret attributes onto a provided span.
#[allow(clippy::too_many_arguments)]
pub fn record_secret_attrs_on(
    span: &Span,
    op: SecretOp,
    key: impl AsRef<str>,
    env: impl AsRef<str>,
    tenant: impl AsRef<str>,
    team: Option<impl AsRef<str>>,
    result: SecretResult,
    error_kind: Option<impl AsRef<str>>,
) {
    let team_ref = team.as_ref().map(|t| t.as_ref());
    let error_kind_ref = error_kind.as_ref().map(|e| e.as_ref());
    span.record(SECRETS_OP, op.as_str());
    span.record(SECRETS_KEY, key.as_ref());
    span.record(SECRETS_SCOPE_ENV, env.as_ref());
    span.record(SECRETS_SCOPE_TENANT, tenant.as_ref());
    if let Some(team) = team_ref {
        span.record(SECRETS_SCOPE_TEAM, team);
    }
    span.record(SECRETS_RESULT, result.as_str());
    if let Some(error_kind) = error_kind_ref {
        span.record(SECRETS_ERROR_KIND, error_kind);
    }

    event!(
        target: "greentic.secrets",
        Level::INFO,
        secrets.op = op.as_str(),
        secrets.key = key.as_ref(),
        secrets.scope.env = env.as_ref(),
        secrets.scope.tenant = tenant.as_ref(),
        secrets.scope.team = team_ref,
        secrets.result = result.as_str(),
        secrets.error_kind = error_kind_ref,
        "secret operation attributes"
    );
}

/// Create a span pre-populated with secret attributes (excluding result/error).
pub fn secret_span(
    op: SecretOp,
    key: impl AsRef<str>,
    env: impl AsRef<str>,
    tenant: impl AsRef<str>,
    team: Option<impl AsRef<str>>,
) -> Span {
    let team_val = team.as_ref().map(|t| t.as_ref());
    span!(
        Level::INFO,
        "secret",
        secrets.op = op.as_str(),
        secrets.key = key.as_ref(),
        secrets.scope.env = env.as_ref(),
        secrets.scope.tenant = tenant.as_ref(),
        secrets.scope.team = team_val
    )
}
