use std::time::Duration;

use anyhow::Context;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec, Opts, Registry,
    TextEncoder,
};

#[derive(Clone)]
pub struct Observability {
    registry: Registry,
    http_requests_total: IntCounterVec,
    http_request_duration_seconds: HistogramVec,
    active_races: IntGaugeVec,
    races_total: IntCounterVec,
    race_duration_seconds: HistogramVec,
}

impl Observability {
    pub fn new() -> anyhow::Result<Self> {
        let registry = Registry::new();

        let http_requests_total = IntCounterVec::new(
            Opts::new(
                "race_gateway_http_requests_total",
                "HTTP requests served by surface, route and status.",
            ),
            &["surface", "route", "method", "status"],
        )
        .context("create http_requests_total metric")?;
        registry
            .register(Box::new(http_requests_total.clone()))
            .context("register http_requests_total metric")?;

        let http_request_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "race_gateway_http_request_duration_seconds",
                "HTTP request duration by surface and route.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
            ]),
            &["surface", "route", "method"],
        )
        .context("create http_request_duration_seconds metric")?;
        registry
            .register(Box::new(http_request_duration_seconds.clone()))
            .context("register http_request_duration_seconds metric")?;

        let active_races = IntGaugeVec::new(
            Opts::new(
                "race_gateway_active_races",
                "Currently active race requests by protocol.",
            ),
            &["protocol"],
        )
        .context("create active_races metric")?;
        registry
            .register(Box::new(active_races.clone()))
            .context("register active_races metric")?;

        let races_total = IntCounterVec::new(
            Opts::new(
                "race_gateway_races_total",
                "Race executions completed by protocol and outcome.",
            ),
            &["protocol", "outcome"],
        )
        .context("create races_total metric")?;
        registry
            .register(Box::new(races_total.clone()))
            .context("register races_total metric")?;

        let race_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "race_gateway_race_duration_seconds",
                "End-to-end race duration by protocol and outcome.",
            )
            .buckets(vec![
                0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
            ]),
            &["protocol", "outcome"],
        )
        .context("create race_duration_seconds metric")?;
        registry
            .register(Box::new(race_duration_seconds.clone()))
            .context("register race_duration_seconds metric")?;

        Ok(Self {
            registry,
            http_requests_total,
            http_request_duration_seconds,
            active_races,
            races_total,
            race_duration_seconds,
        })
    }

    pub fn observe_http(
        &self,
        surface: &str,
        route: &str,
        method: &str,
        status: u16,
        duration: Duration,
    ) {
        let status_owned = status.to_string();
        self.http_requests_total
            .with_label_values(&[surface, route, method, status_owned.as_str()])
            .inc();
        self.http_request_duration_seconds
            .with_label_values(&[surface, route, method])
            .observe(duration.as_secs_f64());
    }

    pub fn start_race(&self, protocol: &str) -> ActiveRaceGuard {
        let gauge = self.active_races.with_label_values(&[protocol]);
        gauge.inc();
        ActiveRaceGuard { gauge: Some(gauge) }
    }

    pub fn finish_race(&self, protocol: &str, outcome: &str, duration: Duration) {
        self.races_total
            .with_label_values(&[protocol, outcome])
            .inc();
        self.race_duration_seconds
            .with_label_values(&[protocol, outcome])
            .observe(duration.as_secs_f64());
    }

    pub fn render(&self) -> anyhow::Result<String> {
        let families = self.registry.gather();
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&families, &mut output)
            .context("encode prometheus metrics")?;
        String::from_utf8(output).context("metrics output is not utf-8")
    }
}

pub struct ActiveRaceGuard {
    gauge: Option<IntGauge>,
}

impl Drop for ActiveRaceGuard {
    fn drop(&mut self) {
        if let Some(gauge) = self.gauge.take() {
            gauge.dec();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Observability;

    #[test]
    fn metrics_render_contains_registered_series() {
        let observability = Observability::new().expect("observability");
        let guard = observability.start_race("openai");
        observability.observe_http(
            "proxy",
            "/healthz",
            "GET",
            200,
            std::time::Duration::from_millis(12),
        );
        observability.finish_race("openai", "winner", std::time::Duration::from_millis(34));
        drop(guard);

        let rendered = observability.render().expect("render metrics");
        assert!(rendered.contains("race_gateway_http_requests_total"));
        assert!(rendered.contains("race_gateway_active_races"));
        assert!(rendered.contains("race_gateway_races_total"));
    }
}
