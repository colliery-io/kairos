//! The AWS Bedrock summarizer (KAIROS-T-0342, COLLIERY-I-0611): the
//! Converse API, signed with Signature Version 4 by hand. No AWS SDK: it
//! brings hyper, aws-config and a large tree for one request shape.
//!
//! The request is `POST https://bedrock-runtime.{region}.amazonaws.com
//! /model/{modelId}/converse` with the system prompt, the user prompt and
//! `inferenceConfig { maxTokens, temperature: 0 }`. The answer is
//! `output.message.content[0].text`. A throttle (`ThrottlingException`, a
//! 429 or a 5xx) is tried again as the hosted chat client does.
//!
//! The signature follows the AWS documentation: the canonical request
//! (method, URI, query, the canonical headers `content-type`, `host`,
//! `x-amz-date` and, with a session token, `x-amz-security-token`, the
//! signed headers, the SHA-256 of the body), the string to sign with the
//! credential scope `{date}/{region}/bedrock/aws4_request`, and the signing
//! key derived from the secret with four HMAC steps. [`sign`] is pure, so
//! the unit test checks it against the worked example of the AWS
//! documentation.

use std::time::Duration;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::hosted::in_parallel;
use crate::summary::{SYSTEM_PROMPT, Summarizer, SummaryRequest};

type HmacSha256 = Hmac<Sha256>;

/// How to reach Bedrock.
#[derive(Debug, Clone)]
pub struct BedrockConfig {
    /// The AWS region, for example `us-east-1`.
    pub region: String,
    /// The model id, for example `anthropic.claude-3-5-haiku-20241022-v1:0`.
    pub model_id: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// The requests sent at the same time, 1 or more.
    pub concurrency: usize,
    pub timeout: Duration,
    /// Another endpoint than `https://bedrock-runtime.{region}.amazonaws.com`:
    /// a test server. The signature still names `region`.
    pub endpoint: Option<String>,
}

impl BedrockConfig {
    /// A configuration with the default timeout of 120 seconds and no
    /// session token.
    pub fn new(
        region: &str,
        model_id: &str,
        access_key_id: &str,
        secret_access_key: &str,
        concurrency: usize,
    ) -> Self {
        BedrockConfig {
            region: region.to_string(),
            model_id: model_id.to_string(),
            access_key_id: access_key_id.to_string(),
            secret_access_key: secret_access_key.to_string(),
            session_token: None,
            concurrency: concurrency.max(1),
            timeout: Duration::from_secs(120),
            endpoint: None,
        }
    }
}

/// The most tokens of one summary.
const MAX_TOKENS: u32 = 400;
const TRIES: usize = 3;
const PAUSES: [Duration; 2] = [Duration::from_millis(500), Duration::from_millis(2_000)];

/// Summaries from Bedrock.
pub struct Bedrock {
    client: reqwest::blocking::Client,
    config: BedrockConfig,
}

#[derive(Deserialize)]
struct ConverseResponse {
    output: Output,
}

#[derive(Deserialize)]
struct Output {
    message: OutMessage,
}

#[derive(Deserialize)]
struct OutMessage {
    #[serde(default)]
    content: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(default)]
    text: String,
}

/// The parts of a signed request, as [`sign`] takes them. Pure data, so the
/// signing is testable with a fixed date.
pub struct SignInput<'a> {
    pub method: &'a str,
    /// The host of the request, as the `Host` header carries it.
    pub host: &'a str,
    /// The path, URI-encoded as it is sent.
    pub path: &'a str,
    /// The query string, canonical (sorted, encoded), or empty.
    pub query: &'a str,
    pub content_type: &'a str,
    pub body: &'a [u8],
    pub region: &'a str,
    pub service: &'a str,
    pub access_key_id: &'a str,
    pub secret_access_key: &'a str,
    pub session_token: Option<&'a str>,
    pub at: DateTime<Utc>,
}

/// The result of a signature: the headers to send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub amz_date: String,
    pub authorization: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hmac(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC takes a key of any size");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Sign one request with Signature Version 4. Pure.
pub fn sign(input: &SignInput<'_>) -> Signed {
    let amz_date = input.at.format("%Y%m%dT%H%M%SZ").to_string();
    let date = input.at.format("%Y%m%d").to_string();
    let payload_hash = hex(&Sha256::digest(input.body));
    let mut headers: Vec<(&str, String)> = vec![
        ("content-type", input.content_type.to_string()),
        ("host", input.host.to_string()),
        ("x-amz-date", amz_date.clone()),
    ];
    if let Some(token) = input.session_token {
        headers.push(("x-amz-security-token", token.to_string()));
    }
    headers.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect();
    let signed_headers = headers
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method, input.path, input.query, canonical_headers, signed_headers, payload_hash
    );
    let scope = format!("{date}/{}/{}/aws4_request", input.region, input.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );
    let key = hmac(
        format!("AWS4{}", input.secret_access_key).as_bytes(),
        date.as_bytes(),
    );
    let key = hmac(&key, input.region.as_bytes());
    let key = hmac(&key, input.service.as_bytes());
    let key = hmac(&key, b"aws4_request");
    let signature = hex(&hmac(&key, string_to_sign.as_bytes()));
    Signed {
        amz_date,
        authorization: format!(
            "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, \
             Signature={signature}",
            input.access_key_id
        ),
    }
}

/// A path segment, URI-encoded as AWS canonicalizes it: the unreserved
/// characters stay, each other byte is `%XX`.
pub fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

impl Bedrock {
    /// A client for `config`. It sends nothing until the first summary.
    pub fn new(config: BedrockConfig) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| format!("the HTTP client did not start: {e}"))?;
        Ok(Bedrock { client, config })
    }

    /// The scheme and host of the endpoint.
    fn base(&self) -> String {
        match &self.config.endpoint {
            Some(endpoint) => endpoint.trim_end_matches('/').to_string(),
            None => format!(
                "https://bedrock-runtime.{}.amazonaws.com",
                self.config.region
            ),
        }
    }

    fn path(&self) -> String {
        format!("/model/{}/converse", encode_segment(&self.config.model_id))
    }

    fn request(&self, prompt: &str) -> Result<String, String> {
        let base = self.base();
        let host = base
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .to_string();
        let path = self.path();
        let url = format!("{base}{path}");
        let body = serde_json::json!({
            "system": [{"text": SYSTEM_PROMPT}],
            "messages": [{"role": "user", "content": [{"text": prompt}]}],
            "inferenceConfig": {"maxTokens": MAX_TOKENS, "temperature": 0},
        })
        .to_string();
        let mut last = String::new();
        for attempt in 0..TRIES {
            if attempt > 0 {
                std::thread::sleep(PAUSES[(attempt - 1).min(PAUSES.len() - 1)]);
            }
            let signed = sign(&SignInput {
                method: "POST",
                host: &host,
                path: &path,
                query: "",
                content_type: "application/json",
                body: body.as_bytes(),
                region: &self.config.region,
                service: "bedrock",
                access_key_id: &self.config.access_key_id,
                secret_access_key: &self.config.secret_access_key,
                session_token: self.config.session_token.as_deref(),
                at: Utc::now(),
            });
            let mut req = self
                .client
                .post(&url)
                .header("content-type", "application/json")
                .header("host", &host)
                .header("x-amz-date", &signed.amz_date)
                .header("authorization", &signed.authorization)
                .body(body.clone());
            if let Some(token) = &self.config.session_token {
                req = req.header("x-amz-security-token", token);
            }
            let response = match req.send() {
                Ok(response) => response,
                Err(e) => {
                    last = format!("{url}: {e}");
                    continue;
                }
            };
            let status = response.status();
            if !status.is_success() {
                // Bedrock names the error in the header x-amzn-ErrorType and
                // gives a message in the body.
                let error_type = response
                    .headers()
                    .get("x-amzn-errortype")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                let text = response.text().unwrap_or_default();
                let first = text.lines().next().unwrap_or("").to_string();
                let throttled = status.as_u16() == 429
                    || status.is_server_error()
                    || error_type.contains("Throttling")
                    || text.contains("ThrottlingException");
                last = if error_type.is_empty() {
                    format!("{url} answered {status}: {first}")
                } else {
                    format!("{url} answered {status} {error_type}: {first}")
                };
                if throttled {
                    continue;
                }
                return Err(last);
            }
            let parsed: ConverseResponse = response
                .json()
                .map_err(|e| format!("{url}: the answer is not a Converse response: {e}"))?;
            let content = parsed
                .output
                .message
                .content
                .into_iter()
                .map(|b| b.text)
                .collect::<Vec<_>>()
                .join("");
            if content.trim().is_empty() {
                return Err(format!("{url}: the answer has no text"));
            }
            return Ok(content);
        }
        Err(format!("{last} ({TRIES} tries)"))
    }
}

impl Summarizer for Bedrock {
    fn model(&self) -> String {
        format!("bedrock/{}", self.config.model_id)
    }

    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        self.request(&request.prompt())
    }

    fn concurrency(&self) -> usize {
        self.config.concurrency.max(1)
    }

    fn summarize_many(&mut self, requests: &[SummaryRequest]) -> Vec<Result<String, String>> {
        let prompts: Vec<String> = requests.iter().map(|r| r.prompt()).collect();
        in_parallel(&prompts, |prompt| self.request(prompt))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// The worked example of the AWS documentation ("Signature Version 4
    /// signing process", the IAM ListUsers GET): the known signature.
    #[test]
    fn the_signature_matches_the_aws_example() {
        let signed = sign(&SignInput {
            method: "GET",
            host: "iam.amazonaws.com",
            path: "/",
            query: "Action=ListUsers&Version=2010-05-08",
            content_type: "application/x-www-form-urlencoded; charset=utf-8",
            body: b"",
            region: "us-east-1",
            service: "iam",
            access_key_id: "AKIDEXAMPLE",
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            session_token: None,
            at: Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap(),
        });
        assert_eq!(signed.amz_date, "20150830T123600Z");
        assert_eq!(
            signed.authorization,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/iam/aws4_request, \
             SignedHeaders=content-type;host;x-amz-date, \
             Signature=5d672d79c15b13162d9279b0855cfba6789a8edb4c82c400e06b5924a6f2b5d7"
        );
    }

    #[test]
    fn a_session_token_is_a_signed_header() {
        let signed = sign(&SignInput {
            method: "POST",
            host: "bedrock-runtime.us-east-1.amazonaws.com",
            path: "/model/x/converse",
            query: "",
            content_type: "application/json",
            body: b"{}",
            region: "us-east-1",
            service: "bedrock",
            access_key_id: "AKID",
            secret_access_key: "secret",
            session_token: Some("token"),
            at: Utc.with_ymd_and_hms(2026, 10, 7, 8, 0, 0).unwrap(),
        });
        assert!(
            signed
                .authorization
                .contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token,")
        );
    }

    #[test]
    fn the_model_id_is_encoded_in_the_path() {
        assert_eq!(
            encode_segment("anthropic.claude-3-5-haiku-20241022-v1:0"),
            "anthropic.claude-3-5-haiku-20241022-v1%3A0"
        );
        let bedrock = Bedrock::new(BedrockConfig::new(
            "eu-west-1",
            "anthropic.claude-3-5-haiku-20241022-v1:0",
            "AKID",
            "secret",
            2,
        ))
        .unwrap();
        assert_eq!(
            bedrock.model(),
            "bedrock/anthropic.claude-3-5-haiku-20241022-v1:0"
        );
        assert_eq!(
            bedrock.base(),
            "https://bedrock-runtime.eu-west-1.amazonaws.com"
        );
        assert_eq!(
            bedrock.path(),
            "/model/anthropic.claude-3-5-haiku-20241022-v1%3A0/converse"
        );
    }
}
