use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, bail};

use crate::adapters::{RaceProtocolAdapter, adapter_for_route};
use crate::app::AppState;
use crate::domain::{RaceModelDescriptor, resolve_candidates_for_protocol};
use crate::downstream::{DispatchRoute, DownstreamStreamFactory};
use crate::group::{RaceCore, RaceExecutionSettings, RaceParticipant};
use crate::key_pool::{KeyPoolSelector, RandomKeyPoolSelector};

use super::request::ProxyRouteRequest;

#[derive(Clone)]
pub struct RaceRunner {
    state: AppState,
}

impl RaceRunner {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub async fn race_stream(
        &self,
        request: ProxyRouteRequest,
    ) -> anyhow::Result<crate::group::RaceStreamExecution> {
        let group = self
            .state
            .config_cache
            .get_group(&request.group_id)
            .with_context(|| format!("group '{}' not found", request.group_id))?;
        if !group.enabled {
            bail!("group '{}' is disabled", group.id);
        }
        let settings = self.state.config_cache.get_settings();

        let models = load_models(&group, &self.state);
        let adapter: Arc<dyn RaceProtocolAdapter> =
            Arc::from(adapter_for_route(request.route_kind));
        let protocol_label = adapter.protocol_label().to_string();
        let resolved = resolve_candidates_for_protocol(&group, &models, adapter.protocol_family())?;
        let selector = RandomKeyPoolSelector;

        let mut participants = Vec::new();
        for target in resolved {
            let pool = self
                .state
                .config_cache
                .get_key_pool(&target.key_pool_id)
                .with_context(|| format!("key pool '{}' not found", target.key_pool_id))?;
            let selected_key = selector.select_key(&pool, None)?;
            participants.push(RaceParticipant {
                candidate: target.candidate,
                endpoint: target.endpoint,
                selected_key: selected_key.clone(),
                masked_key: crate::group::mask_key(&selected_key.secret),
            });
        }

        let runtime = self.state.runtime.ensure_group(&group);
        let stream_factory = Arc::new(DownstreamStreamFactory {
            dispatcher: self.state.dispatcher.clone(),
            route: DispatchRoute {
                route_kind: request.route_kind,
                model_action: request.model_action.clone(),
            },
            request_headers: request.headers.clone(),
            request_body: request.body.clone(),
        });

        RaceCore::new(
            group,
            participants,
            runtime,
            adapter,
            protocol_label.clone(),
            self.state.observability.clone(),
            settings.enable_race_diagnostics_header && request.diagnostics_enabled,
            RaceExecutionSettings {
                max_buffer_events: settings.max_buffer_events,
                max_buffer_bytes: crate::group::MAX_BUFFER_BYTES,
                buffer_backpressure_timeout_ms: settings.buffer_backpressure_timeout_ms,
            },
        )
        .execute(
            stream_factory,
            self.state.observability.start_race(&protocol_label),
        )
        .await
    }
}

fn load_models(
    group: &crate::domain::RaceGroup,
    state: &AppState,
) -> BTreeMap<String, RaceModelDescriptor> {
    state.config_cache.models_for_candidates(&group.candidates)
}
