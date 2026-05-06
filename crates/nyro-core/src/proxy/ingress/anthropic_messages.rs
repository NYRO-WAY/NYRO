//! Thin ingress shell: POST /v1/messages

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde_json::Value;

use crate::protocol::ids::ANTHROPIC_MESSAGES_2023_06_01;
use crate::proxy::context::RequestContext;
use crate::proxy::dispatcher::{dispatch_pipeline, error_response};
use crate::proxy::observability::headers_to_json;
use crate::Gateway;

pub async fn anthropic_messages(
    State(gw): State<Gateway>,
    mut ctx: axum::extract::Extension<RequestContext>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    ctx.ingress_protocol = ANTHROPIC_MESSAGES_2023_06_01;
    let request_headers_str = headers_to_json(&headers);
    let request_body_str = serde_json::to_string(&body).ok();
    let decoder = ANTHROPIC_MESSAGES_2023_06_01.handler().make_decoder();
    let internal = match decoder.decode_request(body) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid request: {e}")),
    };
    dispatch_pipeline(gw, headers, internal, ANTHROPIC_MESSAGES_2023_06_01, "POST", "/v1/messages", request_headers_str, request_body_str).await
}
