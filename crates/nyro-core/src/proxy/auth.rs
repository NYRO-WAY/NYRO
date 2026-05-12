use axum::Json;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn bearer_auth(request: Request, next: Next) -> Response {
    let auth_key = request.extensions().get::<AuthKey>().cloned();

    let expected = match auth_key {
        Some(AuthKey(ref k)) if !k.is_empty() => k.clone(),
        _ => return next.run(request).await,
    };

    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = header.strip_prefix("Bearer ").unwrap_or("");

    if token != expected {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": {
                    "message": "Invalid API key",
                    "type": "NYRO_AUTH_ERROR",
                    "code": "invalid_api_key"
                }
            })),
        )
            .into_response();
    }

    next.run(request).await
}

#[derive(Clone, Debug)]
pub struct AuthKey(pub String);
