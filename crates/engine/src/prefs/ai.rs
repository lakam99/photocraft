//! Non-secret settings for the optional native AI panel. Keys belong to the OS vault.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Ai {
    #[serde(deserialize_with = "model")]
    pub model: String,
    pub attach_frames: bool,
    pub remember_key: bool,
    pub remember_history: bool,
}

pub fn valid_model(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|c| c.is_ascii_alphanumeric() || b"-._/".contains(&c))
}

fn model<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if valid_model(&value) { Ok(value) } else { Err(serde::de::Error::custom("Invalid AI model identifier")) }
}

impl Default for Ai {
    fn default() -> Self {
        Self { model: "gpt-6.1-sol".into(), attach_frames: true, remember_key: true, remember_history: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ai_preferences_roundtrip_and_reject_secrets_and_invalid_models() {
        let mut session = crate::Session::new();
        assert_eq!(session.prefs().ai, Ai::default());
        session.execute("prefs.set", json!({"path":"ai.model","value":"gpt-test"})).unwrap();
        session.execute("prefs.set", json!({"path":"ai.attachFrames","value":false})).unwrap();
        let saved = session.prefs_value();
        let mut reopened = crate::Session::new();
        reopened.load_prefs_json(&saved.to_string()).unwrap();
        assert_eq!(reopened.prefs().ai, session.prefs().ai);
        for value in [json!(""), json!("../ bad"), json!("x".repeat(129)), json!(42), json!(null)] {
            assert!(session.execute("prefs.set", json!({"path":"ai.model","value":value})).is_err());
            assert_eq!(session.prefs().ai.model, "gpt-test");
        }
        assert!(session.execute("prefs.set", json!({"path":"ai.apiKey","value":"not-a-real-key"})).is_err());
        assert!(!session.prefs_value().to_string().contains("not-a-real-key"));
    }
}
