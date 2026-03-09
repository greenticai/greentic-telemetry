//! SigV4-signing HTTP client for direct AWS X-Ray OTLP export.
//!
//! Wraps `reqwest::Client` and signs every outgoing request with AWS SigV4
//! before sending, enabling direct OTLP/HTTP to the X-Ray endpoint without
//! an intermediate ADOT collector.

use std::fmt;
use std::time::SystemTime;

use async_trait::async_trait;
use aws_credential_types::provider::{ProvideCredentials, SharedCredentialsProvider};
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use aws_sigv4::sign::v4;
use bytes::Bytes;
use http::{Request, Response};

type HttpError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Clone)]
pub(crate) struct SigV4HttpClient {
    inner: reqwest::Client,
    credentials_provider: SharedCredentialsProvider,
    region: String,
}

impl fmt::Debug for SigV4HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SigV4HttpClient")
            .field("region", &self.region)
            .finish()
    }
}

impl SigV4HttpClient {
    pub(crate) fn new(
        credentials_provider: SharedCredentialsProvider,
        region: String,
    ) -> Self {
        Self {
            inner: reqwest::Client::new(),
            credentials_provider,
            region,
        }
    }
}

#[async_trait]
impl opentelemetry_http::HttpClient for SigV4HttpClient {
    async fn send_bytes(
        &self,
        request: Request<Bytes>,
    ) -> Result<Response<Bytes>, HttpError> {
        // Resolve current credentials from the provider chain
        let creds = self
            .credentials_provider
            .provide_credentials()
            .await
            .map_err(|e| format!("AWS credentials resolution failed: {e}"))?;

        let (parts, body) = request.into_parts();

        // Build SigV4 signing parameters
        let identity = aws_smithy_runtime_api::client::identity::Identity::from(creds);
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(&self.region)
            .name("xray")
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|e| format!("SigV4 signing params failed: {e}"))?;

        // Create signable request from headers
        let headers_vec: Vec<(&str, &str)> = parts
            .headers
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str(), v)))
            .collect();

        let signable = SignableRequest::new(
            parts.method.as_str(),
            parts.uri.to_string(),
            headers_vec.into_iter(),
            SignableBody::Bytes(&body),
        )
        .map_err(|e| format!("SigV4 signable request failed: {e}"))?;

        // Sign the request
        let (instructions, _signature) = sign(signable, &signing_params.into())
            .map_err(|e| format!("SigV4 signing failed: {e}"))?
            .into_parts();

        // Apply signing headers to the request
        let mut signed_request = Request::from_parts(parts, body);
        instructions.apply_to_request_http1x(&mut signed_request);

        // Convert to reqwest and send
        let reqwest_request: reqwest::Request = signed_request
            .map(|b| b.to_vec())
            .try_into()
            .map_err(|e: reqwest::Error| format!("Failed to convert to reqwest request: {e}"))?;

        let response = self
            .inner
            .execute(reqwest_request)
            .await
            .map_err(|e| format!("HTTP request failed: {e}"))?;

        // Convert response back to http::Response
        let status = response.status();
        let resp_headers = response.headers().clone();
        let resp_body = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read response body: {e}"))?;

        if !status.is_success() {
            let body_str = String::from_utf8_lossy(&resp_body);
            eprintln!(
                "AWS X-Ray export response: status={} body={}",
                status,
                &body_str[..body_str.len().min(500)]
            );
        } else {
            eprintln!("AWS X-Ray export: OK (status={})", status);
        }

        let mut builder = Response::builder().status(status);
        for (k, v) in &resp_headers {
            builder = builder.header(k, v);
        }
        builder
            .body(resp_body)
            .map_err(|e| format!("Failed to build response: {e}").into())
    }
}
