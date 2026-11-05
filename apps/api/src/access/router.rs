//! Route registration that cannot forget a policy: the only way to add a documented route is
//! [`ApiRouter::route`], which takes the policy together with the handler.

use http::Method;
use utoipa::openapi::OpenApi;
use utoipa::openapi::extensions::ExtensionsBuilder;
use utoipa::openapi::path::{Operation, PathItem};
use utoipa_axum::router::{OpenApiRouter, UtoipaMethodRouter};

use super::policy::{Idempotency, PolicyTable, RoutePolicy};

pub struct ApiRouter<S: Clone + Send + Sync + 'static> {
    inner: OpenApiRouter<S>,
    policies: PolicyTable,
}

impl<S: Clone + Send + Sync + 'static> ApiRouter<S> {
    pub fn new(openapi: OpenApi) -> Self {
        Self {
            inner: OpenApiRouter::with_openapi(openapi),
            policies: PolicyTable::default(),
        }
    }

    /// Registers the operations produced by `utoipa_axum::routes!(handler)` under `policy`.
    /// The policy is also published in the OpenAPI document as `x-policy`.
    pub fn route(
        mut self,
        policy: RoutePolicy,
        (schemas, mut paths, handler): UtoipaMethodRouter<S>,
    ) -> Self {
        for (path, item) in paths.paths.iter_mut() {
            for (method, operation) in operations_mut(item) {
                self.policies.insert(&method, path, policy);
                annotate(operation, &policy);
            }
        }
        self.inner = self.inner.routes((schemas, paths, handler));
        self
    }

    /// Adds a policy for a route that is served outside the OpenAPI document (for example
    /// `/openapi.json` itself).
    pub fn undocumented_policy(mut self, method: &Method, path: &str, policy: RoutePolicy) -> Self {
        self.policies.insert(method, path, policy);
        self
    }

    pub fn finish(self) -> (axum::Router<S>, OpenApi, PolicyTable) {
        let (router, openapi) = self.inner.split_for_parts();
        (router, openapi, self.policies)
    }
}

fn operations_mut(item: &mut PathItem) -> Vec<(Method, &mut Operation)> {
    [
        (Method::GET, item.get.as_mut()),
        (Method::PUT, item.put.as_mut()),
        (Method::POST, item.post.as_mut()),
        (Method::DELETE, item.delete.as_mut()),
        (Method::OPTIONS, item.options.as_mut()),
        (Method::HEAD, item.head.as_mut()),
        (Method::PATCH, item.patch.as_mut()),
        (Method::TRACE, item.trace.as_mut()),
    ]
    .into_iter()
    .filter_map(|(method, operation)| operation.map(|op| (method, op)))
    .collect()
}

/// Lists every `(method, path)` operation in an OpenAPI document.
pub fn documented_operations(openapi: &OpenApi) -> Vec<(Method, String)> {
    let mut openapi = openapi.clone();
    let mut out = Vec::new();
    for (path, item) in openapi.paths.paths.iter_mut() {
        for (method, _) in operations_mut(item) {
            out.push((method, path.clone()));
        }
    }
    out
}

fn annotate(operation: &mut Operation, policy: &RoutePolicy) {
    // One extension key: `Extensions` is a HashMap, so several keys would serialize in a
    // random order and make the committed OpenAPI document drift.
    let mut value = serde_json::json!({
        "auth": policy.auth.as_str(),
        "role": policy.role.as_str(),
        "attestation": policy.attestation,
        "rateLimit": policy.rate_limit,
        "versionGate": policy.version_gate,
    });
    // Published only where it applies, so routes without it keep their documented shape.
    if policy.idempotency != Idempotency::None
        && let Some(map) = value.as_object_mut()
    {
        map.insert(
            "idempotency".to_owned(),
            serde_json::Value::from(policy.idempotency.as_str()),
        );
    }
    let extensions = ExtensionsBuilder::new().add("x-policy", value).build();
    match operation.extensions.as_mut() {
        Some(existing) => existing.merge(extensions),
        None => operation.extensions = Some(extensions),
    }
}
