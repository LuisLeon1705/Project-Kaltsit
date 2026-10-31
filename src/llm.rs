//! OpenAI-compatible client for llama-server (a multimodal model + its mmproj, so it can see).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Llm {
    pub url: String,
    pub model: String,
}

/// One slot for her own voice (chat and what she says about the screen), one for analysis, so
/// each keeps its KV-cached prefix.
pub const SLOT_VOICE: u32 = 0;
pub const SLOT_ANALYSIS: u32 = 1;

/// A chat message. `content` is a string, or an array of parts when it carries an image.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Msg {
    pub role: String,
    pub content: Value,
}

impl Msg {
    pub fn text(role: &str, content: impl Into<String>) -> Self {
        Msg { role: role.into(), content: Value::String(content.into()) }
    }

    /// Text plus one JPEG (base64) for the vision encoder.
    pub fn with_image(role: &str, text: impl Into<String>, jpeg_b64: &str) -> Self {
        Msg {
            role: role.into(),
            content: json!([
                { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{jpeg_b64}") } },
                { "type": "text", "text": text.into() }
            ]),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_text(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
            _ => String::new(),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(300))
        .build()
}

impl Llm {
    pub fn describe(&self) -> String {
        // The model is whichever llama-server loaded (it can change: «Modelos»).
        format!("llama-server · {}", self.url)
    }

    /// Streams her words, calling `on_token` for each piece (return `false` to stop).
    pub fn chat_stream(&self, messages: &[Msg], temperature: f32, max_tokens: u32, mut on_token: impl FnMut(&str) -> bool) -> Result<String, String> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "temperature": temperature, "top_p": 0.9, "max_tokens": max_tokens,
            "repeat_penalty": 1.08,
            "id_slot": SLOT_VOICE, "cache_prompt": true,
            "chat_template_kwargs": { "enable_thinking": false }
        });
        let resp = agent()
            .post(&format!("{}/v1/chat/completions", self.url))
            .send_json(body)
            .map_err(|e| format!("LLM ({}): {e}", self.describe()))?;
        let mut full = String::new();
        for line in BufReader::new(resp.into_reader()).lines() {
            let line = line.map_err(|e| e.to_string())?;
            let Some(data) = line.trim().strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let v: Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
            let piece = v["choices"][0]["delta"]["content"].as_str().unwrap_or("");
            if !piece.is_empty() {
                full.push_str(piece);
                if !on_token(piece) {
                    break;
                }
            }
        }
        Ok(full)
    }

    /// One call constrained to a JSON schema.
    pub fn chat_json(&self, messages: &[Msg], schema: &Value, slot: u32, temperature: f32, max_tokens: u32) -> Result<Value, String> {
        let body = json!({
            "model": self.model, "messages": messages, "stream": false,
            "temperature": temperature, "max_tokens": max_tokens,
            "id_slot": slot, "cache_prompt": true,
            "chat_template_kwargs": { "enable_thinking": false },
            "response_format": { "type": "json_schema", "json_schema": { "name": "salida", "schema": schema } }
        });
        let v: Value = agent()
            .post(&format!("{}/v1/chat/completions", self.url))
            .send_json(body)
            .map_err(|e| format!("LLM ({}): {e}", self.describe()))?
            .into_json()
            .map_err(|e| e.to_string())?;
        let content = v["choices"][0]["message"]["content"].as_str().ok_or("respuesta sin contenido")?;
        let start = content.find('{').ok_or("sin JSON")?;
        let end = content.rfind('}').ok_or("JSON incompleto")?;
        serde_json::from_str(&content[start..=end]).map_err(|e| format!("JSON inválido: {e}"))
    }
}
