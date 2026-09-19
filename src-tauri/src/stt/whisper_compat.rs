use async_trait::async_trait;

use crate::error::AppError;

use super::{SttConfig, SttProvider, TranscriptEvent};

/// Configuration for a Whisper-compatible HTTP file-upload STT provider.
#[derive(Debug, Clone)]
pub struct WhisperCompatConfig {
    pub provider_name: String,
    pub endpoint: String,
    pub model: String,
    /// Extra form text fields (e.g. GLM-ASR needs "stream"="false").
    pub extra_fields: Vec<(String, String)>,
    /// Local OpenAI-compatible servers often do not require authentication.
    pub api_key_required: bool,
}

/// Per-provider request settings applied on top of `WhisperCompatConfig`.
#[derive(Debug, Clone)]
pub struct WhisperCompatRequestPolicy {
    /// Total wall-clock budget for one provider attempt chain (including its
    /// internal 5xx/429 retries). Measured from the end of recording, when
    /// `disconnect()` starts the transcription request.
    pub timeout_secs: u64,
    /// Fallback provider used when the primary attempt chain fails.
    pub fallback: Option<WhisperCompatConfig>,
    /// API key used for the fallback provider. When empty the primary key is
    /// reused (convenient for OpenRouter-style routers where switching models
    /// does not require a different key).
    pub fallback_api_key: Option<String>,
}

impl Default for WhisperCompatRequestPolicy {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_STT_REQUEST_TIMEOUT_SECS,
            fallback: None,
            fallback_api_key: None,
        }
    }
}

/// Default per-provider request budget, in seconds, counted from the end of
/// recording. Exposed as a tunable setting (`stt_request_timeout_secs`).
pub const DEFAULT_STT_REQUEST_TIMEOUT_SECS: u64 = 8;
pub const MIN_STT_REQUEST_TIMEOUT_SECS: u64 = 5;
pub const MAX_STT_REQUEST_TIMEOUT_SECS: u64 = 120;

/// Clamp a user-provided request timeout into the supported range.
pub fn clamp_stt_request_timeout_secs(value: u64) -> u64 {
    value.clamp(MIN_STT_REQUEST_TIMEOUT_SECS, MAX_STT_REQUEST_TIMEOUT_SECS)
}

/// Max audio buffer: ~24 MB PCM ≈ 12.5 min at 16kHz 16-bit mono.
/// Keeps the resulting WAV under 25 MB (OpenAI/Groq limit).
const MAX_AUDIO_BYTES: usize = 24 * 1024 * 1024;

/// Number of internal attempts per provider for 5xx/429/network errors.
const ATTEMPTS_PER_PROVIDER: u32 = 3;

/// Build a WAV file from raw PCM 16-bit mono audio. Public so test helpers can reuse it.
pub fn build_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32) / 8;
    let block_align = channels * bits_per_sample / 8;
    let file_size = 36 + data_len;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&bits_per_sample.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

/// Detect all-zero PCM payloads so known-silent recordings skip the network
/// entirely. Real microphones still produce dithering noise, so a small
/// tolerance would only matter for pathological input; an exact match is safe.
fn is_silent_pcm(pcm: &[u8]) -> bool {
    pcm.iter().all(|&b| b == 0)
}

fn truncate_body(body: &str) -> &str {
    let truncate_at = body
        .char_indices()
        .take_while(|&(i, _)| i < 200)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(body.len());
    &body[..truncate_at]
}

struct ProviderAttempt<'a> {
    provider_config: &'a WhisperCompatConfig,
    api_key: &'a str,
}

/// Generic provider for any OpenAI Whisper-compatible transcription API.
/// Works with: OpenAI, Groq, SiliconFlow, GLM-ASR.
pub struct WhisperCompatProvider {
    provider_config: WhisperCompatConfig,
    request_policy: WhisperCompatRequestPolicy,
    stt_config: Option<SttConfig>,
    audio_buffer: Vec<u8>,
    client: reqwest::Client,
}

impl WhisperCompatProvider {
    pub fn new(provider_config: WhisperCompatConfig) -> Self {
        Self {
            provider_config,
            request_policy: WhisperCompatRequestPolicy::default(),
            stt_config: None,
            audio_buffer: Vec::new(),
            client: reqwest::Client::new(),
        }
    }

    pub fn with_client(provider_config: WhisperCompatConfig, client: reqwest::Client) -> Self {
        Self::with_policy(
            provider_config,
            WhisperCompatRequestPolicy::default(),
            client,
        )
    }

    pub fn with_policy(
        provider_config: WhisperCompatConfig,
        request_policy: WhisperCompatRequestPolicy,
        client: reqwest::Client,
    ) -> Self {
        Self {
            provider_config,
            request_policy,
            stt_config: None,
            audio_buffer: Vec::new(),
            client,
        }
    }

    /// Replace the request policy in place (builder-style convenience).
    pub fn with_policy_owned(mut self, request_policy: WhisperCompatRequestPolicy) -> Self {
        self.request_policy = request_policy;
        self
    }

    /// Build a WAV file from raw PCM 16-bit mono audio. Public so test helpers can reuse it.
    pub fn build_wav(pcm: &[u8], sample_rate: u32) -> Vec<u8> {
        build_wav(pcm, sample_rate)
    }

    /// Transcribe a complete WAV payload against one provider, retrying
    /// transient failures within the provider's request budget.
    async fn transcribe_wav_with_provider(
        &self,
        provider: &ProviderAttempt<'_>,
        wav_data: &[u8],
        language: Option<&str>,
        timeout_secs: u64,
    ) -> Result<Option<String>, AppError> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        let mut attempt = 0u32;
        loop {
            attempt += 1;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(AppError::Network(format!(
                    "{} request timed out after {}s (request budget exhausted)",
                    provider.provider_config.provider_name, timeout_secs
                )));
            }

            let file_part = reqwest::multipart::Part::bytes(wav_data.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .map_err(|e| AppError::Config(e.to_string()))?;

            let mut form = reqwest::multipart::Form::new()
                .text("model", provider.provider_config.model.clone())
                .part("file", file_part);

            // Language hint (OpenAI/Groq support `language` field, others use `prompt`)
            if let Some(lang) = language {
                if lang != "multi" {
                    form = form.text("language", lang.to_string());
                }
            }

            // Provider-specific extra fields
            for (key, value) in &provider.provider_config.extra_fields {
                form = form.text(key.clone(), value.clone());
            }

            let mut request = self
                .client
                .post(&provider.provider_config.endpoint)
                .multipart(form)
                .timeout(remaining);

            if !provider.api_key.trim().is_empty() {
                request = request.header("Authorization", format!("Bearer {}", provider.api_key));
            }

            match request.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();

                    if status.is_success() {
                        let v: serde_json::Value = serde_json::from_str(&body)
                            .map_err(|e| AppError::Config(e.to_string()))?;
                        let text = v["text"].as_str().unwrap_or("").trim().to_string();

                        tracing::info!(
                            "{} transcription: {} chars",
                            provider.provider_config.provider_name,
                            text.len()
                        );

                        return Ok(if text.is_empty() { None } else { Some(text) });
                    } else if (status.as_u16() >= 500 || status.as_u16() == 429)
                        && attempt < ATTEMPTS_PER_PROVIDER
                    {
                        let truncated = truncate_body(&body);
                        tracing::warn!(
                            "{} transient HTTP {} (attempt {}/{}): {}",
                            provider.provider_config.provider_name,
                            status,
                            attempt,
                            ATTEMPTS_PER_PROVIDER,
                            truncated
                        );
                        let backoff = 1000u64 * 2u64.pow(attempt - 1);
                        let backoff = std::time::Duration::from_millis(backoff);
                        let remaining =
                            deadline.saturating_duration_since(tokio::time::Instant::now());
                        tokio::time::sleep(backoff.min(remaining)).await;
                        continue;
                    } else {
                        let truncated = truncate_body(&body);
                        tracing::error!(
                            "{} HTTP {}: {}",
                            provider.provider_config.provider_name,
                            status,
                            truncated
                        );
                        return Err(AppError::Api {
                            status: status.as_u16(),
                            body: truncated.to_string(),
                        });
                    }
                }
                Err(e) if e.is_timeout() => {
                    return Err(AppError::Network(format!(
                        "{} request timed out after {}s",
                        provider.provider_config.provider_name, timeout_secs
                    )));
                }
                Err(e) if attempt < ATTEMPTS_PER_PROVIDER => {
                    tracing::warn!(
                        "{} network error (attempt {}/{}): {}",
                        provider.provider_config.provider_name,
                        attempt,
                        ATTEMPTS_PER_PROVIDER,
                        e
                    );
                    let backoff = 1000u64 * 2u64.pow(attempt - 1);
                    let backoff = std::time::Duration::from_millis(backoff);
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(AppError::Network(format!(
                            "{} request timed out after {}s (request budget exhausted)",
                            provider.provider_config.provider_name, timeout_secs
                        )));
                    }
                    tokio::time::sleep(backoff.min(remaining)).await;
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Transcribe the buffered audio through the primary provider first and,
    /// on failure, through the configured fallback provider. Returns the
    /// transcription result plus the provider the audio was persisted for
    /// (`None` on success — nothing needs to be kept).
    async fn transcribe_with_fallback(
        &self,
        wav_data: &[u8],
        config: &SttConfig,
        audio_len_secs: f64,
    ) -> (
        Result<Option<String>, AppError>,
        Option<&WhisperCompatConfig>,
    ) {
        let primary = ProviderAttempt {
            provider_config: &self.provider_config,
            api_key: &config.api_key,
        };
        let result = self
            .transcribe_wav_with_provider(
                &primary,
                wav_data,
                config.language.as_deref(),
                self.request_policy.timeout_secs,
            )
            .await;

        match result {
            Ok(text) => (Ok(text), None),
            Err(primary_error) => {
                let Some(ref fallback_config) = self.request_policy.fallback else {
                    return (Err(primary_error), None);
                };

                tracing::warn!(
                    "{} failed ({}); retrying with fallback provider '{}'",
                    self.provider_config.provider_name,
                    primary_error,
                    fallback_config.provider_name
                );

                let fallback_api_key = self
                    .request_policy
                    .fallback_api_key
                    .as_deref()
                    .filter(|key| !key.trim().is_empty())
                    .unwrap_or(&config.api_key);
                let fallback = ProviderAttempt {
                    provider_config: fallback_config,
                    api_key: fallback_api_key,
                };
                let fallback_result = self
                    .transcribe_wav_with_provider(
                        &fallback,
                        wav_data,
                        config.language.as_deref(),
                        self.request_policy.timeout_secs,
                    )
                    .await;

                match fallback_result {
                    Ok(text) => {
                        tracing::info!(
                            "Fallback provider '{}' transcribed {:.1}s of audio successfully",
                            fallback_config.provider_name,
                            audio_len_secs
                        );
                        (Ok(text), None)
                    }
                    Err(fallback_error) => (Err(fallback_error), Some(fallback_config)),
                }
            }
        }
    }
}

#[async_trait]
impl SttProvider for WhisperCompatProvider {
    async fn connect(&mut self, config: &SttConfig) -> Result<(), AppError> {
        if self.provider_config.api_key_required && config.api_key.is_empty() {
            return Err(AppError::Auth(format!(
                "{} API key is empty",
                self.provider_config.provider_name
            )));
        }
        self.stt_config = Some(config.clone());
        self.audio_buffer.clear();
        tracing::info!(
            "{} provider ready (buffering mode)",
            self.provider_config.provider_name
        );
        Ok(())
    }

    async fn send_audio(&mut self, chunk: &[u8]) -> Result<(), AppError> {
        if self.audio_buffer.len() + chunk.len() > MAX_AUDIO_BYTES {
            return Err(AppError::Config(format!(
                "{}: audio exceeds maximum length (~12 min)",
                self.provider_config.provider_name
            )));
        }
        self.audio_buffer.extend_from_slice(chunk);
        Ok(())
    }

    async fn recv_transcript(&mut self) -> Result<Option<TranscriptEvent>, AppError> {
        // File-based providers transcribe in disconnect(); keep this future
        // pending so the pipeline select loop does not busy-spin while recording.
        std::future::pending().await
    }

    async fn disconnect(&mut self) -> Result<Option<String>, AppError> {
        let config = match &self.stt_config {
            Some(c) => c.clone(),
            None => return Ok(None),
        };

        if self.audio_buffer.is_empty() {
            tracing::info!(
                "{}: no audio buffered, skipping",
                self.provider_config.provider_name
            );
            return Ok(None);
        }

        let audio_len_secs = self.audio_buffer.len() as f64 / (config.sample_rate as f64 * 2.0);
        let silent = is_silent_pcm(&self.audio_buffer);
        let wav_data = if silent {
            tracing::info!(
                "{}: {:.1}s of buffered audio is silent, skipping transcription request",
                self.provider_config.provider_name,
                audio_len_secs
            );
            Vec::new()
        } else {
            build_wav(&self.audio_buffer, config.sample_rate)
        };
        self.audio_buffer.clear();

        if silent {
            return Ok(None);
        }

        tracing::info!(
            "{}: sending {:.1}s of audio for transcription",
            self.provider_config.provider_name,
            audio_len_secs
        );

        let (result, failed_fallback) = self
            .transcribe_with_fallback(&wav_data, &config, audio_len_secs)
            .await;

        match result {
            Ok(text) => Ok(text),
            Err(error) => {
                // Keep the failed recording on disk so the history entry can be
                // retried later (provider outages / model load spikes pass).
                let persisted = super::failed_audio::persist_failed_recording(&wav_data);
                if let Some(path) = &persisted {
                    tracing::warn!(
                        "STT failed; recording ({:.1}s) kept at {} for retry",
                        audio_len_secs,
                        path.display()
                    );
                    return Err(AppError::SttFailedWithAudio {
                        source: Box::new(error),
                        audio_path: path.clone(),
                        audio_len_secs,
                        fallback_attempted: failed_fallback.is_some(),
                    });
                }
                Err(error)
            }
        }
    }

    fn name(&self) -> &str {
        &self.provider_config.provider_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stt_config(api_key: &str) -> SttConfig {
        SttConfig {
            api_key: api_key.to_string(),
            language: None,
            smart_format: true,
            sample_rate: 16000,
            resource_id: None,
            operation_id: None,
            managed_audio: None,
            provider_region: None,
        }
    }

    #[tokio::test]
    async fn connect_allows_empty_api_key_when_not_required() {
        let mut provider = WhisperCompatProvider::new(WhisperCompatConfig {
            provider_name: "custom-whisper".to_string(),
            endpoint: "http://localhost:8000/v1/audio/transcriptions".to_string(),
            model: "test-model".to_string(),
            extra_fields: vec![],
            api_key_required: false,
        });

        let result = provider.connect(&stt_config("")).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn recv_transcript_waits_for_file_based_provider() {
        let mut provider = WhisperCompatProvider::new(WhisperCompatConfig {
            provider_name: "test-whisper".to_string(),
            endpoint: "https://example.test/transcriptions".to_string(),
            model: "test-model".to_string(),
            extra_fields: vec![],
            api_key_required: true,
        });

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(20),
            provider.recv_transcript(),
        )
        .await;

        assert!(result.is_err());
    }

    #[test]
    fn default_request_policy_uses_default_timeout_without_fallback() {
        let policy = WhisperCompatRequestPolicy::default();
        assert_eq!(policy.timeout_secs, DEFAULT_STT_REQUEST_TIMEOUT_SECS);
        assert!(policy.fallback.is_none());
    }

    #[test]
    fn clamps_request_timeout_into_supported_range() {
        assert_eq!(
            clamp_stt_request_timeout_secs(0),
            MIN_STT_REQUEST_TIMEOUT_SECS
        );
        assert_eq!(
            clamp_stt_request_timeout_secs(1),
            MIN_STT_REQUEST_TIMEOUT_SECS
        );
        assert_eq!(clamp_stt_request_timeout_secs(8), 8);
        assert_eq!(
            clamp_stt_request_timeout_secs(10_000),
            MAX_STT_REQUEST_TIMEOUT_SECS
        );
    }

    #[test]
    fn silent_pcm_is_detected() {
        assert!(is_silent_pcm(&[0u8; 3200]));
        let non_silent = [0u8; 3199];
        let mut with_marker = non_silent.to_vec();
        with_marker.push(1);
        assert!(!is_silent_pcm(&with_marker));
        assert!(is_silent_pcm(&[]));
    }

    #[test]
    fn wav_header_reports_payload_length() {
        let pcm = vec![0u8; 3200];
        let wav = build_wav(&pcm, 16000);
        assert_eq!(wav.len(), 44 + 3200);
        // data chunk length at offset 40
        let data_len = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(data_len, 3200);
    }
}
