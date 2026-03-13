mod matcher;

pub use matcher::RouteCache;

use crate::db::models::Route;

impl RouteCache {
    pub fn match_route(&self, ingress_protocol: &str, model: &str) -> Option<&Route> {
        matcher::match_route(&self.routes, ingress_protocol, model)
    }
}
