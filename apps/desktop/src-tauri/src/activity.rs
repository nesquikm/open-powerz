//! The latency-critical activity held while a meter session is open (FR
//! X2SPDW). The meter keeps only ~60 ms of records at 1000 SPS, so on macOS
//! the worker must not be napped (App Nap) or have its timers coalesced
//! while the window is hidden or covered. Other systems need nothing; the
//! hold count is kept on every OS so the lifetime is testable.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Activities held right now, across the process.
static HELD: AtomicUsize = AtomicUsize::new(0);

/// How many activities are held right now.
pub fn held() -> usize {
    HELD.load(Ordering::SeqCst)
}

/// A held activity; dropping it releases it.
#[derive(Debug)]
pub struct Activity {
    #[cfg(target_os = "macos")]
    token: Option<macos::Token>,
}

/// Hold a latency-critical activity until the returned guard is dropped.
pub fn hold(reason: &str) -> Activity {
    HELD.fetch_add(1, Ordering::SeqCst);
    #[cfg(not(target_os = "macos"))]
    let _ = reason;
    Activity {
        #[cfg(target_os = "macos")]
        token: Some(macos::begin(reason)),
    }
}

impl Drop for Activity {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(token) = self.token.take() {
            macos::end(&token);
        }
        HELD.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use objc2::rc::Retained;
    use objc2::runtime::{NSObjectProtocol, ProtocolObject};
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    pub(super) type Token = Retained<ProtocolObject<dyn NSObjectProtocol>>;

    /// User-initiated (no App Nap, no timer throttling) and latency
    /// critical, while still letting an idle Mac sleep.
    pub(super) fn begin(reason: &str) -> Token {
        let options = NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
            | NSActivityOptions::LatencyCritical;
        NSProcessInfo::processInfo()
            .beginActivityWithOptions_reason(options, &NSString::from_str(reason))
    }

    pub(super) fn end(token: &Token) {
        // SAFETY: `token` is exactly the object `beginActivityWithOptions:reason:`
        // returned, which is the type `endActivity:` expects, and it is ended
        // once (the guard takes it out before ending it).
        unsafe { NSProcessInfo::processInfo().endActivity(token) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_is_counted_until_dropped() {
        let before = held();
        let activity = hold("test");
        assert!(held() > before);
        drop(activity);
    }
}
