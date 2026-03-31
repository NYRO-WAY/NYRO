use crate::db::models::Route;
use crate::storage::RouteSnapshotStore;

pub struct RouteCache {
    pub routes: Vec<Route>,
}

impl RouteCache {
    pub async fn load(store: &dyn RouteSnapshotStore) -> anyhow::Result<Self> {
        let routes = store.load_active_snapshot().await?;
        Ok(Self { routes })
    }

    pub async fn reload(&mut self, store: &dyn RouteSnapshotStore) -> anyhow::Result<()> {
        *self = Self::load(store).await?;
        Ok(())
    }
}

pub fn match_route<'a>(routes: &'a [Route], model: &str) -> Option<&'a Route> {
    routes
        .iter()
        .find(|route| route.virtual_model == model)
}
