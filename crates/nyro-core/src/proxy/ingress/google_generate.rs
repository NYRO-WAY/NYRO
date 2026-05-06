//! Thin ingress shell: POST /v1beta/models/:model_action

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde_json::Value;

use crate::protocol::codec::google::decoder::GoogleDecoder;
use crate::protocol::ids::GOOGLE_GENERATE_V1BETA;
use crate::proxy::context::RequestContext;
use crate::proxy::dispatcher::{dispatch_pipeline, error_response};
use crate::proxy::observability::headers_to_json;
use crate::Gateway;

pub async fn google_generate(
    State(gw): State<Gateway>,
    mut ctx: axum::extract::Extension<RequestContext>,
    headers: HeaderMap,
    Path(model_action): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    ctx.ingress_protocol = GOOGLE_GENERATE_V1BETA;
    let (model, action) = match model_action.rsplit_once(':') {
        Some((m, a)) => (m.to_string(), a.to_string()),
        None => (model_action.clone(), "generateContent".to_string()),
    };
    let is_stream = action == "streamGenerateContent";
    let path = format!("/v1beta/models/{model_action}");
    let request_headers_str = headers_to_json(&headers);
    let request_body_str = serde_json::to_string(&body).ok();
    let internal = match GoogleDecoder.decode_with_model(body, &model, is_stream) {
        Ok(r) => r,
        Err(e) => return error_response(400, &format!("invalid Gemini request: {e}")),
    };
    dispatch_pipeline(gw, headers, internal, GOOGLE_GENERATE_V1BETA, "POST", &path, request_headers_str, request_body_str).await
}
