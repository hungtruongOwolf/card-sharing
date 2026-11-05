//! Outbound HTTPS client for identity providers (JWKS, Apple's token endpoint).
//!
//! hyper + rustls with the ring provider and the OS trust store (no aws-lc, matching the rest of
//! the workspace), HTTPS only, no redirects, bounded response size and short timeouts: a slow
//! provider must not hold request workers.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode, header};
use http_body_util::{BodyExt, Full, Limited};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;

#[derive(Debug, thiserror::Error)]
#[error("could not build the identity-provider HTTP client: {0}")]
pub struct HttpClientError(String);

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Whole request, including reading the body.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const USER_AGENT: &str = "card-sharing-api";

/// A provider response: status and the (size-limited) body.
#[derive(Debug)]
pub struct ProviderResponse {
    pub status: StatusCode,
    pub body: Bytes,
}

/// Cheap to clone; share one per process.
#[derive(Clone)]
pub struct ProviderHttp {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
}

/// The production client: HTTPS only.
pub fn provider_http_client() -> Result<ProviderHttp, HttpClientError> {
    build(true)
}

/// Same client, but plain `http://` is allowed. Only for tests against a local mock server.
pub fn insecure_test_http_client() -> Result<ProviderHttp, HttpClientError> {
    build(false)
}

fn build(https_only: bool) -> Result<ProviderHttp, HttpClientError> {
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    for err in &native.errors {
        tracing::warn!(error = %err, "could not load some OS root certificates");
    }
    let (added, ignored) = roots.add_parsable_certificates(native.certs);
    if added == 0 {
        tracing::warn!(
            ignored,
            "no OS root certificates loaded; HTTPS to identity providers will fail"
        );
    }
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|err| HttpClientError(err.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();

    let mut http = HttpConnector::new();
    http.enforce_http(false);
    http.set_connect_timeout(Some(CONNECT_TIMEOUT));
    let builder = HttpsConnectorBuilder::new().with_tls_config(tls);
    let connector = if https_only {
        builder.https_only()
    } else {
        builder.https_or_http()
    }
    .enable_http1()
    .wrap_connector(http);
    Ok(ProviderHttp {
        client: Client::builder(TokioExecutor::new()).build(connector),
    })
}

impl ProviderHttp {
    pub async fn get(&self, url: &str, limit: usize) -> Result<ProviderResponse, String> {
        let request = Request::builder()
            .method(Method::GET)
            .uri(url)
            .header(header::ACCEPT, "application/json")
            .header(header::USER_AGENT, USER_AGENT)
            .body(Full::new(Bytes::new()))
            .map_err(|err| err.to_string())?;
        self.send(request, limit).await
    }

    /// POST `application/x-www-form-urlencoded`.
    pub async fn post_form(
        &self,
        url: &str,
        form: &[(&str, &str)],
        limit: usize,
    ) -> Result<ProviderResponse, String> {
        let body = form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        let request = Request::builder()
            .method(Method::POST)
            .uri(url)
            .header(header::ACCEPT, "application/json")
            .header(header::USER_AGENT, USER_AGENT)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))
            .map_err(|err| err.to_string())?;
        self.send(request, limit).await
    }

    /// Sends a request; redirects are not followed (hyper never does).
    async fn send(
        &self,
        request: Request<Full<Bytes>>,
        limit: usize,
    ) -> Result<ProviderResponse, String> {
        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|err| err.to_string())?;
            let status = response.status();
            let body = Limited::new(response.into_body(), limit)
                .collect()
                .await
                .map_err(|err| format!("reading response: {err}"))?
                .to_bytes();
            Ok(ProviderResponse { status, body })
        };
        tokio::time::timeout(REQUEST_TIMEOUT, exchange)
            .await
            .map_err(|_| "provider request timed out".to_owned())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_production_client_refuses_plain_http() {
        let client = provider_http_client().unwrap();
        assert!(client.get("http://127.0.0.1:9/keys", 1024).await.is_err());
    }
}
