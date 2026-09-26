/// Shared STT provider configuration constants.
///
/// Eliminates the triple duplication of endpoint/model/extra_fields across:
/// - `stt::create_provider`
/// - `lib::test_stt_connection`
/// - `lib::bench_stt_connection`
///
use super::whisper_compat::{self, WhisperCompatConfig};

pub const CUSTOM_WHISPER_PROVIDER: &str = "custom-whisper";
pub const CUSTOM_WHISPER_FALLBACK_PROVIDER: &str = "custom-whisper-fallback";
pub const CUSTOM_WHISPER_PRESET_SPEACHES: &str = "speaches";
pub const CUSTOM_WHISPER_PRESET_CUSTOM: &str = "custom";
pub const DEFAULT_CUSTOM_WHISPER_BASE_URL: &str = "http://localhost:8000/v1";
pub const DEFAULT_CUSTOM_WHISPER_MODEL: &str = "Systran/faster-whisper-large-v3";

/// Configuration for a Whisper-compatible STT provider.
#[allow(clippy::doc_lazy_continuation)]
pub struct SttProviderConfig {
    pub endpoint: &'static str,
    pub model: &'static str,
    pub extra_fields: &'static [(&'static str, &'static str)],
}

/// Returns the endpoint, model name, and any extra form fields for a given
/// Whisper-compatible STT provider.
pub fn get_whisper_config(provider: &str) -> Option<SttProviderConfig> {
    match provider {
        "glm-asr" => Some(SttProviderConfig {
            endpoint: "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
            model: "glm-asr-2512",
            extra_fields: &[("stream", "false")],
        }),
        "openai-whisper" => Some(SttProviderConfig {
            endpoint: "https://api.openai.com/v1/audio/transcriptions",
            model: "whisper-1",
            extra_fields: &[],
        }),
        "groq-whisper" => Some(SttProviderConfig {
            endpoint: "https://api.groq.com/openai/v1/audio/transcriptions",
            model: "whisper-large-v3-turbo",
            extra_fields: &[],
        }),
        "siliconflow" => Some(SttProviderConfig {
            endpoint: "https://api.siliconflow.cn/v1/audio/transcriptions",
            model: "FunAudioLLM/SenseVoiceSmall",
            extra_fields: &[],
        }),
        _ => None,
    }
}

pub fn normalize_custom_whisper_endpoint(base_url: &str) -> Result<String, String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("Base URL is required for Local / Custom Whisper".to_string());
    }

    let mut parsed =
        url::Url::parse(trimmed).map_err(|_| "Base URL must be a valid URL".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Base URL must start with http:// or https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Base URL must not include credentials".to_string());
    }
    if parsed.fragment().is_some() {
        return Err("Base URL must not include a fragment".to_string());
    }

    let normalized_path = parsed.path().trim_end_matches('/').to_string();
    if normalized_path.ends_with("/audio/transcriptions") {
        parsed.set_path(&normalized_path);
    } else {
        parsed.set_path(&format!("{normalized_path}/audio/transcriptions"));
    }

    Ok(parsed.to_string())
}

pub fn build_custom_whisper_config(
    base_url: &str,
    model: &str,
) -> Result<WhisperCompatConfig, String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Model is required for Local / Custom Whisper".to_string());
    }

    Ok(WhisperCompatConfig {
        provider_name: CUSTOM_WHISPER_PROVIDER.to_string(),
        endpoint: normalize_custom_whisper_endpoint(base_url)?,
        model: model.to_string(),
        extra_fields: vec![],
        api_key_required: false,
    })
}

/// Build the fallback provider config for the custom OpenAI-compatible STT.
/// Returns `Ok(None)` when no fallback is configured (both fields blank).
pub fn build_custom_whisper_fallback_config(
    base_url: &str,
    model: &str,
) -> Result<Option<WhisperCompatConfig>, String> {
    let base_url = base_url.trim();
    let model = model.trim();
    if base_url.is_empty() && model.is_empty() {
        return Ok(None);
    }
    if base_url.is_empty() {
        return Err("Fallback base URL is required when a fallback model is set".to_string());
    }
    if model.is_empty() {
        return Err("Fallback model is required when a fallback base URL is set".to_string());
    }

    Ok(Some(WhisperCompatConfig {
        provider_name: CUSTOM_WHISPER_FALLBACK_PROVIDER.to_string(),
        endpoint: normalize_custom_whisper_endpoint(base_url)?,
        model: model.to_string(),
        extra_fields: vec![],
        api_key_required: false,
    }))
}

pub fn build_known_whisper_config(provider: &str) -> Option<WhisperCompatConfig> {
    let cfg = get_whisper_config(provider)?;
    Some(WhisperCompatConfig {
        provider_name: provider.to_string(),
        endpoint: cfg.endpoint.to_string(),
        model: cfg.model.to_string(),
        extra_fields: cfg
            .extra_fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        api_key_required: true,
    })
}

/// Build the request policy (timeout + fallback) for the custom
/// OpenAI-compatible STT from app config. Shared by the dictation pipeline,
/// the Ask flow, and the history re-transcription command.
pub fn build_custom_whisper_request_policy(
    config: &crate::storage::AppConfig,
    fallback_api_key: Option<String>,
) -> Result<whisper_compat::WhisperCompatRequestPolicy, String> {
    let fallback = build_custom_whisper_fallback_config(
        &config.stt_custom_fallback_base_url,
        &config.stt_custom_fallback_model,
    )?;
    Ok(whisper_compat::WhisperCompatRequestPolicy {
        timeout_secs: config.stt_request_timeout_secs(),
        fallback,
        fallback_api_key,
    })
}

pub fn stt_provider_requires_api_key(provider: &str) -> bool {
    !matches!(provider, CUSTOM_WHISPER_PROVIDER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glm_asr_config() {
        let cfg = get_whisper_config("glm-asr").unwrap();
        assert!(cfg.endpoint.contains("bigmodel.cn"));
        assert_eq!(cfg.model, "glm-asr-2512");
        assert!(cfg.extra_fields.contains(&("stream", "false")));
    }

    #[test]
    fn test_openai_whisper_config() {
        let cfg = get_whisper_config("openai-whisper").unwrap();
        assert!(cfg.endpoint.contains("api.openai.com"));
        assert_eq!(cfg.model, "whisper-1");
        assert!(cfg.extra_fields.is_empty());
    }

    #[test]
    fn test_groq_whisper_config() {
        let cfg = get_whisper_config("groq-whisper").unwrap();
        assert!(cfg.endpoint.contains("api.groq.com"));
        assert_eq!(cfg.model, "whisper-large-v3-turbo");
        assert!(cfg.extra_fields.is_empty());
    }

    #[test]
    fn test_siliconflow_config() {
        let cfg = get_whisper_config("siliconflow").unwrap();
        assert!(cfg.endpoint.contains("siliconflow"));
        assert_eq!(cfg.model, "FunAudioLLM/SenseVoiceSmall");
        assert!(cfg.extra_fields.is_empty());
    }

    #[test]
    fn test_unknown_provider_returns_none() {
        assert!(get_whisper_config("unknown").is_none());
    }

    #[test]
    fn test_deepgram_not_in_whisper_config() {
        assert!(get_whisper_config("deepgram").is_none());
    }

    #[test]
    fn test_assemblyai_not_in_whisper_config() {
        assert!(get_whisper_config("assemblyai").is_none());
    }

    #[test]
    fn custom_whisper_is_keyless_and_unknown_providers_are_keyed() {
        assert!(!stt_provider_requires_api_key(CUSTOM_WHISPER_PROVIDER));
        assert!(stt_provider_requires_api_key("deepgram"));
    }

    #[test]
    fn test_normalize_custom_whisper_base_url() {
        let endpoint = normalize_custom_whisper_endpoint("http://localhost:8000/v1").unwrap();
        assert_eq!(endpoint, "http://localhost:8000/v1/audio/transcriptions");
    }

    #[test]
    fn test_normalize_custom_whisper_full_endpoint() {
        let endpoint =
            normalize_custom_whisper_endpoint("http://localhost:8000/v1/audio/transcriptions")
                .unwrap();
        assert_eq!(endpoint, "http://localhost:8000/v1/audio/transcriptions");
    }

    #[test]
    fn custom_whisper_appends_transcription_path_before_query() {
        let endpoint =
            normalize_custom_whisper_endpoint("https://example.com/openai?api-version=2026-01-01")
                .unwrap();

        assert_eq!(
            endpoint,
            "https://example.com/openai/audio/transcriptions?api-version=2026-01-01"
        );
    }

    #[test]
    fn custom_whisper_preserves_query_on_full_endpoint() {
        let endpoint = normalize_custom_whisper_endpoint(
            "https://example.com/v1/audio/transcriptions?api-version=2026-01-01",
        )
        .unwrap();

        assert_eq!(
            endpoint,
            "https://example.com/v1/audio/transcriptions?api-version=2026-01-01"
        );
    }

    #[test]
    fn custom_whisper_rejects_embedded_credentials_and_fragments() {
        let credentials =
            normalize_custom_whisper_endpoint("https://user:secret@example.com/v1").unwrap_err();
        let fragment =
            normalize_custom_whisper_endpoint("https://example.com/v1#section").unwrap_err();

        assert!(credentials.contains("credentials"));
        assert!(fragment.contains("fragment"));
    }

    #[test]
    fn test_custom_whisper_rejects_empty_base_url() {
        let err = normalize_custom_whisper_endpoint("   ").unwrap_err();
        assert!(err.contains("Base URL is required"));
    }

    #[test]
    fn test_custom_whisper_rejects_non_http_url() {
        let err = normalize_custom_whisper_endpoint("file:///tmp/server").unwrap_err();
        assert!(err.contains("http://"));
    }

    #[test]
    fn test_build_custom_whisper_config() {
        let cfg = build_custom_whisper_config(
            "http://localhost:8000/v1",
            "Systran/faster-whisper-large-v3",
        )
        .unwrap();
        assert_eq!(cfg.provider_name, CUSTOM_WHISPER_PROVIDER);
        assert_eq!(
            cfg.endpoint,
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(cfg.model, "Systran/faster-whisper-large-v3");
        assert!(!cfg.api_key_required);
    }

    #[test]
    fn test_build_custom_whisper_config_requires_model() {
        let err = build_custom_whisper_config("http://localhost:8000/v1", "  ").unwrap_err();
        assert!(err.contains("Model is required"));
    }

    #[test]
    fn fallback_config_is_none_when_both_fields_blank() {
        assert!(build_custom_whisper_fallback_config("", "")
            .unwrap()
            .is_none());
        assert!(build_custom_whisper_fallback_config("  ", " ")
            .unwrap()
            .is_none());
    }

    #[test]
    fn fallback_config_requires_both_fields() {
        let err = build_custom_whisper_fallback_config("", "some-model").unwrap_err();
        assert!(err.contains("Fallback base URL is required"));

        let err = build_custom_whisper_fallback_config("http://localhost:9000/v1", "").unwrap_err();
        assert!(err.contains("Fallback model is required"));
    }

    #[test]
    fn fallback_config_normalizes_endpoint() {
        let cfg = build_custom_whisper_fallback_config(
            "https://openrouter.ai/api/v1",
            "openai/whisper-large-v3",
        )
        .unwrap()
        .expect("fallback should be built");
        assert_eq!(cfg.provider_name, CUSTOM_WHISPER_FALLBACK_PROVIDER);
        assert_eq!(
            cfg.endpoint,
            "https://openrouter.ai/api/v1/audio/transcriptions"
        );
        assert_eq!(cfg.model, "openai/whisper-large-v3");
    }
}
