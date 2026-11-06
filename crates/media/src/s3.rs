//! S3-compatible object storage adapter (Cloudflare R2 in production, ADR-0008; MinIO locally and
//! in CI). AWS Signature Version 4 is implemented here over the workspace's hyper + rustls (ring)
//! stack instead of pulling an SDK: the adapter only needs `PutObject`, `GetObject` and
//! `DeleteObject` on three private buckets with path-style URLs.
//!
//! Security notes:
//! - Credentials come from the environment (secret manager) and never appear in logs or errors;
//!   errors carry the operation, the HTTP status and the S3 error code only (no URL, bucket or key).
//! - Payloads are signed (`x-amz-content-sha256` is the body's SHA-256), so a proxy cannot swap them.
//! - HTTPS only, unless the caller explicitly allows `http://` (local MinIO, never production).
//! - Responses are size-limited and every request has a deadline.
//!
//! Bucket-level controls are infrastructure, not code: the quarantine bucket has a 48 h lifecycle
//! rule and the legal-hold bucket is created with object lock (see ADR-0008).

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode, Uri, header};
use http_body_util::{BodyExt, Full, Limited};
use hyper_rustls::{HttpsConnector, HttpsConnectorBuilder};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use platform::clock::SharedClock;
use platform::config::Secret;
use ring::hmac;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset};

use crate::store::{BoxFuture, Bucket, ObjectKey, ObjectStore, StoreError};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Whole exchange, including the body (uploads are at most a few MiB).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Error bodies are tiny XML documents; read at most this much of them.
const ERROR_BODY_LIMIT: usize = 4 * 1024;
/// Default cap on a downloaded object; larger objects are an error, not a truncation.
pub const DEFAULT_MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
const SERVICE: &str = "s3";
const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Physical bucket names for the three logical buckets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Buckets {
    pub quarantine: String,
    pub clean: String,
    pub legal_hold: String,
}

impl S3Buckets {
    fn name(&self, bucket: Bucket) -> &str {
        match bucket {
            Bucket::Quarantine => &self.quarantine,
            Bucket::Clean => &self.clean,
            Bucket::LegalHold => &self.legal_hold,
        }
    }
}

/// Settings for [`S3ObjectStore`]. Build them from the environment; [`S3Settings::validate`]
/// runs at startup so a bad value fails fast.
#[derive(Debug, Clone)]
pub struct S3Settings {
    /// `scheme://host[:port]` with no path, query or user info, e.g.
    /// `https://<account>.r2.cloudflarestorage.com`.
    pub endpoint: String,
    /// Signing region (`auto` for R2, `us-east-1` for MinIO).
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: Secret,
    pub buckets: S3Buckets,
    /// Allow a plain `http://` endpoint (local MinIO only; refused in production by the caller).
    pub allow_http: bool,
    pub max_object_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct S3ConfigError(String);

/// A validated endpoint: scheme and authority (the `Host` header value).
#[derive(Debug, Clone)]
struct Endpoint {
    scheme: &'static str,
    authority: String,
}

impl S3Settings {
    pub fn validate(&self) -> Result<(), S3ConfigError> {
        self.endpoint()?;
        let region_ok = !self.region.is_empty()
            && self.region.len() <= 32
            && self
                .region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !region_ok {
            return Err(S3ConfigError(
                "region must be lowercase letters, digits and hyphens".to_owned(),
            ));
        }
        let key_ok =
            |v: &str| !v.is_empty() && v.len() <= 256 && v.bytes().all(|b| b.is_ascii_graphic());
        if !key_ok(&self.access_key_id) || !key_ok(self.secret_access_key.expose()) {
            return Err(S3ConfigError(
                "access key id and secret must be non-empty printable ASCII".to_owned(),
            ));
        }
        for name in [
            &self.buckets.quarantine,
            &self.buckets.clean,
            &self.buckets.legal_hold,
        ] {
            if !valid_bucket_name(name) {
                return Err(S3ConfigError(format!(
                    "bucket name {name:?} must be 3-63 lowercase letters, digits or hyphens"
                )));
            }
        }
        let b = &self.buckets;
        if b.quarantine == b.clean || b.quarantine == b.legal_hold || b.clean == b.legal_hold {
            return Err(S3ConfigError(
                "quarantine, clean and legal-hold must be different buckets".to_owned(),
            ));
        }
        if self.max_object_bytes == 0 {
            return Err(S3ConfigError("max object size must be positive".to_owned()));
        }
        Ok(())
    }

    fn endpoint(&self) -> Result<Endpoint, S3ConfigError> {
        let bad = |why: &str| S3ConfigError(format!("endpoint {why}"));
        let uri: Uri = self
            .endpoint
            .parse()
            .map_err(|_| bad("is not a valid URL"))?;
        let scheme = match uri.scheme_str() {
            Some("https") => "https",
            Some("http") if self.allow_http => "http",
            Some("http") => return Err(bad("must use https")),
            _ => return Err(bad("must be an https URL")),
        };
        let authority = uri.authority().ok_or_else(|| bad("has no host"))?;
        if authority.as_str().contains('@') {
            return Err(bad("must not contain credentials"));
        }
        let path_ok = matches!(uri.path(), "" | "/");
        if !path_ok || uri.query().is_some() {
            return Err(bad("must not have a path or query"));
        }
        Ok(Endpoint {
            scheme,
            authority: authority.as_str().to_owned(),
        })
    }
}

/// S3 bucket naming rules restricted to the safe subset: no dots (no virtual-host TLS issues).
fn valid_bucket_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (3..=63).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
}

/// The S3-compatible [`ObjectStore`].
pub struct S3ObjectStore {
    endpoint: Endpoint,
    signer: SigV4,
    buckets: S3Buckets,
    max_object_bytes: usize,
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    clock: SharedClock,
}

impl S3ObjectStore {
    pub fn new(settings: S3Settings, clock: SharedClock) -> Result<Self, S3ConfigError> {
        settings.validate()?;
        let endpoint = settings.endpoint()?;
        Ok(Self {
            client: http_client(settings.allow_http)?,
            signer: SigV4 {
                access_key_id: settings.access_key_id,
                secret_access_key: settings.secret_access_key,
                region: settings.region,
            },
            endpoint,
            buckets: settings.buckets,
            max_object_bytes: settings.max_object_bytes,
            clock,
        })
    }

    /// Sends one signed request to `/<bucket>[/<key>]` and returns the status and the body
    /// (successful bodies up to `limit`, error bodies up to [`ERROR_BODY_LIMIT`]).
    async fn send(
        &self,
        method: Method,
        path: &str,
        extra_headers: &[(&str, &str)],
        body: Bytes,
        limit: usize,
    ) -> Result<(StatusCode, Bytes), StoreError> {
        let payload_hash = hex(&Sha256::digest(&body));
        let now = self.clock.now();
        let mut headers: Vec<(String, String)> = vec![
            ("host".to_owned(), self.endpoint.authority.clone()),
            ("x-amz-content-sha256".to_owned(), payload_hash.clone()),
            ("x-amz-date".to_owned(), amz_date(now)),
        ];
        for (name, value) in extra_headers {
            headers.push(((*name).to_ascii_lowercase(), (*value).trim().to_owned()));
        }
        let authorization =
            self.signer
                .authorization(method.as_str(), path, "", &headers, &payload_hash, now);
        let mut builder = Request::builder()
            .method(method.clone())
            .uri(format!(
                "{}://{}{}",
                self.endpoint.scheme,
                self.endpoint.authority,
                uri_encode_path(path)
            ))
            .header(header::AUTHORIZATION, authorization);
        for (name, value) in headers.iter().filter(|(n, _)| n != "host") {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let request = builder
            .body(Full::new(body))
            .map_err(|_| op_error(&method, "could not build the request"))?;

        let exchange = async {
            let response = self
                .client
                .request(request)
                .await
                .map_err(|_| op_error(&method, "connection failed"))?;
            let status = response.status();
            let cap = if status.is_success() {
                limit
            } else {
                ERROR_BODY_LIMIT
            };
            let body = Limited::new(response.into_body(), cap)
                .collect()
                .await
                .map_err(|_| op_error(&method, "response body unreadable or too large"))?
                .to_bytes();
            Ok((status, body))
        };
        tokio::time::timeout(REQUEST_TIMEOUT, exchange)
            .await
            .map_err(|_| op_error(&method, "timed out"))?
    }

    fn object_path(&self, bucket: Bucket, key: &ObjectKey) -> String {
        format!("/{}/{}", self.buckets.name(bucket), key.as_str())
    }

    /// Creates `bucket` (idempotent). For tests and local setup; production buckets are
    /// provisioned by infrastructure with their lifecycle and object-lock rules.
    pub async fn create_bucket(&self, bucket: Bucket) -> Result<(), StoreError> {
        let path = format!("/{}", self.buckets.name(bucket));
        let (status, body) = self
            .send(Method::PUT, &path, &[], Bytes::new(), ERROR_BODY_LIMIT)
            .await?;
        let code = error_code(&body);
        if status.is_success()
            || matches!(
                code.as_deref(),
                Some("BucketAlreadyOwnedByYou" | "BucketAlreadyExists")
            )
        {
            Ok(())
        } else {
            Err(status_error(&Method::PUT, status, code))
        }
    }
}

impl ObjectStore for S3ObjectStore {
    fn put<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
        bytes: Bytes,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            let path = self.object_path(bucket, key);
            let (status, body) = self
                .send(
                    Method::PUT,
                    &path,
                    &[("content-type", "application/octet-stream")],
                    bytes,
                    ERROR_BODY_LIMIT,
                )
                .await?;
            if status.is_success() {
                Ok(())
            } else {
                Err(status_error(&Method::PUT, status, error_code(&body)))
            }
        })
    }

    fn get<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<Option<Bytes>, StoreError>> {
        Box::pin(async move {
            let path = self.object_path(bucket, key);
            let (status, body) = self
                .send(Method::GET, &path, &[], Bytes::new(), self.max_object_bytes)
                .await?;
            match status {
                s if s.is_success() => Ok(Some(body)),
                StatusCode::NOT_FOUND => Ok(None),
                s => Err(status_error(&Method::GET, s, error_code(&body))),
            }
        })
    }

    fn delete<'a>(
        &'a self,
        bucket: Bucket,
        key: &'a ObjectKey,
    ) -> BoxFuture<'a, Result<(), StoreError>> {
        Box::pin(async move {
            let path = self.object_path(bucket, key);
            let (status, body) = self
                .send(Method::DELETE, &path, &[], Bytes::new(), ERROR_BODY_LIMIT)
                .await?;
            // S3 answers 204 for missing keys too; some compatible stores answer 404.
            if status.is_success() || status == StatusCode::NOT_FOUND {
                Ok(())
            } else {
                Err(status_error(&Method::DELETE, status, error_code(&body)))
            }
        })
    }
}

fn op_error(method: &Method, what: &str) -> StoreError {
    StoreError::Io(format!("s3 {method}: {what}"))
}

fn status_error(method: &Method, status: StatusCode, code: Option<String>) -> StoreError {
    StoreError::Io(format!(
        "s3 {method} answered {} ({})",
        status.as_u16(),
        code.as_deref().unwrap_or("no error code")
    ))
}

/// The `<Code>` of an S3 XML error body, if it is a plain identifier (it never echoes keys).
fn error_code(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let start = text.find("<Code>")? + "<Code>".len();
    let len = text[start..].find("</Code>")?;
    let code = &text[start..start + len];
    (!code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| code.to_owned())
}

fn http_client(
    allow_http: bool,
) -> Result<Client<HttpsConnector<HttpConnector>, Full<Bytes>>, S3ConfigError> {
    let mut roots = rustls::RootCertStore::empty();
    let native = rustls_native_certs::load_native_certs();
    let (added, _) = roots.add_parsable_certificates(native.certs);
    if added == 0 {
        tracing::warn!("no OS root certificates loaded; HTTPS to object storage will fail");
    }
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|err| S3ConfigError(err.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut http = HttpConnector::new();
    http.enforce_http(false);
    http.set_connect_timeout(Some(CONNECT_TIMEOUT));
    let builder = HttpsConnectorBuilder::new().with_tls_config(tls);
    let connector = if allow_http {
        builder.https_or_http()
    } else {
        builder.https_only()
    }
    .enable_http1()
    .wrap_connector(http);
    Ok(Client::builder(TokioExecutor::new()).build(connector))
}

/// AWS Signature Version 4 for the `s3` service.
struct SigV4 {
    access_key_id: String,
    secret_access_key: Secret,
    region: String,
}

impl SigV4 {
    /// The `Authorization` header for a request. `headers` are lowercase names with trimmed values
    /// and must include `host`, `x-amz-date` and `x-amz-content-sha256`; all are signed.
    fn authorization(
        &self,
        method: &str,
        path: &str,
        canonical_query: &str,
        headers: &[(String, String)],
        payload_hash: &str,
        now: OffsetDateTime,
    ) -> String {
        let mut sorted: Vec<&(String, String)> = headers.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let canonical_headers: String = sorted
            .iter()
            .map(|(name, value)| format!("{name}:{value}\n"))
            .collect();
        let signed_headers = sorted
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let canonical_request = format!(
            "{method}\n{}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
            uri_encode_path(path)
        );
        let date_time = amz_date(now);
        let date = &date_time[..8];
        let scope = format!("{date}/{}/{SERVICE}/aws4_request", self.region);
        let string_to_sign = format!(
            "{ALGORITHM}\n{date_time}\n{scope}\n{}",
            hex(&Sha256::digest(canonical_request.as_bytes()))
        );
        let secret = format!("AWS4{}", self.secret_access_key.expose());
        let mut key = hmac_sha256(secret.as_bytes(), date.as_bytes());
        for part in [self.region.as_bytes(), SERVICE.as_bytes(), b"aws4_request"] {
            key = hmac_sha256(&key, part);
        }
        let signature = hex(&hmac_sha256(&key, string_to_sign.as_bytes()));
        format!(
            "{ALGORITHM} Credential={}/{scope}, SignedHeaders={signed_headers}, Signature={signature}",
            self.access_key_id
        )
    }
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&key, message).as_ref().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `YYYYMMDDTHHMMSSZ` in UTC.
fn amz_date(now: OffsetDateTime) -> String {
    let t = now.to_offset(UtcOffset::UTC);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

/// SigV4 URI encoding of a path: unreserved characters and `/` stay, every other byte is `%XX`.
fn uri_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use time::macros::datetime;

    // AWS documentation example credentials (public test values), split so secret scanners do not
    // mistake them for real ones.
    fn example_signer() -> SigV4 {
        SigV4 {
            access_key_id: ["AKIA", "IOSFODNN7EXAMPLE"].concat(),
            secret_access_key: Secret::new(
                ["wJalrXUtnFEMI/K7MDENG/", "bPxRfiCYEXAMPLEKEY"].concat(),
            ),
            region: "us-east-1".to_owned(),
        }
    }

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// AWS S3 SigV4 documentation, "Example: GET Object".
    #[test]
    fn signs_the_aws_get_object_example() {
        let auth = example_signer().authorization(
            "GET",
            "/test.txt",
            "",
            &headers(&[
                ("host", "examplebucket.s3.amazonaws.com"),
                ("range", "bytes=0-9"),
                ("x-amz-content-sha256", EMPTY_SHA256),
                ("x-amz-date", "20130524T000000Z"),
            ]),
            EMPTY_SHA256,
            datetime!(2013-05-24 00:00:00 UTC),
        );
        assert!(auth.ends_with(
            "/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        ));
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIA"));
    }

    /// AWS S3 SigV4 documentation, "Example: PUT Object" (path needs percent-encoding).
    #[test]
    fn signs_the_aws_put_object_example() {
        let payload_hash = hex(&Sha256::digest(b"Welcome to Amazon S3."));
        assert_eq!(
            payload_hash,
            "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
        );
        let auth = example_signer().authorization(
            "PUT",
            "/test$file.text",
            "",
            &headers(&[
                ("date", "Fri, 24 May 2013 00:00:00 GMT"),
                ("host", "examplebucket.s3.amazonaws.com"),
                ("x-amz-content-sha256", &payload_hash),
                ("x-amz-date", "20130524T000000Z"),
                ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ]),
            &payload_hash,
            datetime!(2013-05-24 00:00:00 UTC),
        );
        assert!(auth.ends_with(
            "Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
        ));
    }

    #[test]
    fn header_order_does_not_change_the_signature() {
        let signer = example_signer();
        let now = datetime!(2013-05-24 00:00:00 UTC);
        let a = headers(&[
            ("host", "h"),
            ("x-amz-content-sha256", EMPTY_SHA256),
            ("x-amz-date", "20130524T000000Z"),
        ]);
        let mut b = a.clone();
        b.reverse();
        assert_eq!(
            signer.authorization("GET", "/b/k", "", &a, EMPTY_SHA256, now),
            signer.authorization("GET", "/b/k", "", &b, EMPTY_SHA256, now)
        );
    }

    #[test]
    fn formats_dates_and_paths() {
        assert_eq!(
            amz_date(datetime!(2026-11-06 01:02:03 +07:00)),
            "20261105T180203Z"
        );
        assert_eq!(uri_encode_path("/a b/c$~.-_"), "/a%20b/c%24~.-_");
    }

    fn settings() -> S3Settings {
        S3Settings {
            endpoint: "https://account.r2.cloudflarestorage.com".to_owned(),
            region: "auto".to_owned(),
            access_key_id: "id".to_owned(),
            secret_access_key: Secret::new("secret"),
            buckets: S3Buckets {
                quarantine: "cs-quarantine".to_owned(),
                clean: "cs-clean".to_owned(),
                legal_hold: "cs-legal-hold".to_owned(),
            },
            allow_http: false,
            max_object_bytes: DEFAULT_MAX_OBJECT_BYTES,
        }
    }

    #[test]
    fn validates_settings() {
        assert_eq!(settings().validate(), Ok(()));
        let with = |f: fn(&mut S3Settings)| {
            let mut s = settings();
            f(&mut s);
            s.validate()
        };
        assert!(with(|s| s.endpoint = "http://127.0.0.1:9000".to_owned()).is_err());
        assert!(
            with(|s| {
                s.endpoint = "http://127.0.0.1:9000".to_owned();
                s.allow_http = true;
            })
            .is_ok()
        );
        assert!(with(|s| s.endpoint = "https://host/path".to_owned()).is_err());
        assert!(with(|s| s.endpoint = "https://u:p@host".to_owned()).is_err());
        assert!(with(|s| s.endpoint = "https://host?x=1".to_owned()).is_err());
        assert!(with(|s| s.endpoint = "ftp://host".to_owned()).is_err());
        assert!(with(|s| s.region = "US East".to_owned()).is_err());
        assert!(with(|s| s.access_key_id = String::new()).is_err());
        assert!(with(|s| s.buckets.clean = "Bad_Name".to_owned()).is_err());
        assert!(with(|s| s.buckets.clean = "a.b.c".to_owned()).is_err());
        assert!(with(|s| s.buckets.clean = "cs-quarantine".to_owned()).is_err());
        assert!(with(|s| s.buckets.clean = "-cs".to_owned()).is_err());
        assert!(with(|s| s.max_object_bytes = 0).is_err());
    }

    #[test]
    fn error_codes_are_plain_identifiers_only() {
        assert_eq!(
            error_code(b"<Error><Code>NoSuchBucket</Code><Key>abc</Key></Error>").as_deref(),
            Some("NoSuchBucket")
        );
        assert_eq!(error_code(b"<Error><Code>a/b</Code></Error>"), None);
        assert_eq!(error_code(b"not xml"), None);
    }

    #[test]
    fn settings_debug_redacts_the_secret() {
        let debug = format!("{:?}", settings());
        assert!(!debug.contains("\"secret\""), "{debug}");
    }
}
