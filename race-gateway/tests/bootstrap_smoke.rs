use race_gateway::domain::{BootstrapData, RaceSettings};

#[test]
fn bootstrap_default_contains_settings() {
    let bootstrap = BootstrapData::default();
    assert_eq!(bootstrap.settings, Some(RaceSettings::default()));
}
