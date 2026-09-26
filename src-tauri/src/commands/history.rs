use crate::credentials::{
    resolve_llm_config_secret, resolve_stt_config_secret, SystemCredentialVault,
};
use crate::llm::{self, LlmConfig, PolishRequest};
use crate::storage;
use crate::stt::{self, whisper_compat::WhisperCompatProvider, SttConfig, SttProvider};

/// Maximum accepted size for a persisted failed recording (24 MB, matching
/// the in-memory PCM cap of the Whisper-compatible providers).
const MAX_PENDING_AUDIO_BYTES: usize = 24 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrySttOutcome {
    pub success: bool,
    pub raw_text: Option<String>,
    pub polished_text: Option<String>,
    pub error: Option<String>,
    /// When true the failed recording was kept on disk and can be retried again.
    pub retryable: bool,
}

fn build_retry_stt_config(config: &storage::AppConfig, api_key: String) -> SttConfig {
    SttConfig {
        api_key,
        language: if config.stt_language == "multi" {
            None
        } else {
            Some(config.stt_language.clone())
        },
        smart_format: true,
        sample_rate: 16000,
        resource_id: (config.stt_provider == stt::volcengine::VOLCENGINE_DOUBAO_PROVIDER)
            .then(|| config.stt_volcengine_resource_id.clone()),
        provider_region: (config.stt_provider == stt::aliyun_qwen3_asr::ALIYUN_QWEN3_ASR_PROVIDER)
            .then(|| config.stt_aliyun_qwen_region.clone()),
    }
}

/// Re-transcribe a persisted failed recording and, on success, run AI polish
/// (when enabled) and update the history entry. The polished result is NOT
/// auto-inserted into any application — the caller copies it from the history
/// pane. On failure the pending audio is kept so the entry stays retryable.
#[tauri::command]
pub async fn retry_history_stt(
    app: tauri::AppHandle,
    state: tauri::State<'_, storage::HistoryStore>,
    config_state: tauri::State<'_, storage::ConfigManager>,
    client: tauri::State<'_, reqwest::Client>,
    entry_id: i64,
) -> Result<RetrySttOutcome, String> {
    let config = config_state.load().await.map_err(|e| e.to_string())?;
    let Some(entry) = state.get(entry_id).await.map_err(|e| e.to_string())? else {
        return Err("History entry not found".to_string());
    };
    let Some(ref audio_path) = entry.pending_audio_path else {
        return Err("This entry has no retryable recording".to_string());
    };

    let audio_path = std::path::PathBuf::from(audio_path);
    let wav_data = match stt::failed_audio::read_failed_recording(&audio_path) {
        Ok(data) if data.len() <= MAX_PENDING_AUDIO_BYTES && data.len() > 44 => data,
        Ok(_) => {
            // Missing/corrupt audio: clear the pending path so the UI stops
            // offering a retry that can never succeed.
            let _ = state
                .update_retry_failure(entry_id, "recorded audio is no longer available")
                .await;
            return Err("Recorded audio is no longer available".to_string());
        }
        Err(error) => {
            let _ = state
                .update_retry_failure(entry_id, "recorded audio is no longer available")
                .await;
            return Err(format!("Failed to read recorded audio: {error}"));
        }
    };

    // Primary transcription attempt using the persisted WAV as-is.
    let transcript = transcribe_wav(&config, &client, &wav_data).await;
    let raw_text = match transcript {
        Ok(text) => text,
        Err(error) => {
            let message = error.to_string();
            let _ = state.update_retry_failure(entry_id, &message).await;
            return Ok(RetrySttOutcome {
                success: false,
                raw_text: None,
                polished_text: None,
                error: Some(message),
                retryable: true,
            });
        }
    };

    // AI polish (when enabled) — same provider decision as the dictation flow.
    let polished_text = polish_retry_text(&config, &app, &raw_text).await;

    state
        .update_retry_success(entry_id, &raw_text, &polished_text)
        .await
        .map_err(|e| e.to_string())?;
    stt::failed_audio::delete_failed_recording(&audio_path);

    Ok(RetrySttOutcome {
        success: true,
        raw_text: Some(raw_text),
        polished_text: Some(polished_text),
        error: None,
        retryable: false,
    })
}

/// Transcribe a complete WAV payload through the currently configured provider.
/// For the custom OpenAI-compatible provider the fallback policy applies too.
async fn transcribe_wav(
    config: &storage::AppConfig,
    client: &reqwest::Client,
    _wav_data: &[u8],
) -> Result<String, anyhow::Error> {
    let api_key = resolve_stt_config_secret(config, &SystemCredentialVault).unwrap_or_default();
    if stt::config::stt_provider_requires_api_key(&config.stt_provider) && api_key.is_empty() {
        anyhow::bail!("STT API key is not configured");
    }

    let stt_config = build_retry_stt_config(config, api_key);
    let mut provider = match config.stt_provider.as_str() {
        stt::config::CUSTOM_WHISPER_PROVIDER => {
            let primary = stt::config::build_custom_whisper_config(
                &config.stt_custom_base_url,
                &config.stt_custom_model,
            )
            .map_err(anyhow::Error::msg)?;
            let fallback_api_key = if config.stt_custom_fallback_api_key.trim().is_empty() {
                crate::credentials::resolve_stt_fallback_secret(&SystemCredentialVault)
                    .unwrap_or_default()
            } else {
                config.stt_custom_fallback_api_key.clone()
            };
            let fallback_api_key =
                (!fallback_api_key.trim().is_empty()).then_some(fallback_api_key);
            let policy = stt::config::build_custom_whisper_request_policy(config, fallback_api_key)
                .map_err(anyhow::Error::msg)?;
            Box::new(WhisperCompatProvider::with_policy(
                primary,
                policy,
                client.clone(),
            )) as Box<dyn SttProvider>
        }
        "deepgram"
        | "assemblyai"
        | stt::volcengine::VOLCENGINE_DOUBAO_PROVIDER
        | stt::aliyun_qwen3_asr::ALIYUN_QWEN3_ASR_PROVIDER => {
            anyhow::bail!(
                "Re-transcription is not available for streaming provider entries; only file-upload providers are supported"
            );
        }
        _ => {
            let wc = stt::config::build_known_whisper_config(&config.stt_provider)
                .ok_or_else(|| anyhow::anyhow!("Unknown STT provider: {}", config.stt_provider))?;
            Box::new(WhisperCompatProvider::with_client(wc, client.clone())) as Box<dyn SttProvider>
        }
    };

    provider.connect(&stt_config).await?;
    let transcript = provider.disconnect().await?;
    transcript.ok_or_else(|| anyhow::anyhow!("No speech detected in the recorded audio"))
}

/// Run the polish step for a re-transcribed text using the current LLM config.
/// Falls back to the raw transcription when polish is disabled or fails.
async fn polish_retry_text(
    config: &storage::AppConfig,
    app: &tauri::AppHandle,
    raw_text: &str,
) -> String {
    let llm_api_key = resolve_llm_config_secret(config, &SystemCredentialVault).unwrap_or_default();

    if !config.polish_enabled
        || !llm::has_usable_provider_credentials(&config.llm_provider, &llm_api_key)
    {
        return raw_text.to_string();
    }

    let llm_config = LlmConfig {
        provider: config.llm_provider.clone(),
        api_key: llm_api_key,
        model: config.llm_model.clone(),
        base_url: config.llm_base_url.clone(),
        max_tokens: config.polish_max_tokens.max(16),
        temperature: config.polish_temperature,
        system_prompt_append: config.polish_system_prompt_append.clone(),
        request_overrides: config.polish_request_overrides().unwrap_or_default(),
        timeout_secs: config.llm_request_timeout_secs,
    };
    let provider = llm::create_provider(&config.llm_provider, None);
    let req = PolishRequest {
        raw_text: raw_text.to_string(),
        context: crate::app_detector::types::ContextProfileSummary {
            profile_id: "general.retry".to_string(),
            family: crate::app_detector::types::ContextFamily::General,
            app_label: "Re-transcribed".to_string(),
            icon_key: "general".to_string(),
            override_id: None,
            browser_access_status: crate::app_detector::types::BrowserAccessStatus::NotApplicable,
            browser_target: None,
        },
        dictionary: Vec::new(),
        correction_rules: Vec::new(),
        polish_style: config.polish_style.clone(),
        mapped_scene_prompt: String::new(),
        active_scene_prompt: config
            .active_scene
            .as_ref()
            .map(|scene| scene.prompt_template.clone())
            .unwrap_or_default(),
        polish_custom_prompt: config.polish_custom_prompt.clone(),
        translate_enabled: config.translate_enabled,
        target_lang: config.target_lang.clone(),
        selected_text: None,
        voice_intent: crate::voice_intent::VoiceIntent {
            kind: crate::voice_intent::VoiceIntentKind::DictateInsert,
            placement: crate::voice_intent::VoiceOutputPlacement::InsertAtCursor,
            confidence: 1.0,
            search_provider: None,
            payload: None,
            grammar_locale: None,
            fallback_reason: None,
        },
    };

    match provider.polish(&llm_config, &req, None).await {
        Ok(response) => response.polished_text,
        Err(error) => {
            tracing::warn!("Re-transcription polish failed, keeping raw text: {error}");
            let _ = app;
            raw_text.to_string()
        }
    }
}

#[tauri::command]
pub async fn get_history(
    state: tauri::State<'_, storage::HistoryStore>,
    limit: u32,
    offset: u32,
) -> Result<Vec<storage::HistoryEntry>, String> {
    state.list(limit, offset).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_history(state: tauri::State<'_, storage::HistoryStore>) -> Result<(), String> {
    state.clear().await.map_err(|e| e.to_string())
}
