//! What a judge call cost (#741). One call judges every candidate in a
//! request, so cost belongs to the call: the hook logs it once, as a
//! `judge_call` event, beside the `way_judged` events the call produced.

use serde::{Deserialize, Serialize};

use crate::profile::{Profile, Provider};

/// Anthropic bills cache reads at a tenth of the input price and five-minute
/// cache writes at 1.25 times it. The judge sets no cache breakpoints, so
/// these apply only if a provider caches on its own.
const CACHE_READ_FACTOR: f64 = 0.1;
const CACHE_WRITE_FACTOR: f64 = 1.25;

/// The usage block a provider returned with its answer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    /// Uncached input tokens.
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// The cost the provider reported for the call (OpenRouter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cost_usd: Option<f64>,
}

/// Where a call's cost came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    /// The provider reported it, or refused the call (zero).
    Provider,
    /// Tokens times the profile's prices, or the model's list price.
    PriceTable,
    /// No usage came back (deadline or transport failure), or there is no
    /// price for the model. The call may still be billed.
    Unknown,
}

impl CostSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CostSource::Provider => "provider",
            CostSource::PriceTable => "price_table",
            CostSource::Unknown => "unknown",
        }
    }
}

/// One provider call the agent made for a judge request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JudgeCall {
    pub engine: String,
    pub provider: Provider,
    pub model: String,
    pub candidates: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub cost_source: CostSource,
}

impl JudgeCall {
    /// A call priced from its usage, or of unknown cost when none came back.
    pub fn priced(engine: &str, profile: &Profile, candidates: usize, usage: Option<Usage>) -> JudgeCall {
        let (cost_usd, cost_source) = match &usage {
            Some(u) => price(u, profile),
            None => (None, CostSource::Unknown),
        };
        JudgeCall {
            engine: engine.to_string(),
            provider: profile.provider,
            model: profile.model.clone(),
            candidates,
            usage,
            cost_usd,
            cost_source,
        }
    }
}

/// The list price, USD per million input and output tokens, of a model
/// agent-ways ships a profile for and whose provider reports no cost:
/// Claude Haiku 5.5 ($0.10 / $0.50, for prompts up to 100K tokens) and
/// Haiku 4.5 ($1 / $5) on Anthropic, under the alias or a dated id.
pub fn list_price(provider: Provider, model: &str) -> Option<(f64, f64)> {
    let is = |base: &str| {
        model
            .strip_prefix(base)
            .is_some_and(|rest| rest.is_empty() || rest.strip_prefix('-').is_some_and(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit())))
    };
    if provider != Provider::Anthropic {
        return None;
    }
    if is("claude-haiku-5-5") {
        Some((0.10, 0.50))
    } else if is("claude-haiku-4-5") {
        Some((1.0, 5.0))
    } else {
        None
    }
}

/// The provider's figure where it reported one, else tokens times the
/// profile's prices (both set) or the model's list price, else unknown.
pub fn price(usage: &Usage, profile: &Profile) -> (Option<f64>, CostSource) {
    if let Some(c) = usage.provider_cost_usd {
        return (Some(c), CostSource::Provider);
    }
    let prices = match (profile.price_in_per_mtok, profile.price_out_per_mtok) {
        (Some(p_in), Some(p_out)) => Some((p_in, p_out)),
        _ => list_price(profile.provider, &profile.model),
    };
    match prices {
        Some((p_in, p_out)) => {
            let input = usage.input_tokens as f64
                + usage.cache_read_tokens as f64 * CACHE_READ_FACTOR
                + usage.cache_write_tokens as f64 * CACHE_WRITE_FACTOR;
            let cost = (input * p_in + usage.output_tokens as f64 * p_out) / 1e6;
            (Some(cost), CostSource::PriceTable)
        }
        None => (None, CostSource::Unknown),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn haiku() -> Profile {
        crate::profile::shipped()["anthropic"].clone()
    }

    #[test]
    fn anthropic_is_priced_from_the_table() {
        let u = Usage { input_tokens: 1_000_000, output_tokens: 100_000, ..Default::default() };
        let (cost, source) = price(&u, &haiku());
        assert_eq!(source, CostSource::PriceTable);
        assert!((cost.unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn cache_tokens_use_anthropic_multipliers() {
        let u = Usage { cache_read_tokens: 1_000_000, cache_write_tokens: 1_000_000, ..Default::default() };
        let (cost, _) = price(&u, &haiku());
        assert!((cost.unwrap() - 1.35).abs() < 1e-9);
    }

    #[test]
    fn the_list_price_follows_the_model_and_an_override_wins() {
        let u = Usage { input_tokens: 1_000_000, ..Default::default() };
        let mut p = haiku();
        p.model = "claude-haiku-5-5".into();
        assert_eq!(price(&u, &p), (Some(0.10), CostSource::PriceTable));
        p.model = "claude-haiku-4-5-20251001".into();
        assert_eq!(price(&u, &p), (Some(1.0), CostSource::PriceTable));
        p.model = "claude-sonnet-4-5".into();
        assert_eq!(price(&u, &p), (None, CostSource::Unknown));
        p.price_in_per_mtok = Some(3.0);
        assert_eq!(price(&u, &p), (None, CostSource::Unknown), "a lone price is ignored");
        p.model = "claude-haiku-4-5".into();
        assert_eq!(price(&u, &p), (Some(1.0), CostSource::PriceTable));
        p.model = "claude-sonnet-4-5".into();
        (p.price_in_per_mtok, p.price_out_per_mtok) = (Some(3.0), Some(15.0));
        assert_eq!(price(&u, &p), (Some(3.0), CostSource::PriceTable));
    }

    #[test]
    fn list_prices_cover_haiku_5_5_and_4_5() {
        assert_eq!(list_price(Provider::Anthropic, "claude-haiku-5-5"), Some((0.10, 0.50)));
        assert_eq!(list_price(Provider::Anthropic, "claude-haiku-5-5-20260101"), Some((0.10, 0.50)));
        assert_eq!(list_price(Provider::Anthropic, "claude-haiku-4-5"), Some((1.0, 5.0)));
        assert_eq!(list_price(Provider::Anthropic, "claude-haiku-4-5-20251001"), Some((1.0, 5.0)));
        assert_eq!(list_price(Provider::Openrouter, "anthropic/claude-haiku-5.5"), None);
    }

    #[test]
    fn a_provider_figure_wins_over_the_table() {
        let u = Usage { input_tokens: 10, output_tokens: 10, provider_cost_usd: Some(0.002), ..Default::default() };
        assert_eq!(price(&u, &haiku()), (Some(0.002), CostSource::Provider));
    }

    #[test]
    fn no_prices_and_no_usage_are_unknown() {
        let openrouter = crate::profile::shipped()["openrouter"].clone();
        assert_eq!(price(&Usage::default(), &openrouter), (None, CostSource::Unknown));
        let call = JudgeCall::priced("anthropic", &haiku(), 3, None);
        assert_eq!((call.cost_usd, call.cost_source), (None, CostSource::Unknown));
    }
}
