#[derive(Debug, Clone)]
pub struct MongoStorageConfig {
    pub uri: String,
    pub database: String,
    pub collections: MongoCollectionNames,
}

impl MongoStorageConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.uri.trim().is_empty() {
            anyhow::bail!("mongo uri cannot be empty");
        }
        if self.database.trim().is_empty() {
            anyhow::bail!("mongo database cannot be empty");
        }
        self.collections.validate()
    }
}

#[derive(Debug, Clone)]
pub struct MongoCollectionNames {
    pub providers: String,
    pub routes: String,
    pub api_keys: String,
    pub api_key_routes: String,
    pub request_logs: String,
    pub settings: String,
}

impl Default for MongoCollectionNames {
    fn default() -> Self {
        Self {
            providers: "providers".to_string(),
            routes: "routes".to_string(),
            api_keys: "api_keys".to_string(),
            api_key_routes: "api_key_routes".to_string(),
            request_logs: "request_logs".to_string(),
            settings: "settings".to_string(),
        }
    }
}

impl MongoCollectionNames {
    pub fn validate(&self) -> anyhow::Result<()> {
        let all = [
            &self.providers,
            &self.routes,
            &self.api_keys,
            &self.api_key_routes,
            &self.request_logs,
            &self.settings,
        ];
        for name in all {
            if name.trim().is_empty() {
                anyhow::bail!("mongo collection name cannot be empty");
            }
        }
        Ok(())
    }
}

