use std::path::Path;

use anyhow::Context;

use crate::domain::BootstrapData;

pub fn load_bootstrap_data(path: &Path) -> anyhow::Result<BootstrapData> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read bootstrap file {}", path.display()))?;
    let data = serde_json::from_str::<BootstrapData>(&raw)
        .with_context(|| format!("failed to parse bootstrap file {}", path.display()))?;
    Ok(data)
}
