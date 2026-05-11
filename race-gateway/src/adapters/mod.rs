mod anthropic;
mod google;
mod openai;

use std::collections::BTreeMap;

use bytes::Bytes;

use crate::domain::{DownstreamRouteKind, ProtocolFamily};
use crate::group::CandidateState;

pub use anthropic::AnthropicAdapter;
pub use google::GoogleAdapter;
pub use openai::OpenAiAdapter;

#[derive(Debug, Default, Clone, Copy)]
pub struct EventAnalysis {
    pub has_content: bool,
}

pub trait RaceProtocolAdapter: Send + Sync {
    fn protocol_family(&self) -> ProtocolFamily;
    fn route_kind(&self) -> DownstreamRouteKind;
    fn protocol_label(&self) -> &'static str;
    fn response_content_type(&self) -> &'static str {
        "text/event-stream"
    }
    fn inspect_event(&self, state: &mut CandidateState, event: &[u8]) -> EventAnalysis;
    fn all_failed_events(&self, group_name: &str, errors: &BTreeMap<String, String>) -> Vec<Bytes>;
    fn fallback_close_events(&self, group_name: &str, state: &CandidateState) -> Vec<Bytes>;
}

pub fn adapter_for_route(route_kind: DownstreamRouteKind) -> Box<dyn RaceProtocolAdapter> {
    match route_kind {
        DownstreamRouteKind::OpenAiChatCompletions | DownstreamRouteKind::OpenAiResponses => {
            Box::new(OpenAiAdapter::new(route_kind))
        }
        DownstreamRouteKind::AnthropicMessages => Box::new(AnthropicAdapter),
        DownstreamRouteKind::GoogleV1BetaModels | DownstreamRouteKind::GoogleV1Models => {
            Box::new(GoogleAdapter::new(route_kind))
        }
    }
}
