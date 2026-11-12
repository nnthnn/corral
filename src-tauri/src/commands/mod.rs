pub mod project_settings;
pub mod projects;
pub mod provision;
pub mod pty_process;
pub mod run;
pub mod run_detect;
pub mod run_targets;
pub mod session;
pub mod settings;
pub mod system;
pub mod worktrees;

/// Every Tauri command surfaces errors as `String` (the IPC boundary has no
/// use for `corral_core::Error`'s typed variants), so this replaces the
/// `.map_err(|e| e.to_string())` repeated at nearly every call site.
pub(crate) trait StrErr<T> {
    fn str_err(self) -> Result<T, String>;
}

impl<T, E: std::fmt::Display> StrErr<T> for Result<T, E> {
    fn str_err(self) -> Result<T, String> {
        self.map_err(|e| e.to_string())
    }
}

/// Cap for PTY output retained in memory, shared by both Setup (provisioning)
/// and Run processes. These are one-shot/long-lived commands, so this is
/// generous, but it still bounds growth the way `session::Session`'s PTY
/// scrollback already does for `write_session` — before this existed, a long
/// or noisy command's buffer grew without limit. Raised well past the old
/// 2MB cap, which could silently drop the head of a verbose build's log on
/// reattach.
pub(crate) const MAX_PTY_OUTPUT_BUFFER: usize = 32 * 1024 * 1024;

/// Append `chunk` to `buffer`, trimming from the front so it never exceeds
/// `max_len` bytes. Shared by `session::Session`'s PTY scrollback and
/// `provision::Run`'s output buffer — both retain "recent output" in memory
/// so a UI panel can reattach after navigating away and back.
pub(crate) fn append_capped(buffer: &mut Vec<u8>, chunk: &[u8], max_len: usize) {
    buffer.extend_from_slice(chunk);
    if buffer.len() > max_len {
        let excess = buffer.len() - max_len;
        buffer.drain(0..excess);
    }
}

/// Serde adapter that serializes `Vec<u8>` as a base64 string instead of
/// serde_json's default array-of-numbers. The default inflates a 4KB PTY
/// chunk into `104, 105, ...` (up to ~4x the bytes, and thousands of JSON
/// number tokens to parse per event); base64 is ~1.33x and the frontend
/// decodes it in a single `atob` pass. Used by the `-output` event payloads
/// (the high-frequency streaming path) — the one-shot re-attach buffers
/// (`Attach.buffer`, `PtyProcessState.output`) intentionally stay arrays,
/// since they're fetched once, not per chunk.
pub(crate) mod base64_bytes {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_capped_keeps_buffer_under_growing_chunks() {
        let mut buffer = Vec::new();
        append_capped(&mut buffer, b"hello ", 8);
        append_capped(&mut buffer, b"world", 8);
        assert_eq!(buffer.len(), 8);
        assert_eq!(buffer, b"lo world");
    }

    #[test]
    fn append_capped_is_a_noop_below_the_cap() {
        let mut buffer = Vec::new();
        append_capped(&mut buffer, b"hi", 100);
        assert_eq!(buffer, b"hi");
    }

    #[test]
    fn base64_bytes_serializes_as_a_base64_string() {
        #[derive(serde::Serialize)]
        struct Wrapper {
            #[serde(with = "super::base64_bytes")]
            data: Vec<u8>,
        }

        let w = Wrapper {
            data: b"hi".to_vec(),
        };
        let json = serde_json::to_string(&w).unwrap();
        assert_eq!(json, r#"{"data":"aGk="}"#);
    }
}
