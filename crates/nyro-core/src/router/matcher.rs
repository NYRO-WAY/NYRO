use sqlx::SqlitePool;

use crate::db::models::Route;

pub struct RouteCache {
    pub routes: Vec<Route>,
}

impl RouteCache {
    pub async fn load(pool: &SqlitePool) -> anyhow::Result<Self> {
        let routes: Vec<Route> = sqlx::query_as::<_, Route>(
            r#"SELECT
                id, name, COALESCE(ingress_protocol, 'openai') AS ingress_protocol,
                COALESCE(NULLIF(virtual_model, ''), match_pattern) AS virtual_model,
                target_provider, target_model,
                COALESCE(access_control, 0) AS access_control,
                is_active,
                created_at
            FROM routes
            WHERE is_active = 1"#,
        )
        .fetch_all(pool)
        .await?;

        Ok(Self { routes })
    }

    pub async fn reload(&mut self, pool: &SqlitePool) -> anyhow::Result<()> {
        *self = Self::load(pool).await?;
        Ok(())
    }
}

pub fn match_route<'a>(routes: &'a [Route], ingress_protocol: &str, model: &str) -> Option<&'a Route> {
    routes
        .iter()
        .find(|route| route.ingress_protocol == ingress_protocol && route.virtual_model == model)
}
