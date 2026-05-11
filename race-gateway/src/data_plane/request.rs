use axum::http::HeaderMap;
use bytes::Bytes;

use crate::domain::DownstreamRouteKind;

#[derive(Debug, Clone)]
pub struct ProxyRouteRequest {
    pub group_id: String,
    pub route_kind: DownstreamRouteKind,
    pub model_action: Option<String>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub diagnostics_enabled: bool,
}

impl ProxyRouteRequest {
    pub fn new(
        group_id: String,
        route_kind: DownstreamRouteKind,
        model_action: Option<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Self {
        let diagnostics_enabled = headers
            .get("x-nyro-race-diagnostics")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value.eq_ignore_ascii_case("1") || value.eq_ignore_ascii_case("true")
            });

        Self {
            group_id,
            route_kind,
            model_action,
            headers,
            body,
            diagnostics_enabled,
        }
    }
}
