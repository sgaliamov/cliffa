use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A handle to control the lifecycle of an application, signaling when it should gracefully finish execution.
///
/// The handle starts in a "running" state and can be set to "should finish" by calling `finish()`.
/// Application code should check `should_finish()` or `is_running()` to decide when to complete its work and exit.
#[derive(Clone)]
pub struct AppHandle {
    state: Arc<AtomicBool>,
}

impl AppHandle {
    /// Creates a new handler in the "running" state.
    #[inline]
    pub fn new() -> Self {
        Self {
            state: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Requests that the application gracefully finish execution.
    #[inline]
    pub fn finish(&self) {
        self.state.store(false, Ordering::Release);
    }

    /// Checks if the application should finish execution.
    #[inline(always)]
    pub fn should_finish(&self) -> bool {
        !self.is_running()
    }

    /// Checks if the application should continue running.
    #[inline(always)]
    pub fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire)
    }
}

impl Default for AppHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_handler_is_running() {
        let app = AppHandle::new();
        assert!(app.is_running());
        assert!(!app.should_finish());
    }

    #[test]
    fn cloned_handler_reflects_finish_from_original() {
        let app1 = AppHandle::new();
        assert!(app1.is_running());

        let app2 = app1.clone();
        app1.finish();

        assert!(app1.should_finish());
        assert!(app2.should_finish());
        assert!(!app1.is_running());
        assert!(!app2.is_running());
    }
}
