//! A tiny extension trait for the "log a failure at `warn` level and
//! continue" idiom repeated across best-effort cleanup code
//! (`worktree.rs`'s file-copy helpers, `system.rs`'s temp-file sweep). Not
//! applied everywhere that idiom appears — call sites with meaningfully
//! different, already-descriptive context text stay inline.

/// Turns a `Result<T, E>` into `Option<T>`, logging `Err` at `warn` level as
/// `"{context}: {e}"` before discarding it.
pub trait LogErr<T> {
    fn log_warn(self, context: &str) -> Option<T>;
}

impl<T, E: std::fmt::Display> LogErr<T> for Result<T, E> {
    fn log_warn(self, context: &str) -> Option<T> {
        self.inspect_err(|e| tracing::warn!("{context}: {e}")).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_result_passes_through_the_value() {
        let r: Result<i32, String> = Ok(42);
        assert_eq!(r.log_warn("test context"), Some(42));
    }

    #[test]
    fn err_result_becomes_none() {
        let r: Result<i32, String> = Err("boom".to_string());
        assert_eq!(r.log_warn("test context"), None);
    }
}
