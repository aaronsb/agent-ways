//! Provider HTTP calls: the zero-cost key check and the model list (ADR-502 §6).
//!
//! Requests go over ureq with rustls. Error text returned here comes from the
//! provider's error body or the transport, never from the request, so a key
//! cannot leak through it.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde_json::Value;

use crate::cost::Usage;
use crate::profile::Provider;

const ANTHROPIC: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";

/// An HTTP agent that reports 4xx and 5xx as responses, so callers can name
/// the status. Clones share one connection pool.
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .user_agent(concat!("ways-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// What a key check found.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    /// The key works and the model is served.
    Valid,
    Invalid(String),
    NoCredit,
    RateLimited,
    /// The key works; the provider does not serve this model to it.
    ModelUnavailable(String),
    /// The provider could not be reached.
    Unreachable(String),
    /// Any other status, with the provider's message.
    Failed(u16, String),
}

impl Check {
    pub fn is_valid(&self) -> bool {
        matches!(self, Check::Valid)
    }

    /// The provider accepted the key, whatever else it said about the model
    /// or the balance.
    pub fn key_authenticated(&self) -> bool {
        matches!(self, Check::Valid | Check::NoCredit | Check::ModelUnavailable(_))
    }

    /// The word a check record stores.
    pub fn record_word(&self) -> &'static str {
        match self {
            Check::Valid => "valid",
            Check::Invalid(_) => "invalid",
            Check::NoCredit => "no_credit",
            Check::RateLimited => "rate_limited",
            Check::ModelUnavailable(_) => "model_unavailable",
            Check::Unreachable(_) => "unreachable",
            Check::Failed(..) => "failed",
        }
    }
}

impl std::fmt::Display for Check {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Check::Valid => write!(f, "valid"),
            Check::Invalid(m) => write!(f, "invalid: {m}"),
            Check::NoCredit => write!(f, "no credit left on this key"),
            Check::RateLimited => write!(f, "rate-limited; try again shortly"),
            Check::ModelUnavailable(m) => write!(f, "key valid, but the provider does not offer model {m}"),
            Check::Unreachable(m) => write!(f, "provider unreachable: {m}"),
            Check::Failed(code, m) => write!(f, "provider answered {code}: {m}"),
        }
    }
}

/// Checks the key and the model with calls that cost nothing: a model lookup
/// on Anthropic; the key-info and model-endpoints lookups on OpenRouter.
pub fn check(provider: Provider, key: &str, model: &str) -> Check {
    let http = agent(Duration::from_secs(15));
    match provider {
        Provider::Anthropic => {
            let url = format!("{ANTHROPIC}/v1/models/{model}");
            match get(&http, &url, Some(provider), key) {
                Err(e) => Check::Unreachable(e),
                Ok((200, _)) => Check::Valid,
                Ok((404, _)) => Check::ModelUnavailable(model.to_string()),
                Ok((status, body)) => status_check(status, &body),
            }
        }
        Provider::Openrouter => {
            let info = match get(&http, &format!("{OPENROUTER}/key"), Some(provider), key) {
                Err(e) => return Check::Unreachable(e),
                Ok((200, body)) => body,
                Ok((status, body)) => return status_check(status, &body),
            };
            let remaining = info.pointer("/data/limit_remaining").and_then(Value::as_f64);
            if remaining.is_some_and(|r| r <= 0.0) {
                return Check::NoCredit;
            }
            match get(&http, &format!("{OPENROUTER}/models/{model}/endpoints"), None, "") {
                Err(e) => Check::Unreachable(e),
                Ok((200, _)) => Check::Valid,
                Ok((404, _)) => Check::ModelUnavailable(model.to_string()),
                Ok((status, body)) => status_check(status, &body),
            }
        }
    }
}

fn status_check(status: u16, body: &Value) -> Check {
    let message = error_message(body);
    match status {
        401 | 403 => Check::Invalid(message),
        402 => Check::NoCredit,
        429 => Check::RateLimited,
        _ => Check::Failed(status, message),
    }
}

/// The provider's own error message, from either provider's error shape.
pub(crate) fn error_message(body: &Value) -> String {
    body.pointer("/error/message")
        .and_then(Value::as_str)
        .unwrap_or("no message")
        .chars()
        .take(300)
        .collect()
}

pub use crate::models::ModelInfo;

/// The provider's model list. Anthropic's needs the key and carries no prices;
/// OpenRouter's is public and priced.
pub fn models(provider: Provider, key: Option<&str>) -> Result<Vec<ModelInfo>> {
    let http = agent(Duration::from_secs(20));
    let (url, auth) = match provider {
        Provider::Anthropic => {
            let Some(key) = key else { bail!("listing Anthropic's models needs a key; run `ways agent key add --provider anthropic`") };
            (format!("{ANTHROPIC}/v1/models?limit=1000"), Some((provider, key)))
        }
        Provider::Openrouter => (format!("{OPENROUTER}/models"), None),
    };
    let (status, body) = match auth {
        Some((p, k)) => get(&http, &url, Some(p), k),
        None => get(&http, &url, None, ""),
    }
    .map_err(anyhow::Error::msg)
    .context("listing models")?;
    if status != 200 {
        bail!("listing models: provider answered {status}: {}", error_message(&body));
    }
    let per_mtok = |v: Option<&Value>| {
        v.and_then(Value::as_str).and_then(|s| s.parse::<f64>().ok()).map(|p| p * 1e6)
    };
    let list = body.get("data").and_then(Value::as_array).context("model list has no data array")?;
    Ok(list
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?.to_string();
            let name = m
                .get("display_name")
                .or_else(|| m.get("name"))
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            Some(ModelInfo {
                id,
                name,
                input_per_mtok: per_mtok(m.pointer("/pricing/prompt")),
                output_per_mtok: per_mtok(m.pointer("/pricing/completion")),
            })
        })
        .collect())
}

/// A judge call that gave no verdicts. `reason` is the fallback reason:
/// `deadline`, `provider_<status>: …`, `transport: …` or `answer: …`.
/// `usage` is set when the provider answered with one, so the call is priced
/// even though its answer was unusable, and at zero when it refused the call
/// with a 4xx.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeFailure {
    pub reason: String,
    pub usage: Option<Usage>,
}

impl JudgeFailure {
    fn bare(reason: String) -> JudgeFailure {
        JudgeFailure { reason, usage: None }
    }
}

/// Asks the engine one batched question: P(yes) per candidate, in order,
/// with the usage the provider reported.
pub fn judge(
    http: &ureq::Agent,
    provider: Provider,
    key: &str,
    model: &str,
    prompt: &str,
    n: usize,
    timeout: Duration,
) -> std::result::Result<(Vec<f64>, Option<Usage>), JudgeFailure> {
    use crate::judge;
    // Haiku 5.5's tokenizer counts the same text as about 30% more tokens.
    let max_tokens = 96 + 64 * n;
    // Newer models answer HTTP 400 to any non-default sampling parameter.
    let temperature = crate::profile::accepts_sampling(model);
    let (url, mut body) = match provider {
        Provider::Anthropic => (
            format!("{ANTHROPIC}/v1/messages"),
            serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": judge::SYSTEM,
                "tools": [{
                    "name": judge::TOOL_NAME,
                    "description": judge::TOOL_DESCRIPTION,
                    "strict": true,
                    "input_schema": judge::tool_schema(n),
                }],
                "tool_choice": {"type": "tool", "name": judge::TOOL_NAME},
                "messages": [{"role": "user", "content": prompt}],
            }),
        ),
        Provider::Openrouter => (
            format!("{OPENROUTER}/chat/completions"),
            serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": [
                    {"role": "system", "content": judge::SYSTEM},
                    {"role": "user", "content": prompt},
                ],
                "tools": [{"type": "function", "function": {
                    "name": judge::TOOL_NAME,
                    "description": judge::TOOL_DESCRIPTION,
                    "parameters": judge::tool_schema(n),
                }}],
                "tool_choice": {"type": "function", "function": {"name": judge::TOOL_NAME}},
            }),
        ),
    };
    if temperature {
        body["temperature"] = serde_json::json!(0);
    }
    let mut req = http.post(&url).config().timeout_global(Some(timeout)).build();
    req = match provider {
        Provider::Anthropic => req.header("x-api-key", key).header("anthropic-version", ANTHROPIC_VERSION),
        Provider::Openrouter => req.header("Authorization", &format!("Bearer {key}")).header("X-Title", "agent-ways"),
    };
    let mut resp = req.send_json(&body).map_err(|e| JudgeFailure::bare(transport_reason(e)))?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| JudgeFailure::bare(transport_reason(e)))?;
    // Status first: an HTML error page from a proxy is still a provider error.
    let reply: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status != 200 {
        // A provider that refuses a request (4xx) does not bill it. A
        // timeout or a 5xx, from the provider or a proxy, may come after the
        // model ran, so its cost is unknown.
        let refused = (400..500).contains(&status) && status != 408;
        let usage = refused.then(|| Usage { provider_cost_usd: Some(0.0), ..Default::default() });
        return Err(JudgeFailure { reason: format!("provider_{status}: {}", error_message(&reply)), usage });
    }
    let usage = usage(provider, &reply);
    let failed = |reason: String| JudgeFailure { reason, usage: usage.clone() };
    let input = match provider {
        Provider::Anthropic => reply
            .get("content")
            .and_then(Value::as_array)
            .and_then(|blocks| blocks.iter().find(|b| b.get("type").and_then(Value::as_str) == Some("tool_use")))
            .and_then(|b| b.get("input"))
            .cloned()
            .ok_or_else(|| failed(format!("answer: no tool_use block (stop_reason {})", reply["stop_reason"])))?,
        Provider::Openrouter => {
            let args = reply
                .pointer("/choices/0/message/tool_calls/0/function/arguments")
                .and_then(Value::as_str)
                .ok_or_else(|| failed("answer: no tool call".to_string()))?;
            serde_json::from_str(args).map_err(|e| failed(format!("answer: arguments are not JSON: {e}")))?
        }
    };
    let p_yes = judge::parse_judgements(&input, n).map_err(|e| failed(format!("answer: {e}")))?;
    Ok((p_yes, usage))
}

/// The usage block of a 200 reply, in either provider's shape. `None` when
/// the reply carries no token counts.
fn usage(provider: Provider, reply: &Value) -> Option<Usage> {
    let u = reply.get("usage")?;
    let n = |ptr: &str| u.pointer(ptr).and_then(Value::as_u64);
    match provider {
        Provider::Anthropic => Some(Usage {
            input_tokens: n("/input_tokens")?,
            output_tokens: n("/output_tokens")?,
            cache_read_tokens: n("/cache_read_input_tokens").unwrap_or(0),
            cache_write_tokens: n("/cache_creation_input_tokens").unwrap_or(0),
            provider_cost_usd: None,
        }),
        // OpenRouter's prompt_tokens counts cache reads and writes too.
        Provider::Openrouter => {
            let cached = n("/prompt_tokens_details/cached_tokens").unwrap_or(0);
            let written = n("/prompt_tokens_details/cache_write_tokens").unwrap_or(0);
            Some(Usage {
                input_tokens: n("/prompt_tokens")?.saturating_sub(cached + written),
                output_tokens: n("/completion_tokens")?,
                cache_read_tokens: cached,
                cache_write_tokens: written,
                provider_cost_usd: u.get("cost").and_then(Value::as_f64),
            })
        }
    }
}

fn transport_reason(e: ureq::Error) -> String {
    match e {
        ureq::Error::Timeout(_) => "deadline".to_string(),
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => "deadline".to_string(),
        other => format!("transport: {other}"),
    }
}

/// GET with the provider's auth headers. Returns the status and the parsed
/// body (Null when it is not JSON), or the transport error's text.
fn get(http: &ureq::Agent, url: &str, auth: Option<Provider>, key: &str) -> std::result::Result<(u16, Value), String> {
    let mut req = http.get(url);
    match auth {
        Some(Provider::Anthropic) => {
            req = req.header("x-api-key", key).header("anthropic-version", ANTHROPIC_VERSION);
        }
        Some(Provider::Openrouter) => {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        None => {}
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statuses_map_to_checks() {
        let body = json!({"type": "error", "error": {"type": "authentication_error", "message": "invalid x-api-key"}});
        assert_eq!(status_check(401, &body), Check::Invalid("invalid x-api-key".into()));
        assert_eq!(status_check(402, &json!({})), Check::NoCredit);
        assert_eq!(status_check(429, &Value::Null), Check::RateLimited);
        assert_eq!(status_check(500, &Value::Null), Check::Failed(500, "no message".into()));
    }

    #[test]
    fn usage_reads_both_provider_shapes() {
        let anthropic = json!({"usage": {"input_tokens": 900, "output_tokens": 40, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}});
        assert_eq!(
            usage(Provider::Anthropic, &anthropic),
            Some(Usage { input_tokens: 900, output_tokens: 40, ..Default::default() })
        );
        let openrouter = json!({"usage": {"prompt_tokens": 1000, "completion_tokens": 40, "cost": 0.0012, "prompt_tokens_details": {"cached_tokens": 100, "cache_write_tokens": 50}}});
        assert_eq!(
            usage(Provider::Openrouter, &openrouter),
            Some(Usage { input_tokens: 850, output_tokens: 40, cache_read_tokens: 100, cache_write_tokens: 50, provider_cost_usd: Some(0.0012) })
        );
        assert_eq!(usage(Provider::Anthropic, &json!({"content": []})), None);
    }

    #[test]
    fn error_message_reads_both_provider_shapes() {
        assert_eq!(error_message(&json!({"error": {"message": "No auth", "code": 401}})), "No auth");
        assert_eq!(error_message(&json!({"type": "error", "error": {"message": "bad"}})), "bad");
    }
}
