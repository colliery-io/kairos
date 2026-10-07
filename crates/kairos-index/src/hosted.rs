//! The hosted summarizer (KAIROS-T-0341, COLLIERY-I-0611): an
//! OpenAI-compatible chat endpoint. Ollama Cloud speaks it, and so do a
//! local Ollama, vLLM and OpenRouter, so one client serves them all.
//!
//! The request is `POST {base}/chat/completions` with the system prompt of
//! [`crate::summary::SYSTEM_PROMPT`] and the user prompt of
//! [`SummaryRequest::prompt`], temperature 0, a cap on the tokens of the
//! answer. The answer is `choices[0].message.content`.
//!
//! A hosted endpoint takes many requests at once, so [`HostedChat`] sends a
//! batch on `concurrency` threads ([`Summarizer::summarize_many`]). A 429 or
//! a 5xx is tried again, 3 tries in all, with a pause that grows; the final
//! error names the status and the first line of the body. The client is
//! blocking, as the embedding providers are: the builder calls it from a
//! blocking worker, never from an async task.

use std::time::Duration;

use serde::Deserialize;

use crate::summary::{SYSTEM_PROMPT, Summarizer, SummaryRequest};

/// How to reach the endpoint.
#[derive(Debug, Clone)]
pub struct HostedConfig {
    /// The provider, for the model name of the summaries: `ollama-cloud`
    /// for every OpenAI-compatible endpoint.
    pub provider: String,
    /// The base URL, without `/chat/completions`: for example
    /// `https://ollama.com/v1`.
    pub base_url: String,
    /// The model to ask for, as the endpoint names it.
    pub model: String,
    /// The bearer token, if the endpoint wants one.
    pub api_key: Option<String>,
    /// The requests sent at the same time, 1 or more.
    pub concurrency: usize,
    /// The time for one request.
    pub timeout: Duration,
}

impl HostedConfig {
    /// A configuration with the default timeout of 120 seconds.
    pub fn new(provider: &str, base_url: &str, model: &str, concurrency: usize) -> Self {
        HostedConfig {
            provider: provider.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key: None,
            concurrency: concurrency.max(1),
            timeout: Duration::from_secs(120),
        }
    }
}

/// The most tokens of one summary. A summary is 1 to 3 sentences; this cap
/// stops a model that does not stop.
const MAX_TOKENS: u32 = 400;

/// The tries for one request, and the pause before each try after the
/// first.
const TRIES: usize = 3;
const PAUSES: [Duration; 2] = [Duration::from_millis(500), Duration::from_millis(2_000)];

/// Summaries from an OpenAI-compatible chat endpoint.
pub struct HostedChat {
    client: reqwest::blocking::Client,
    config: HostedConfig,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    content: String,
}

impl HostedChat {
    /// A client for `config`. It sends nothing until the first summary.
    pub fn new(config: HostedConfig) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| format!("the HTTP client did not start: {e}"))?;
        Ok(HostedChat { client, config })
    }

    fn url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        )
    }

    /// One request, tried up to [`TRIES`] times for a 429 or a 5xx.
    fn request(&self, prompt: &str) -> Result<String, String> {
        let url = self.url();
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": prompt},
            ],
            "temperature": 0,
            "max_tokens": MAX_TOKENS,
            "stream": false,
        });
        let mut last = String::new();
        for attempt in 0..TRIES {
            if attempt > 0 {
                std::thread::sleep(PAUSES[(attempt - 1).min(PAUSES.len() - 1)]);
            }
            let mut req = self.client.post(&url).json(&body);
            if let Some(key) = &self.config.api_key {
                req = req.bearer_auth(key);
            }
            let response = match req.send() {
                Ok(response) => response,
                Err(e) => {
                    last = format!("{url}: {e}");
                    continue;
                }
            };
            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                let text = response.text().unwrap_or_default();
                last = format!(
                    "{url} answered {status}: {}",
                    text.lines().next().unwrap_or("")
                );
                continue;
            }
            if !status.is_success() {
                let text = response.text().unwrap_or_default();
                return Err(format!(
                    "{url} answered {status}: {}",
                    text.lines().next().unwrap_or("")
                ));
            }
            let parsed: ChatResponse = response
                .json()
                .map_err(|e| format!("{url}: the answer is not a chat completion: {e}"))?;
            let content = parsed
                .choices
                .into_iter()
                .next()
                .map(|c| c.message.content)
                .unwrap_or_default();
            if content.trim().is_empty() {
                return Err(format!("{url}: the answer has no content"));
            }
            return Ok(content);
        }
        Err(format!("{last} ({TRIES} tries)"))
    }
}

impl Summarizer for HostedChat {
    fn model(&self) -> String {
        format!("{}/{}", self.config.provider, self.config.model)
    }

    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        self.request(&request.prompt())
    }

    fn concurrency(&self) -> usize {
        self.config.concurrency.max(1)
    }

    /// The requests on `concurrency` threads at the same time; the results
    /// in the order of the requests.
    fn summarize_many(&mut self, requests: &[SummaryRequest]) -> Vec<Result<String, String>> {
        let prompts: Vec<String> = requests.iter().map(|r| r.prompt()).collect();
        in_parallel(&prompts, |prompt| self.request(prompt))
    }
}

/// Run `request` for each prompt on its own thread, and give the results
/// in the order of the prompts. The caller gives one batch at a time, of
/// the size of its concurrency, so the thread count is bounded.
pub(crate) fn in_parallel<F>(prompts: &[String], request: F) -> Vec<Result<String, String>>
where
    F: Fn(&str) -> Result<String, String> + Sync,
{
    let request = &request;
    std::thread::scope(|scope| {
        let handles: Vec<_> = prompts
            .iter()
            .map(|prompt| scope.spawn(move || request(prompt)))
            .collect();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("the request thread stopped".to_string()))
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_model_name_has_the_provider() {
        let chat = HostedChat::new(HostedConfig::new(
            "ollama-cloud",
            "https://ollama.com/v1/",
            "gemma4:31b",
            4,
        ))
        .unwrap();
        assert_eq!(chat.model(), "ollama-cloud/gemma4:31b");
        assert_eq!(chat.url(), "https://ollama.com/v1/chat/completions");
        assert_eq!(chat.concurrency(), 4);
    }

    #[test]
    fn the_concurrency_is_at_least_one() {
        let chat = HostedChat::new(HostedConfig::new("ollama-cloud", "http://x", "m", 0)).unwrap();
        assert_eq!(chat.concurrency(), 1);
    }
}
