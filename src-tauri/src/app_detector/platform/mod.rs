use std::sync::Arc;

use super::types::ContextSignals;
use super::types::TargetAppGuard;
use super::types::{BrowserAccessStatus, BrowserTarget};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) trait ContextSignalSource: Send + Sync + 'static {
    fn collect(&self) -> Option<ContextSignals>;
}

pub(crate) fn default_source() -> Arc<dyn ContextSignalSource> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsContextSource)
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(linux::LinuxContextSource)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        Arc::new(UnsupportedContextSource)
    }
}

pub(crate) fn restore_target_application(target: &TargetAppGuard) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows::restore_target_application(target)
    }
    #[cfg(target_os = "linux")]
    {
        linux::restore_target_application(target)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = target;
        false
    }
}

pub(crate) fn request_browser_access(target: BrowserTarget) -> BrowserAccessStatus {
    // Browser-access detection (reading the frontmost browser URL) was a
    // macOS-only feature; it is not available in this build.
    let _ = target;
    BrowserAccessStatus::NotApplicable
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
struct UnsupportedContextSource;

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
impl ContextSignalSource for UnsupportedContextSource {
    fn collect(&self) -> Option<ContextSignals> {
        None
    }
}
