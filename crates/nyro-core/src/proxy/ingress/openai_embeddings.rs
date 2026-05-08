//! Thin ingress shell: POST /v1/embeddings

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde_json::Value;

use crate::protocol::ids::OPENAI_EMBEDDINGS_V1;
use crate::protocol::ir::{AiRequest, RawEnvelope};
use crate::proxy::context::RequestContext;
use crate::proxy::dispatcher::{dispatch_pipeline, error_response};
use crate::Gateway;

pub async fn openai_embeddings(
    State(gw): State<Gateway>,
    mut ctx: axum::extract::Extension<RequestContext>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    ctx.ingress_protocol = OPENAI_EMBEDDINGS_V1;
    let flat_headers: std::collections::HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|vs| (k.as_str().to_lowercase(), vs.to_string())))
        .collect();
    let envelope = RawEnvelope::new(Some(body.clone()), flat_headers, "POST", "/v1/embeddings");
    let decoder = OPENAI_EMBEDDINGS_V1.handler().make_decoder();
    let internal = match decoder.decode_request(body) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid request: {e}")),
    };
    let request: AiRequest = internal.into();
    dispatch_pipeline(gw, headers, envelope, request, OPENAI_EMBEDDINGS_V1).await
}
