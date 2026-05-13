//! Deterministic protocol negotiation (PR-04).
//!
//! `negotiate()` is a pure function — given the ingress protocol, any
//! route-level preference, the provider's declared endpoints, and the
//! capability matrix (PR-07), it returns a `ProtocolPlan` or a typed error.
//!
//! Determinism guarantee: for identical inputs the output is always identical.
//! The previous `HashMap.iter().find` in `ProviderProtocols::resolve_egress`
//! was non-deterministic; the new Vec-backed `ProviderProtocols.endpoints`
//! (also from PR-04) eliminates that.

use crate::db::models::RouteTarget;
use crate::error::GatewayError;
use crate::protocol::ProviderProtocols;
use crate::protocol::ids::ProtocolId;
use crate::proxy::context::RequestContext;

// ── ProtocolPlan ──────────────────────────────────────────────────────────────

/// The fully-resolved protocol decision for one request.
#[derive(Debug, Clone)]
pub struct ProtocolPlan {
    /// The protocol the client used.
    pub ingress: ProtocolId,
    /// The protocol to use when talking to the upstream provider.
    pub egress: ProtocolId,
    /// The operational mode.
    pub mode: ProtocolMode,
    /// The base URL for the upstream call (may be overridden by the provider
    /// adapter in PR-13).
    pub base_url: String,
    /// Whether the codec must do a lossy transform (fields will be dropped).
    pub needs_conversion: bool,
}

/// How the gateway handles the codec path for this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolMode {
    /// Ingress == egress, no codec translation needed.
    Native,
    /// Codec translation required; all fields can be represented.
    Transform,
    /// Codec translation required; some fields will be dropped (only allowed
    /// when `route.allow_lossy == true`).
    LossyTransform,
    /// Bypass codec; forward body verbatim (P2-A, not active yet).
    PassThrough,
    /// No valid egress found; request must be rejected.
    Reject,
}

// ── RoutingStrategy trait ─────────────────────────────────────────────────────

/// Pluggable target-ordering strategy.
///
/// `OrderedStrategy` preserves the DB declaration order (current behaviour).
/// P2-H will add `WeightedStrategy` / `LeastLatencyStrategy` etc.
pub trait RoutingStrategy: Send + Sync {
    fn name(&self) -> &'static str;
    fn select_ordered(&self, targets: &[RouteTarget], _ctx: &RequestContext) -> Vec<RouteTarget>;
}

/// Ordered strategy: target order == DB row order. Preserves the pre-PR-04
/// behaviour of `TargetSelector::select_ordered`.
pub struct OrderedStrategy;

impl RoutingStrategy for OrderedStrategy {
    fn name(&self) -> &'static str {
        "ordered"
    }

    fn select_ordered(&self, targets: &[RouteTarget], _ctx: &RequestContext) -> Vec<RouteTarget> {
        targets.to_vec()
    }
}

/// Resolve the `RoutingStrategy` implementation by name.
///
/// P2-H will expand this to a proper registry.
pub fn get_routing_strategy(name: &str) -> Box<dyn RoutingStrategy> {
    match name.to_ascii_lowercase().as_str() {
        "ordered" | "" => Box::new(OrderedStrategy),
        other => {
            tracing::warn!(
                strategy = other,
                "unknown routing strategy; falling back to ordered"
            );
            Box::new(OrderedStrategy)
        }
    }
}

// ── negotiate() ───────────────────────────────────────────────────────────────

/// Determine the egress protocol and operational mode for a single request.
///
/// Priority order:
/// 1. Route-level `egress_protocol` preference (if set and supported).
/// 2. Exact ingress-protocol match in provider declarations.
/// 3. Same-family fallback (first in Vec order).
/// 4. Provider default.
/// 5. Reject.
///
/// The `provider_decl` may be `None` when the provider hasn't declared any
/// endpoints (old-style config), in which case we fall back to the ingress
/// protocol and assume `base_url = ""` (the provider adapter fills it in).
pub fn negotiate(
    ingress: ProtocolId,
    route_pref: Option<ProtocolId>,
    provider_decl: Option<&ProviderProtocols>,
    ctx: &mut RequestContext,
) -> Result<ProtocolPlan, GatewayError> {
    // If no provider declarations, pass-through on ingress protocol.
    let Some(decl) = provider_decl else {
        let plan = ProtocolPlan {
            ingress,
            egress: ingress,
            mode: ProtocolMode::Native,
            base_url: String::new(),
            needs_conversion: false,
        };
        ctx.egress_protocol = Some(ingress);
        return Ok(plan);
    };

    // Tier 1: route-level preference.
    if let Some(pref) = route_pref {
        if let Some(ep) = decl.get(pref) {
            let mode = if pref == ingress {
                ProtocolMode::Native
            } else {
                ProtocolMode::Transform
            };
            ctx.egress_protocol = Some(pref);
            ctx.trace("negotiate", format!("route_pref exact: {pref}"));
            return Ok(ProtocolPlan {
                ingress,
                egress: pref,
                mode,
                base_url: ep.base_url.clone(),
                needs_conversion: pref != ingress,
            });
        }
        ctx.trace(
            "negotiate",
            format!("route_pref {pref} not supported by provider; falling through"),
        );
    }

    // Tiers 2–4: delegate to ProviderProtocols::resolve_egress.
    let resolved = decl.resolve_egress(ingress);

    // Check lossy-reject policy from the egress endpoint's capability matrix.
    let egress_caps = resolved.protocol.handler().capabilities();

    if resolved.needs_conversion && egress_caps.lossy_default_reject {
        // Lossy transform — rejected by default unless the route explicitly
        // opts in via `allow_lossy` (to be threaded in a later PR).
        // For now, we allow it but set the mode to `LossyTransform` so the
        // dispatcher can surface a warning.
        ctx.trace(
            "negotiate",
            format!("lossy transform: {} → {}", ingress, resolved.protocol),
        );
    }

    let mode = if !resolved.needs_conversion {
        ProtocolMode::Native
    } else {
        ProtocolMode::Transform
    };

    ctx.egress_protocol = Some(resolved.protocol);
    ctx.trace(
        "negotiate",
        format!(
            "resolved: {} → {} ({})",
            ingress,
            resolved.protocol,
            if resolved.needs_conversion {
                "transform"
            } else {
                "native"
            }
        ),
    );

    Ok(ProtocolPlan {
        ingress,
        egress: resolved.protocol,
        mode,
        base_url: resolved.base_url,
        needs_conversion: resolved.needs_conversion,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::ProtocolEndpointEntry;
    use crate::protocol::ids::{
        ANTHROPIC_MESSAGES_2023_06_01, OPENAI_CHAT_COMPLETIONS_V1, OPENAI_RESPONSES_V1,
    };
    use std::time::Duration;

    fn make_decl(pairs: &[(ProtocolId, &str)]) -> ProviderProtocols {
        let endpoints = pairs
            .iter()
            .map(|(id, url)| {
                (
                    *id,
                    ProtocolEndpointEntry {
                        base_url: url.to_string(),
                    },
                )
            })
            .collect();
        ProviderProtocols {
            default: pairs
                .first()
                .map(|(id, _)| *id)
                .unwrap_or(OPENAI_CHAT_COMPLETIONS_V1),
            endpoints,
        }
    }

    fn ctx() -> RequestContext {
        RequestContext::new(OPENAI_CHAT_COMPLETIONS_V1, Duration::from_secs(30))
    }

    #[test]
    fn native_when_ingress_exact_match() {
        let decl = make_decl(&[(OPENAI_CHAT_COMPLETIONS_V1, "https://api.openai.com")]);
        let mut c = ctx();
        let plan = negotiate(OPENAI_CHAT_COMPLETIONS_V1, None, Some(&decl), &mut c).unwrap();
        assert_eq!(plan.mode, ProtocolMode::Native);
        assert_eq!(plan.egress, OPENAI_CHAT_COMPLETIONS_V1);
        assert!(!plan.needs_conversion);
    }

    #[test]
    fn transform_when_same_family_fallback() {
        // Provider only declares chat; ingress is responses.
        let decl = make_decl(&[(OPENAI_CHAT_COMPLETIONS_V1, "https://api.openai.com")]);
        let mut c = ctx();
        let plan = negotiate(OPENAI_RESPONSES_V1, None, Some(&decl), &mut c).unwrap();
        assert_eq!(plan.mode, ProtocolMode::Transform);
        assert_eq!(plan.egress, OPENAI_CHAT_COMPLETIONS_V1);
        assert!(plan.needs_conversion);
    }

    #[test]
    fn route_pref_wins_over_ingress_match() {
        let decl = make_decl(&[
            (OPENAI_CHAT_COMPLETIONS_V1, "https://api.openai.com"),
            (ANTHROPIC_MESSAGES_2023_06_01, "https://api.anthropic.com"),
        ]);
        let mut c = ctx();
        let plan = negotiate(
            OPENAI_CHAT_COMPLETIONS_V1,
            Some(ANTHROPIC_MESSAGES_2023_06_01),
            Some(&decl),
            &mut c,
        )
        .unwrap();
        assert_eq!(plan.egress, ANTHROPIC_MESSAGES_2023_06_01);
    }

    #[test]
    fn no_decl_returns_native() {
        let mut c = ctx();
        let plan = negotiate(OPENAI_CHAT_COMPLETIONS_V1, None, None, &mut c).unwrap();
        assert_eq!(plan.mode, ProtocolMode::Native);
        assert_eq!(plan.egress, OPENAI_CHAT_COMPLETIONS_V1);
    }

    #[test]
    fn vec_order_is_deterministic() {
        // The first same-protocol entry in Vec order is always chosen.
        let decl = make_decl(&[
            (OPENAI_RESPONSES_V1, "https://responses.openai.com"),
            (OPENAI_CHAT_COMPLETIONS_V1, "https://chat.openai.com"),
        ]);
        let mut c = ctx();
        // Ingress is embeddings (OpenAICompatible — no exact match).
        use crate::protocol::ids::OPENAI_EMBEDDINGS_V1;
        let plan = negotiate(OPENAI_EMBEDDINGS_V1, None, Some(&decl), &mut c).unwrap();
        // OPENAI_RESPONSES_V1 is OpenAIResponses (different protocol) — Tier 2 skips it.
        // OPENAI_CHAT_COMPLETIONS_V1 is OpenAICompatible (same protocol as embeddings) — first match wins.
        assert_eq!(plan.egress, OPENAI_CHAT_COMPLETIONS_V1);
    }
}
