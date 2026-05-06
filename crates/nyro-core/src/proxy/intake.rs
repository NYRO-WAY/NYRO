//! Request intake layer.
//!
//! Responsibilities (single-concern):
//! - Extract and validate the raw request body.
//! - Extract the `model` field from the body.
//! - Serialize headers for logging.
//! - Provide the `request_id` from `RequestContext`.
//!
//! This module has NO knowledge of auth, routing, or upstream calls.

use axum::http::HeaderMap;
use serde_json::Value;

use crate::error::GatewayError;
use crate::proxy::context::RequestContext;
use crate::proxy::observability::headers_to_json;

/// Result of the intake phase.
pub struct IntakeResult {
    /// The parsed request body.
    pub body: Value,
    /// The `model` field extracted from the body (trimmed, non-empty).
    pub model: String,
    /// Serialized headers for logging.
    pub request_headers_str: Option<String>,
    /// Serialized body for logging.
    pub request_body_str: Option<String>,
}

/// Parse and validate a standard chat / responses / messages / generate
/// ingress body.
///
/// Returns `Err(GatewayError)` if the body is missing the `model` field.
pub fn intake_body(
    headers: &HeaderMap,
    body: Value,
) -> Result<IntakeResult, GatewayError> {
    let request_headers_str = headers_to_json(headers);
    let request_body_str = serde_json::to_string(&body).ok();

    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| GatewayError::bad_request("model_required", "model is required"))?;

    Ok(IntakeResult {
        body,
        model,
        request_headers_str,
        request_body_str,
    })
}

/// Extract the `model` from a body, returning an error if absent.
///
/// Used by ingress paths that perform their own decoding before calling the
/// pipeline (e.g. embeddings and Gemini which need a pre-decode step).
pub fn extract_model(body: &Value) -> Result<String, GatewayError> {
    body.get("model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| GatewayError::bad_request("model_required", "model is required"))
}

/// Stamp the request_id header on an outbound response for client correlation.
pub fn stamp_request_id(
    response: &mut axum::response::Response,
    ctx: &RequestContext,
) {
    if let Ok(value) = axum::http::HeaderValue::from_str(&ctx.request_id) {
        response
            .headers_mut()
            .insert("x-request-id", value);
    }
}
