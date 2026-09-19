//! Persistence for failed STT recordings.
//!
//! When every configured transcription attempt fails, the recorded audio is
//! written under `<app data dir>/failed-recordings/<id>.wav` and its path is
//! attached to the surfaced `AppError` (see `AppError::SttFailedWithAudio`) so
//! the pipeline can store it on the history entry. The history pane then
//! offers "re-transcribe" which replays the saved audio through the (possibly
//! recovered) provider.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// How long orphaned failed recordings may stay on disk before cleanup.
pub const FAILED_AUDIO_RETENTION_DAYS: i64 = 7;

static FAILED_AUDIO_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Point the failed-recordings directory at the platform app-data directory.
/// Called once during app setup, before any recording can fail.
/// Returns the previous override (tests use this to restore state).
pub fn set_failed_audio_dir(dir: PathBuf) -> Option<PathBuf> {
    FAILED_AUDIO_DIR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(dir)
}

fn failed_audio_dir() -> PathBuf {
    FAILED_AUDIO_DIR
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_else(|| PathBuf::from("failed-recordings"))
}

fn generate_recording_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("stt-{now:032x}.wav")
}

/// Write the WAV payload to the failed-recordings directory. Best effort:
/// failures return `None` and the caller continues without a retryable copy.
pub fn persist_failed_recording(wav_data: &[u8]) -> Option<PathBuf> {
    if wav_data.is_empty() {
        return None;
    }
    let dir = failed_audio_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!(
            "Failed to create failed-recordings dir {}: {error}",
            dir.display()
        );
        return None;
    }
    let path = dir.join(generate_recording_id());
    match std::fs::write(&path, wav_data) {
        Ok(()) => Some(path),
        Err(error) => {
            tracing::warn!("Failed to persist failed recording: {error}");
            None
        }
    }
}

/// Read a persisted failed recording back for a retry.
pub fn read_failed_recording(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Delete a persisted failed recording (after a successful retry, or when the
/// history entry was removed). Missing files are treated as success.
pub fn delete_failed_recording(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to delete failed recording {}: {error}",
                path.display()
            );
        }
    }
}

/// Remove orphaned failed recordings older than the retention window.
/// Called on app startup; failures are logged and ignored.
pub fn cleanup_failed_recordings() {
    let dir = failed_audio_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let cutoff_ms = (chrono::Local::now() - chrono::Duration::days(FAILED_AUDIO_RETENTION_DAYS))
        .timestamp_millis();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wav") {
            continue;
        }
        let modified_ms = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since_epoch| since_epoch.as_millis() as i64);
        if modified_ms.is_some_and(|modified_ms| modified_ms < cutoff_ms) {
            delete_failed_recording(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that mutate the global failed-recordings directory.
    static DIR_LOCK: Mutex<()> = Mutex::new(());

    fn temp_test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "opentypeless-test-failed-audio-{label}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn persist_and_read_roundtrip() {
        let _guard = DIR_LOCK.lock().unwrap();
        let dir = temp_test_dir("roundtrip");
        set_failed_audio_dir(dir.clone());

        let payload = vec![1u8, 2, 3, 4];
        let path = persist_failed_recording(&payload).expect("persist should succeed");
        assert!(path.starts_with(&dir));
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("wav"));

        let read_back = read_failed_recording(&path).expect("read back should succeed");
        assert_eq!(read_back, payload);

        delete_failed_recording(&path);
        assert!(!path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn persist_empty_payload_is_rejected() {
        let _guard = DIR_LOCK.lock().unwrap();
        set_failed_audio_dir(temp_test_dir("empty"));
        assert!(persist_failed_recording(&[]).is_none());
    }

    #[test]
    fn cleanup_removes_only_expired_wav_files() {
        let _guard = DIR_LOCK.lock().unwrap();
        let dir = temp_test_dir("cleanup");
        let _ = std::fs::create_dir_all(&dir);
        set_failed_audio_dir(dir.clone());

        let old_path = dir.join("stt-old.wav");
        let fresh_path = dir.join("stt-fresh.wav");
        let keep_path = dir.join("stt-old.txt");
        std::fs::write(&old_path, b"old").unwrap();
        std::fs::write(&fresh_path, b"fresh").unwrap();
        std::fs::write(&keep_path, b"keep").unwrap();

        // Backdate the old file beyond the retention window.
        let old_time = std::time::SystemTime::now()
            - std::time::Duration::from_secs((FAILED_AUDIO_RETENTION_DAYS as u64 + 1) * 24 * 3600);
        let file = std::fs::File::options()
            .write(true)
            .open(&old_path)
            .unwrap();
        file.set_modified(old_time).unwrap();
        drop(file);

        cleanup_failed_recordings();

        assert!(!old_path.exists());
        assert!(fresh_path.exists());
        assert!(keep_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
