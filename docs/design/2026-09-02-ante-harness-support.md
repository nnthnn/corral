# Ante Harness Support — Research & Design

> **Status:** pre-implementation research/design doc (not yet a task-by-task
> plan). Captures what Ante is and how it would slot in as a *new `Harness`
> variant* alongside `Claude` / `Opencode` / `Opencode2`. Written 2026-09-02
> for exploration in a fresh worktree.

**TL;DR:** Ante (https://antigma.ai, https://github.com/AntigmaLabs/ante) is
itself a harness — a self-contained terminal coding agent in one ~15MB Rust
binary. corral already has the exact abstraction for this: the `Harness` enum
(`crates/corral-core/src/harness.rs`). Adding Ante is mostly "another arm in
the `match &settings.harness`", like Opencode2 was. The interesting design
fork is whether to embed Ante's TUI in a PTY (the opencode/opencode2 pattern)
or to drive its structured `ante serve` JSONL protocol instead — the latter is
where Ante offers things corral currently has to fake (real turn/tool events,
usage + context-window telemetry, typed permission approval).

---

## 1. What Ante is

- A **terminal coding agent** ("another terminal agent") that works like Claude
  Code or Codex, but as a single Rust binary with **zero runtime deps**, built
  in native Rust. Roughly **~7× less peak memory, ~9× less avg CPU, ~5× less
  disk I/O** than Claude Code on their 20-parallel-tasks benchmark.
- **Modes:**
  | Mode | Command | Purpose |
  |---|---|---|
  | Interactive TUI | `ante` | day-to-day terminal work |
  | Headless | `ante -p "..."` | one-shot tasks, scripts, CI |
  | Server | `ante serve` | editor/integration clients over a **JSONL protocol** (stdio / ws) |
  | Gateway | `ante gateway` | Slack/Discord bot |
- **`--profile <name>`** swaps the *whole* settings file per run — system
  prompt, tool set, skills, memory. "One binary, many agents": a profile
  defines the agent (see `curated/pi.settings.json` for the stripped 4-tool
  extreme).
- **Resume** via `--resume ses_...` (session ids in `ses_...` form, like the
  ones corral stores per worktree).
- **Offline** via `--offline-model <gguf>` (built-in llama.cpp); providers via
  `~/.ante/catalog.json` (17 presets + a config layer for arbitrary
  OpenAI-compatible endpoints). No account required.

## 2. Repo / licensing shape (important)

The open repo is **not** the harness core:

- `crates/protocol-shape` — the **wire protocol** (`Op`/`Evt` types + ids +
  usage). Apache-2.0, semver'd, described as "an external compatibility
  boundary." **This is the thing to build against.**
- `crates/ante-sdk` — Rust SDK for "driving long-lived external agent
  runtimes." Currently only a **`claude` module** (drives Claude Code over its
  stream-json protocol — amusingly, an adapter *for* Claude) plus a
  `stdio` JSONL transport. **No `ante` module yet.**
- `crates/exec`, `crates/llm` — standalone libs.
- `ante-acp` — an **empty crate** (1-byte `lib.rs`). ACP support is a stub.
- `ante-harbor` — their Terminal-Bench eval adapter.

The **core `ante` binary is closed-source during alpha** (shipped prebuilt
under `BINARY-TERMS.md`; open-sourcing progress tracked in issue #21). So the
harness core itself is a black box you drive over the protocol — the protocol
is the contract, and it's explicitly "beta: expect breaking changes."

## 3. How it slots into corral (the "another mode" framing)

Ante is a **new `Harness` variant**, exactly like Opencode/Opencode2. The
touch points, from the existing code:

1. **`crates/corral-core/src/harness.rs`** — add `Harness::Ante`:
   - `Serialize`/`Deserialize` → `"ante"`
   - `display_name()` → `"Ante"` (or `"Ante (beta)"`)
   - `default_executable()` → `"ante"`
   - `install_hint()` → install command + custom-path pointer
   - Note: `Harness::Unknown` **already** forward-compat-guards this — an
     older corral reading a newer `settings.json` with `"ante"` preserves it
     verbatim and refuses to launch with a clear message (`session.rs`
     `Unknown` arm). The enum was built for exactly this.
2. **`crates/corral-core/src/settings.rs`** — add `ante_executable_path:
   String` (mirrors `opencode_executable_path` / `opencode2_executable_path`),
   default `String::new()`, missing-field deserialization default, round-trip
   tests.
3. **`src-tauri/src/commands/session.rs`** — add a `Harness::Ante` arm to the
   `match &settings.harness` (~line 630) that resolves the binary, builds the
   PTY `CommandBuilder`, and wires resume via `StoredSession::harness`
   filtering (per-worktree `--resume` / fresh session id — same shape as the
   Claude arm).
4. **Frontend** — `src/types.ts` (add `"ante"` to the `harness` union + the
   executable-path setting), `src/components/SettingsForm.tsx` (harness
   option + conditional executable field), `src/components/SettingsPanel.test.tsx`
   (visibility tests). `src/store.ts` default stays `"claude"`.

**No new Tauri commands** are required for the PTY-embed path (it reuses
`start_session`), so no `build.rs` / `capabilities/default.json` /
`generate_handler!` changes — unlike the opencode HTTP helpers in
`crates/corral-core/src/opencode.rs`, which were internal to the session arm,
not new commands.

## 4. The design fork

### Path A — PTY-embed the TUI (minimal, matches opencode/opencode2)

`Harness::Ante` spawns `ante` (with `--resume`/session-id handling) in a PTY,
rendered by xterm.js like every other harness. This is a small, well-trodden
diff following the Opencode2 arm.

- ✅ Reuses the entire session/terminal/status pipeline unchanged.
- ✅ Fastest path to a working "Ante mode."
- ❌ Throws away the structured protocol: status stays the PTY-output/hook
  guess, no usage/cost telemetry, permission approval is whatever the TUI
  renders.

### Path B — drive `ante serve` (ambitious, the real payoff)

`Harness::Ante` (or a variant of it) spawns `ante serve` and speaks the
`protocol-shape` `Op`/`Evt` JSONL. A thin Rust client (the `ante-sdk` `stdio`
transport + `protocol-shape` types are both Apache-2.0 and liftable — there's
just no `ante` module wrapping them yet) sends `StartSession`/`UserInput`/
`ApprovalResponse` and consumes the `Evt` stream.

- ✅ Structured events corral currently reconstructs by hand (see §5).
- ✅ **New capabilities corral doesn't have at all:** `UsageUpdate` (usage +
  context-window occupancy), `ContextReport` (per-category token breakdown),
  `SessionEnd.usage`, `TurnEnd { status, steps }`.
- ❌ Bigger lift: client, event→Tauri-event mapping, and a rendering story
  beyond xterm.js (a conversation/tool-timeline view vs. a raw PTY).
- ❌ Risk concentrated on a beta protocol + closed binary.

### Recommendation

**Path A first** (ship a working Ante mode, exactly parallel to Opencode2),
**then spike Path B** in that worktree. The two aren't mutually exclusive: a
first cut embeds the TUI so users get parity, and a `serve`-backed structured
view can come later. Do the spike (§6) *before* writing any `serve` code.

## 5. What `serve` gives that PTY-embedding doesn't

| corral today (all harnesses) | Ante `protocol-shape` `Evt` |
|---|---|
| `hooks.rs` Stop/Notification/UserPromptSubmit → waiting/working guess | `TurnStart` / `TurnPause{Approval}` / `TurnResume` / `TurnEnd{status,steps}` |
| PTY scrollback — no "what did it run" | `ShellOutput { command, stdout, stderr, exit_code }` |
| No cost/usage visibility | `UsageUpdate { usage, context }`, `SessionEnd { usage }`, `ContextReport` |
| `agent_session.rs` worktree → last session id | `ResumeSession { session_id }` (first-class) |
| `onboarding.rs` prompt + `project_settings.rs` provisioning | `SessionRequest { system_prompt, tools, include_tools, exclude_tools, permission_mode, model, provider, effort, cwd, save_session }` |
| Permission handling via each CLI's own TUI | `Op::ApprovalResponse { turn_id, responses: Vec<ToolDecision> }` — typed Accept/Deny/AcceptAlways |

The last row is the notable fit for corral's worktree model: Ante's
`SessionRequest` + `--profile` make **"one worktree = one agent definition"** a
first-class concept (per-worktree system prompt, tool set, effort, provider),
where corral currently assembles it from onboarding text and provisioning
rules.

## 6. Spike (do this first, in the new worktree)

Goal: confirm the `serve` protocol is rich/stable enough to build on before
committing to Path B.

1. `curl -fsSL https://ante.run/install.sh | bash`; `ante --version`.
2. In a scratch worktree, run a one-shot `ante -p "add a function that ..."`
   and note the `--resume`/session-id behavior.
3. Run `ante serve` and capture the JSONL transcript for a small task. Confirm
   the actual sequence and shapes of `StartSession`/`SessionStart` →
   `UserInput` → `TurnStart` → `ToolStart`/`ToolEnd` → `TurnEnd` →
   `UsageUpdate` → `SessionEnd`.
4. Check the transport (stdio vs ws flag) and whether one `serve` process
   keeps a session alive across turns (long-lived runtime) or is per-turn.
5. Check `--profile` + `SessionRequest`-field override behavior (does a
   per-worktree profile cleanly express corral's per-worktree agent identity?).

Open questions to answer during the spike: resume semantics vs. corral's
`StoredSession::harness` model; whether `serve` exposes a terminal/TUI stream
at all (or only structured events); stability of the `Op`/`Evt` types across
the preview releases we'd need to pin.

## 7. Risks / open questions

- **Closed core binary during alpha** (Apache-2.0 covers only SDK/protocol/docs).
  Build against `protocol-shape`, not the binary; pin a release channel.
- **Beta protocol**, fast iteration (`0.preview.71` → `.91`); `protocol-shape`
  is semver'd as the compatibility boundary but expect drift.
- **No `ante` module in `ante-sdk`** — you'd assemble a client from the
  `stdio` transport + `protocol-shape` types (or wait for them to ship one).
- **`ante-acp` is empty** — don't plan around it.
- **Status signal:** Ante has no Claude-Code-style hooks to drive corral's
  waiting/working dot; but `TurnStart`/`TurnPause`/`TurnEnd` are strictly
  richer than the hook guessing — it's a *better* signal, just a different one
  to wire in.

## 8. References

- https://antigma.ai — product site
- https://github.com/AntigmaLabs/ante — repo (README covers modes, providers,
  profiles, repo layout)
- `crates/protocol-shape` — `Op`/`Evt` wire types (the contract)
- `crates/ante-sdk` — `claude` module + `stdio` transport (liftable)
- https://docs.antigma.ai — usage / serve / profile docs
