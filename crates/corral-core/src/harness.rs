use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Harness {
    #[default]
    Claude,
    /// OpenCode v1 (legacy): the pre-2.0 CLI, driven as `opencode serve` plus
    /// `opencode attach <url> --session <id>`. Current releases no longer
    /// ship a v1 `opencode` (it's v2 now), so this only works when the user
    /// points the executable at an actual v1 install.
    Opencode,
    /// OpenCode v2, driven as an embedded TUI pointed at a dedicated
    /// `opencode2 serve` instance (`opencode2 --server <url> --session <id>`).
    /// The V2 CLI ships its own TUI, so corral embeds it rather than rendering
    /// the conversation itself (the earlier chat-in-terminal HTTP relay was
    /// removed because it couldn't match the real TUI's formatting). The
    /// released `@opencode/cli` package ships both the `opencode` and
    /// `opencode2` binaries pointing at the same v2 executable.
    Opencode2,
    /// Pi (https://pi.dev), embedded as its interactive TUI. Pi is a
    /// self-contained CLI like Claude Code: fresh sessions get corral's
    /// onboarding briefing via `--append-system-prompt`, and reopening a
    /// worktree resumes with `--session-id`. Pi has no built-in permission
    /// system (`skip_permissions` is a no-op), and no Claude-style command
    /// hooks — real waiting/working status comes from a small extension
    /// corral writes and loads via `--extension` (see `crate::pi`).
    Pi,
    /// A harness value written to `settings.json` by a *newer* corral build
    /// than the one reading it (e.g. a future third agent). `settings.json`
    /// is a shared file across every corral build on a machine, so an older
    /// build must be able to read a newer build's file — if it couldn't, the
    /// whole settings parse would fail and the file would be reset to
    /// defaults on the next save. Instead the unknown value is preserved
    /// verbatim (the raw string is kept, not just a marker) so re-saving
    /// round-trips it unchanged rather than downgrading it to a known
    /// harness.
    Unknown(String),
}

impl Serialize for Harness {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Harness::Claude => "claude",
            Harness::Opencode => "opencode",
            Harness::Opencode2 => "opencode2",
            Harness::Pi => "pi",
            Harness::Unknown(name) => name,
        })
    }
}

impl<'de> Deserialize<'de> for Harness {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name: String = String::deserialize(deserializer)?;
        Ok(match name.as_str() {
            "claude" => Harness::Claude,
            "opencode" => Harness::Opencode,
            "opencode2" => Harness::Opencode2,
            "pi" => Harness::Pi,
            _ => Harness::Unknown(name),
        })
    }
}

impl Harness {
    pub fn display_name(&self) -> String {
        match self {
            Harness::Claude => "Claude Code".to_string(),
            Harness::Opencode => "OpenCode v1 (legacy)".to_string(),
            Harness::Opencode2 => "OpenCode v2".to_string(),
            Harness::Pi => "Pi".to_string(),
            Harness::Unknown(name) => format!("unknown harness ({name})"),
        }
    }

    /// The executable corral resolves via `PATH` when the user hasn't set a
    /// custom `*_executable_path` override in Settings. `Unknown` has no
    /// known binary — session startup refuses it before this is used.
    pub fn default_executable(&self) -> &'static str {
        match self {
            Harness::Claude => "claude",
            Harness::Opencode => "opencode",
            Harness::Opencode2 => "opencode2",
            Harness::Pi => "pi",
            Harness::Unknown(_) => "",
        }
    }

    /// How to fix a missing binary, for the friendly spawn error
    /// (`bin::missing_executable_error`).
    pub fn install_hint(&self) -> &'static str {
        match self {
            Harness::Claude => {
                "Install Claude Code (https://claude.com/claude-code) or set a \
                 custom path under Settings → Agent executable."
            }
            Harness::Opencode => {
                "OpenCode v1 (legacy) is no longer shipped by the current CLI — \
                 the `opencode` binary is now v2. Install a v1 binary and set \
                 its path under Settings → Agent executable."
            }
            Harness::Opencode2 => {
                "Install OpenCode via pnpm (`pnpm add -g @opencode/cli`, which \
                 ships both the `opencode` and `opencode2` binaries) or set a \
                 custom path under Settings → Agent executable."
            }
            Harness::Pi => {
                "Install Pi with `curl -fsSL https://pi.dev/install.sh | sh` \
                 (or `npm install -g --ignore-scripts \
                 @earendil-works/pi-coding-agent`) or set a custom path under \
                 Settings → Agent executable."
            }
            Harness::Unknown(_) => {
                "Update corral to a build that knows this harness, or change \
                 the harness under Settings."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_lowercase_snake_case() {
        assert_eq!(
            serde_json::to_string(&Harness::Claude).unwrap(),
            "\"claude\""
        );
        assert_eq!(
            serde_json::to_string(&Harness::Opencode).unwrap(),
            "\"opencode\""
        );
        assert_eq!(
            serde_json::to_string(&Harness::Opencode2).unwrap(),
            "\"opencode2\""
        );
        assert_eq!(serde_json::to_string(&Harness::Pi).unwrap(), "\"pi\"");
    }

    #[test]
    fn default_is_claude() {
        assert_eq!(Harness::default(), Harness::Claude);
    }

    #[test]
    fn display_names_mark_v2_as_current_and_v1_as_legacy() {
        assert_eq!(Harness::Claude.display_name(), "Claude Code");
        assert_eq!(Harness::Opencode2.display_name(), "OpenCode v2");
        assert_eq!(Harness::Opencode.display_name(), "OpenCode v1 (legacy)");
        assert_eq!(Harness::Pi.display_name(), "Pi");
    }

    #[test]
    fn known_harnesses_deserialize_from_snake_case_names() {
        assert_eq!(
            serde_json::from_str::<Harness>("\"claude\"").unwrap(),
            Harness::Claude
        );
        assert_eq!(
            serde_json::from_str::<Harness>("\"opencode\"").unwrap(),
            Harness::Opencode
        );
        assert_eq!(
            serde_json::from_str::<Harness>("\"opencode2\"").unwrap(),
            Harness::Opencode2
        );
        assert_eq!(
            serde_json::from_str::<Harness>("\"pi\"").unwrap(),
            Harness::Pi
        );
    }

    #[test]
    fn unknown_harness_round_trips_verbatim() {
        // The core forward-compat guarantee: a harness value written by a
        // newer build must parse into `Unknown` and serialize back to the
        // exact same string — never error (which would reset settings to
        // defaults) and never downgrade to a known harness.
        let h: Harness = serde_json::from_str("\"opencode9\"").unwrap();
        assert_eq!(h, Harness::Unknown("opencode9".to_string()));
        assert_eq!(serde_json::to_string(&h).unwrap(), "\"opencode9\"");
    }
}
