//! Any OpenAI-compatible chat API (Gemini's OpenAI endpoint, OpenRouter, Groq, Ollama, …).

use std::future::Future;
use std::time::Duration;

use serde_json::{Value, json};

use crate::{Backend, Cancel, Error, Model, Request, Role, failed};

pub struct OpenAiCompat {
    /// Up to and without `/chat/completions`.
    pub base_url: String,
    pub model: String,
    /// Empty = no Authorization header (a local server).
    pub key: String,
}

impl Backend for OpenAiCompat {
    fn complete(&self, req: &Request, cancel: &Cancel) -> Result<String, Error> {
        if self.model.trim().is_empty() {
            return Err(failed("No model set. Pick one in Settings."));
        }
        let base = self.base()?;
        block_on(cancel, self.send(base, req))
    }

    fn models(&self, cancel: &Cancel) -> Result<Vec<Model>, Error> {
        let base = self.base()?;
        block_on(cancel, async {
            let r = self.auth(client(Duration::from_secs(20))?.get(format!("{base}/models")));
            let r = r.send().await.map_err(|e| failed(network(&e)))?;
            let status = r.status().as_u16();
            parse_models(status, &r.text().await.map_err(|e| failed(network(&e)))?)
        })
    }
}

fn block_on<T>(cancel: &Cancel, work: impl Future<Output = Result<T, Error>>) -> Result<T, Error> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| failed(e.to_string()))?;
    let result = rt.block_on(async {
        tokio::select! {
            r = work => r,
            _ = async { while !cancel.is_cancelled() { tokio::time::sleep(Duration::from_millis(50)).await } } => Err(Error::Cancelled),
        }
    });
    // A DNS lookup may still hang on a helper thread: don't wait for it.
    rt.shutdown_background();
    result
}

/// No redirects: the key must not travel to another address.
fn client(timeout: Duration) -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| failed(e.to_string()))
}

/// Models that can't chat, by name (the list doesn't say what a model is for).
const NOT_CHAT: [&str; 11] = ["embed", "tts", "whisper", "transcribe", "imagen", "-image", "veo-", "dall-e", "moderation", "rerank", "-live-"];

fn parse_models(status: u16, text: &str) -> Result<Vec<Model>, Error> {
    if status != 200 {
        return Err(error(status, text));
    }
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let data = v["data"].as_array().ok_or_else(|| failed("The API doesn't list its models."))?;
    let mut ids: Vec<String> = data
        .iter()
        .filter_map(|m| m["id"].as_str())
        // Gemini lists "models/x" but takes "x" too.
        .map(|id| id.strip_prefix("models/").unwrap_or(id).to_string())
        .filter(|id| !NOT_CHAT.iter().any(|n| id.to_ascii_lowercase().contains(n)))
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids.into_iter().map(|id| Model { name: id.clone(), id }).collect())
}

impl OpenAiCompat {
    fn base(&self) -> Result<&str, Error> {
        let base = self.base_url.trim().trim_end_matches('/');
        let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
            .iter()
            .any(|p| base.strip_prefix(p).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])));
        if base.starts_with("https://") || local {
            Ok(base)
        } else {
            Err(failed("The API address must start with https:// (http:// only for localhost)."))
        }
    }

    fn auth(&self, r: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if self.key.is_empty() { r } else { r.bearer_auth(&self.key) }
    }

    async fn send(&self, base: &str, req: &Request) -> Result<String, Error> {
        let client = client(Duration::from_secs(600))?;
        // Not every server knows json_schema; the prompt describes the shape too.
        let formats = [
            Some(json!({ "type": "json_schema", "json_schema": { "name": "answer", "strict": true, "schema": req.schema } })),
            Some(json!({ "type": "json_object" })),
            None,
        ];
        let last = formats.len() - 1;
        for (i, format) in formats.into_iter().enumerate() {
            let mut body = json!({ "model": self.model.trim(), "messages": messages(req) });
            if let Some(f) = format {
                body["response_format"] = f;
            }
            let r = self.auth(client.post(format!("{base}/chat/completions")).json(&body));
            let r = r.send().await.map_err(|e| failed(network(&e)))?;
            let status = r.status().as_u16();
            let text = r.text().await.map_err(|e| failed(network(&e)))?;
            if matches!(status, 400 | 422) && i < last && ["response_format", "json_schema", "json_object"].iter().any(|k| text.contains(k)) {
                continue;
            }
            return parse(status, &text);
        }
        unreachable!("the last format always returns")
    }
}

fn messages(req: &Request) -> Vec<Value> {
    let mut out = vec![json!({ "role": "system", "content": req.system })];
    out.extend(req.messages.iter().map(|m| {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        json!({ "role": role, "content": m.text })
    }));
    out
}

fn parse(status: u16, text: &str) -> Result<String, Error> {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    if status != 200 {
        return Err(error(status, text));
    }
    let choice = &v["choices"][0];
    if choice["finish_reason"] == "length" {
        return Err(failed("The answer was cut off: the model hit its output limit."));
    }
    match choice["message"]["content"].as_str().filter(|s| !s.trim().is_empty()) {
        Some(s) => Ok(s.to_string()),
        None => Err(failed("The model returned an empty answer.")),
    }
}

fn error(status: u16, text: &str) -> Error {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    // Gemini wraps errors in a list.
    let e = if v.is_array() { &v[0]["error"] } else { &v["error"] };
    let msg = e["message"].as_str().or(e.as_str()).unwrap_or(text);
    let what = match status {
        401 | 403 => "The API rejected the key",
        404 => "The API doesn't know this model or address",
        429 => "Rate limit or quota reached",
        _ => "The API returned an error",
    };
    failed(format!("{what} ({status}): {}", short(msg)))
}

fn network(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "The API didn't answer in time.".into()
    } else if e.is_connect() {
        "Couldn't reach the API. Check the address and your connection.".into()
    } else {
        format!("The request failed: {e}")
    }
}

/// The first 300 characters, for error messages.
pub(crate) fn short(s: &str) -> String {
    let s = s.trim();
    let mut out: String = s.chars().take(300).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn request() -> Request {
        Request { system: "s".into(), messages: vec![], schema: json!({}) }
    }

    /// A server answering each connection with the next response.
    fn serve(responses: Vec<(u16, &'static str)>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            responses
                .into_iter()
                .map(|(status, body)| {
                    let (mut s, _) = l.accept().unwrap();
                    let mut got = Vec::new();
                    let mut buf = [0; 4096];
                    loop {
                        let n = s.read(&mut buf).unwrap();
                        got.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&got).to_string();
                        if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                            let len: usize = head
                                .lines()
                                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length: ").map(|v| v.parse().unwrap()))
                                .unwrap_or(0);
                            if rest.len() >= len || n == 0 {
                                break;
                            }
                        }
                    }
                    write!(s, "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
                    String::from_utf8_lossy(&got).into_owned()
                })
                .collect()
        });
        (url, h)
    }

    #[test]
    fn falls_back_without_json_schema() {
        let (base_url, h) = serve(vec![
            (400, r#"{"error":{"message":"unsupported response_format"}}"#),
            (200, r#"{"choices":[{"message":{"content":"{}"}}]}"#),
        ]);
        let b = OpenAiCompat { base_url, model: "m".into(), key: "k".into() };
        assert_eq!(b.complete(&request(), &Cancel::default()).unwrap(), "{}");
        let got = h.join().unwrap();
        assert!(got[0].contains("json_schema") && got[1].contains("json_object"));
        assert!(got[0].to_ascii_lowercase().contains("authorization: bearer k"));
    }

    #[test]
    fn lists_chat_models() {
        let (base_url, h) = serve(vec![(200, r#"{"data":[{"id":"models/b"},{"id":"a"},{"id":"text-embedding-1"}]}"#)]);
        let b = OpenAiCompat { base_url, model: String::new(), key: "k".into() };
        let ids: Vec<String> = b.models(&Cancel::default()).unwrap().into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "b"]);
        let got = h.join().unwrap();
        assert!(got[0].starts_with("GET /v1/models") && got[0].contains("Bearer k"));
        assert!(parse_models(401, "{}").is_err() && parse_models(200, "<html>").is_err());
    }

    #[test]
    fn refuses_plain_http_and_reports_errors() {
        let b = OpenAiCompat { base_url: "http://localhost.evil.com/v1".into(), model: "m".into(), key: "k".into() };
        assert!(matches!(b.complete(&request(), &Cancel::default()), Err(Error::Failed(m)) if m.contains("https")));
        assert!(matches!(parse(429, r#"[{"error":{"message":"slow down"}}]"#), Err(Error::Failed(m)) if m.contains("slow down")));
        assert!(parse(200, r#"{"choices":[{"finish_reason":"length","message":{"content":"{"}}]}"#).is_err());
    }

    #[test]
    fn cancel_is_immediate() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); // accepts, never answers
        let b = OpenAiCompat { base_url: format!("http://{}", l.local_addr().unwrap()), model: "m".into(), key: String::new() };
        let cancel = Cancel::default();
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            c.cancel();
        });
        assert_eq!(b.complete(&request(), &cancel), Err(Error::Cancelled));
    }
}
