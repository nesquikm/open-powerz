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

/// Run the calling thread at the user-interactive quality-of-service class on
/// macOS (AC-SX3EDV.8), so the scheduler keeps the poll loop on time; called
/// first thing on the meter worker's poll thread. A no-op on other systems.
pub fn raise_poll_thread_qos() {
    #[cfg(target_os = "macos")]
    macos::raise_thread_qos();
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

    /// Best effort: a refused request leaves the thread at its default class.
    pub(super) fn raise_thread_qos() {
        // SAFETY: the call only changes the calling thread's own QoS class; it
        // takes plain values and no pointers.
        let _ = unsafe {
            libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
        };
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

    /// AC-SX3EDV.8: the poll thread asks for the user-interactive QoS class on
    /// macOS; the class read back from the thread itself must be that one.
    #[cfg(target_os = "macos")]
    #[test]
    #[allow(unsafe_code)]
    fn the_poll_thread_runs_at_user_interactive_qos_on_macos() {
        let class = std::thread::spawn(|| {
            raise_poll_thread_qos();
            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative: libc::c_int = 0;
            // SAFETY: both out-pointers point at live locals of the right types, and
            // `pthread_self()` is the calling thread, alive for the whole call.
            let rc = unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
            };
            assert_eq!(rc, 0, "pthread_get_qos_class_np failed");
            class as u32
        })
        .join()
        .expect("the probe thread panicked");
        assert_eq!(
            class,
            libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE as u32,
            "the poll thread's QoS class"
        );
    }

    /// AC-SX3EDV.8: other systems keep their scheduling; the call exists and is a no-op.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn raising_the_poll_thread_qos_is_harmless_elsewhere() {
        std::thread::spawn(raise_poll_thread_qos)
            .join()
            .expect("raise_poll_thread_qos panicked off macOS");
    }
}
