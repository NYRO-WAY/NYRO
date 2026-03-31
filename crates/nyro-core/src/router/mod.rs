pub mod health;
mod matcher;
pub mod selector;

pub use matcher::RouteCache;
pub use selector::{SelectedTarget, TargetSelector};

use crate::db::models::Route;

impl RouteCache {
    pub fn match_route(&self, model: &str) -> Option<&Route> {
        matcher::match_route(&self.routes, model)
    }
}
