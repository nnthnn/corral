//! Embedded browser backend for OpenCode's `browser.*` tools.
//!
//! One dedicated corral browser window per opencode session. The window's own
//! top-level webview is a small local "chrome" page (tabs + address bar), and
//! each opencode tab is a child webview positioned below that bar. The local
//! chrome page is the only part with Tauri permissions; page webviews are
//! remote and deliberately get no capability at all.
//!
//! The page-side control surface lives in `browser_agent.js`, injected as an
//! initialization script. Rust drives it through `eval_with_callback` (and a
//! small poll bridge for async `evaluate`), so a remote page never calls a
//! Tauri command itself.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, Webview, WebviewBuilder,
    WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    webview::{NewWindowResponse, PageLoadEvent},
};
use url::Url;
use uuid::Uuid;

const AGENT_JS: &str = include_str!("browser_agent.js");
const MIN_WIDTH: f64 = 320.0;
const MIN_HEIGHT: f64 = 200.0;

/// A failed browser operation: `(code, message)`, mapped to the RPC failure
/// shape by the command layer.
pub type BrowserError = (String, String);

pub struct BrowserWindow {
    app: AppHandle,
    session: String,
    chrome_label: String,
    state: Mutex<State>,
    /// Serializes window creation. The state mutex is dropped while Tauri
    /// builds the window (which dispatches to the main thread), so two
    /// concurrent callers could otherwise create it twice.
    creating: Mutex<()>,
}

struct State {
    window: Option<WebviewWindow>,
    tabs: Vec<Tab>,
    focused: Option<String>,
    ui_ready: bool,
    bounds: Bounds,
    generation: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct Tab {
    id: String,
    webview: Webview,
    url: String,
    title: String,
    loading: bool,
    generation: u64,
    /// A best-effort local history. The page can navigate itself (links,
    /// redirects), so this is not authoritative; it is enough for the
    /// canGoBack/canGoForward fields the schema asks for.
    history: Vec<String>,
    history_index: usize,
}

impl BrowserWindow {
    pub fn new(app: AppHandle, session: String) -> Arc<Self> {
        Arc::new(Self {
            app,
            session,
            chrome_label: format!("corral-browser-chrome-{}", Uuid::new_v4()),
            state: Mutex::new(State {
                window: None,
                tabs: Vec::new(),
                focused: None,
                ui_ready: false,
                bounds: Bounds::default(),
                generation: 0,
            }),
            creating: Mutex::new(()),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn close_all(&self) {
        let (window, tabs) = {
            let mut state = self.state();
            let tabs = std::mem::take(&mut state.tabs);
            state.focused = None;
            state.ui_ready = false;
            (state.window.take(), tabs)
        };
        for tab in tabs {
            let _ = tab.webview.close();
        }
        if let Some(window) = window {
            let _ = window.close();
        }
    }

    /// If the user closed the browser window, its child webviews are gone.
    /// Clear the inventory so the next `tabs.list`/command reflects reality.
    fn ensure_current_window(&self) -> bool {
        {
            let state = self.state();
            let Some(window) = &state.window else {
                return false;
            };
            // Do not use the manager lookup here: Tauri registers a freshly
            // created window slightly after `build()` returns, so a new
            // window can transiently be absent from `get_webview_window`.
            // The stored handle itself tells us whether it was destroyed.
            if window.is_visible().is_ok() {
                return true;
            }
        }
        tracing::warn!(
            "browser_window: clearing {} because its window was destroyed",
            self.chrome_label
        );
        let mut state = self.state();
        state.window = None;
        state.tabs.clear();
        state.focused = None;
        state.ui_ready = false;
        false
    }

    pub fn state_json(&self) -> Value {
        self.ensure_current_window();
        let state = self.state();
        json!({
            "tabs": state.tabs.iter().map(tab_json).collect::<Vec<_>>(),
            "focusedTabID": state.focused,
        })
    }

    pub fn tab_json(&self, tab_id: &str) -> Option<Value> {
        self.ensure_current_window();
        self.state()
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(tab_json)
    }

    pub fn focused_tab_json(&self) -> Option<Value> {
        self.ensure_current_window();
        let state = self.state();
        state
            .focused
            .as_deref()
            .and_then(|id| state.tabs.iter().find(|tab| tab.id == id))
            .map(tab_json)
    }

    fn emit_state(&self) {
        let payload = {
            let state = self.state();
            json!({
                "session": self.session,
                "tabs": state.tabs.iter().map(tab_json).collect::<Vec<_>>(),
                "focusedTabID": state.focused,
            })
        };
        let _ = self
            .app
            .emit_to(self.chrome_label.clone(), "browser-state", payload);
    }

    fn ensure_window(&self) -> Result<WebviewWindow, BrowserError> {
        let _creating = self.creating.lock().unwrap_or_else(|e| e.into_inner());
        {
            let state = self.state();
            if let Some(window) = &state.window
                && window.is_visible().is_ok()
            {
                return Ok(window.clone());
            }
        }
        // The window was closed (or never existed): its child webviews are
        // gone too, so drop the stale inventory before recreating it.
        {
            let mut state = self.state();
            state.window = None;
            state.tabs.clear();
            state.focused = None;
            state.ui_ready = false;
        }
        let url = self.chrome_url()?;
        let window =
            WebviewWindowBuilder::new(&self.app, &self.chrome_label, WebviewUrl::External(url))
                .title(format!("Corral Browser — {}", self.session))
                .inner_size(1200.0, 800.0)
                .min_inner_size(520.0, 360.0)
                .build()
                .map_err(|e| {
                    (
                        "unsupported".to_string(),
                        format!("corral could not create its browser window: {e}"),
                    )
                })?;
        tracing::info!(
            "browser_window: opened {} for {}",
            self.chrome_label,
            self.session
        );
        self.state().window = Some(window.clone());
        Ok(window)
    }

    fn chrome_url(&self) -> Result<Url, BrowserError> {
        let main = self.app.get_webview_window("main").ok_or_else(|| {
            (
                "unsupported".to_string(),
                "corral's main window is unavailable".to_string(),
            )
        })?;
        let base = main.url().map_err(|e| {
            (
                "unsupported".to_string(),
                format!("corral could not resolve its own URL: {e}"),
            )
        })?;
        let mut url = base.join("browser.html").map_err(|e| {
            (
                "unsupported".to_string(),
                format!("corral could not build the browser URL: {e}"),
            )
        })?;
        url.query_pairs_mut()
            .append_pair("session", &self.session)
            .append_pair("label", &self.chrome_label);
        Ok(url)
    }

    pub fn ui_ready(&self, bounds: Bounds) {
        {
            let mut state = self.state();
            state.ui_ready = true;
            state.bounds = clamp_bounds(bounds);
            if state.focused.is_none() {
                state.focused = state.tabs.first().map(|tab| tab.id.clone());
            }
        }
        self.apply_active();
        self.emit_state();
    }

    pub fn set_bounds(&self, bounds: Bounds) {
        {
            let mut state = self.state();
            state.bounds = clamp_bounds(bounds);
        }
        self.apply_active();
    }

    fn apply_active(&self) {
        let state = self.state();
        if !state.ui_ready || state.tabs.is_empty() {
            return;
        }
        let bounds = state.bounds;
        for tab in &state.tabs {
            if state.focused.as_deref() == Some(tab.id.as_str()) {
                let _ = tab.webview.set_bounds(rect(bounds));
                let _ = tab.webview.show();
                let _ = tab.webview.set_focus();
            } else {
                let _ = tab.webview.hide();
            }
        }
    }

    pub fn open_tab(self: &Arc<Self>, raw_url: &str, focus: bool) -> Result<Value, BrowserError> {
        self.ensure_window()?;
        let url = page_url(raw_url)?;
        let bounds = self.state().bounds;
        let tab_id = format!("tab_{}", Uuid::new_v4());
        let page_label = format!("corral-browser-page-{}", Uuid::new_v4());
        let weak = Arc::downgrade(self);
        let page_tab_id = tab_id.clone();
        let builder = WebviewBuilder::new(page_label, WebviewUrl::External(url.clone()))
            .initialization_script_for_all_frames(AGENT_JS)
            .focused(false)
            .on_navigation(navigation_allowed)
            // Page popups and downloads are not managed by this v1 backend;
            // don't let a remote page create unmanaged windows or files.
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, _| false)
            .on_page_load(move |webview, payload| {
                if let Some(window) = weak.upgrade() {
                    window.handle_page_load(&page_tab_id, &webview, &payload);
                }
            });
        let window = self.app.get_window(&self.chrome_label).ok_or_else(|| {
            (
                "unsupported".to_string(),
                "corral's browser window is unavailable".to_string(),
            )
        })?;
        let webview = window
            .add_child(
                builder,
                LogicalPosition::new(bounds.x, bounds.y),
                LogicalSize::new(bounds.width.max(MIN_WIDTH), bounds.height.max(MIN_HEIGHT)),
            )
            .map_err(|e| {
                (
                    "unsupported".to_string(),
                    format!("corral could not create a browser tab: {e}"),
                )
            })?;
        // Stay hidden until the chrome page reports its content bounds.
        let _ = webview.hide();
        let tab = Tab {
            id: tab_id.clone(),
            webview,
            url: url.to_string(),
            title: String::new(),
            loading: true,
            generation: 0,
            history: vec![url.to_string()],
            history_index: 0,
        };
        let mut state = self.state();
        state.generation += 1;
        state.tabs.push(tab);
        if focus || state.focused.is_none() {
            state.focused = Some(tab_id.clone());
        }
        drop(state);
        self.apply_active();
        if focus && let Some(window) = self.app.get_webview_window(&self.chrome_label) {
            let _ = window.set_focus();
        }
        self.emit_state();
        // The chrome page reports its content bounds from its first React
        // effect. Waiting briefly here means an immediate follow-up command
        // (wait/snapshot) sees a positioned, visible page webview.
        if !self.wait_for_ui(Duration::from_secs(5)) {
            tracing::warn!("browser_window: chrome page did not report bounds in time");
        }
        self.apply_active();
        self.tab_json(&tab_id).ok_or_else(|| {
            (
                "unsupported".to_string(),
                "browser tab vanished immediately after opening".to_string(),
            )
        })
    }

    pub fn focus_tab(&self, tab_id: &str) -> Result<Value, BrowserError> {
        {
            let mut state = self.state();
            if !state.tabs.iter().any(|tab| tab.id == tab_id) {
                return Err(tab_unavailable());
            }
            state.focused = Some(tab_id.to_string());
        }
        self.apply_active();
        if let Some(window) = self.app.get_webview_window(&self.chrome_label) {
            let _ = window.set_focus();
        }
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    pub fn close_tab(&self, tab_id: &str) -> Result<Value, BrowserError> {
        let webview = {
            let mut state = self.state();
            let index = state
                .tabs
                .iter()
                .position(|tab| tab.id == tab_id)
                .ok_or_else(tab_unavailable)?;
            let tab = state.tabs.remove(index);
            if state.focused.as_deref() == Some(tab_id) {
                state.focused = state.tabs.first().map(|tab| tab.id.clone());
            }
            tab.webview
        };
        let _ = webview.close();
        self.apply_active();
        self.emit_state();
        Ok(self.state_json())
    }

    pub fn navigate(&self, tab_id: &str, raw_url: &str) -> Result<Value, BrowserError> {
        let url = page_url(raw_url)?;
        let webview = self.webview(tab_id)?;
        webview.navigate(url.clone()).map_err(|e| {
            (
                "unsupported".to_string(),
                format!("corral could not navigate the tab: {e}"),
            )
        })?;
        {
            let mut state = self.state();
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.url = url.to_string();
                tab.title = String::new();
                tab.loading = true;
                tab.generation += 1;
                tab.history.truncate(tab.history_index + 1);
                tab.history.push(url.to_string());
                tab.history_index = tab.history.len() - 1;
            }
            state.generation += 1;
        }
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    pub fn go_back(&self, tab_id: &str) -> Result<Value, BrowserError> {
        self.webview(tab_id)?
            .eval("history.back()")
            .map_err(js_error)?;
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    pub fn go_forward(&self, tab_id: &str) -> Result<Value, BrowserError> {
        self.webview(tab_id)?
            .eval("history.forward()")
            .map_err(js_error)?;
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    pub fn reload(&self, tab_id: &str) -> Result<Value, BrowserError> {
        self.webview(tab_id)?.reload().map_err(js_error)?;
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    pub fn stop(&self, tab_id: &str) -> Result<Value, BrowserError> {
        self.webview(tab_id)?
            .eval("window.stop()")
            .map_err(js_error)?;
        self.emit_state();
        self.tab_json(tab_id).ok_or_else(tab_unavailable)
    }

    fn ensure_ui(&self) -> Result<(), BrowserError> {
        if self.state().ui_ready {
            Ok(())
        } else {
            Err(js_error_str(
                "corral's browser window is still starting; retry in a moment",
            ))
        }
    }

    fn wait_for_ui(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.state().ui_ready {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn webview(&self, tab_id: &str) -> Result<Webview, BrowserError> {
        self.ensure_current_window();
        self.state()
            .tabs
            .iter()
            .find(|tab| tab.id == tab_id)
            .map(|tab| tab.webview.clone())
            .ok_or_else(tab_unavailable)
    }

    fn handle_page_load(
        self: &Arc<Self>,
        tab_id: &str,
        webview: &Webview,
        payload: &tauri::webview::PageLoadPayload<'_>,
    ) {
        let finished = payload.event() == PageLoadEvent::Finished;
        let url = payload.url().to_string();
        {
            let mut state = self.state();
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.loading = !finished;
                if finished && tab.url != url {
                    tab.url = url.clone();
                    if tab.history.get(tab.history_index + 1) == Some(&url) {
                        tab.history_index += 1;
                    } else if tab.history_index > 0
                        && tab.history.get(tab.history_index - 1) == Some(&url)
                    {
                        tab.history_index -= 1;
                    } else {
                        tab.history.truncate(tab.history_index + 1);
                        tab.history.push(url);
                        tab.history_index = tab.history.len() - 1;
                    }
                }
            }
        }
        self.emit_state();
        if finished {
            let weak = Arc::downgrade(self);
            let page_tab_id = tab_id.to_string();
            let _ = webview.eval_with_callback("document.title", move |raw| {
                if let Some(window) = weak.upgrade() {
                    let title = serde_json::from_str::<String>(&raw).unwrap_or(raw);
                    window.set_title(&page_tab_id, &title);
                }
            });
        }
    }

    fn set_title(&self, tab_id: &str, title: &str) {
        let changed = {
            let mut state = self.state();
            if let Some(tab) = state.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                let title = title.chars().take(2048).collect::<String>();
                if tab.title != title {
                    tab.title = title;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if changed {
            self.emit_state();
        }
    }

    /// Synchronous in-page call into `window.__corral.<method>(args)`.
    pub fn agent_call(
        &self,
        tab_id: &str,
        method: &str,
        args: Value,
    ) -> Result<Value, BrowserError> {
        let webview = self.webview(tab_id)?;
        let args = serde_json::to_string(&args).map_err(|e| js_error_str(e.to_string()))?;
        let script = format!(
            "JSON.stringify((() => {{ try {{ const api = window.__corral; \
             if (!api) throw new Error('corral browser agent is not ready'); \
             return {{ ok: true, value: api.{method}({args}) }}; }} \
             catch (e) {{ return {{ ok: false, error: String((e && e.message) || e) }}; }} }})())"
        );
        let parsed = eval_json(&webview, script, Duration::from_secs(15))?;
        if parsed.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(parsed.get("value").cloned().unwrap_or(Value::Null))
        } else {
            Err(js_error_str(
                parsed
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("browser operation failed")
                    .to_string(),
            ))
        }
    }

    /// Async `evaluate`, using the page's poll bridge so `awaitPromise` works
    /// without blocking the webview's main thread.
    pub fn evaluate(
        &self,
        tab_id: &str,
        script: &str,
        element_ref: Option<&str>,
    ) -> Result<Value, BrowserError> {
        let webview = self.webview(tab_id)?;
        let id = Uuid::new_v4().to_string();
        let id_literal = serde_json::to_string(&id).map_err(|e| js_error_str(e.to_string()))?;
        let body = match element_ref {
            Some(reference) => format!(
                "const element = window.__corral._element({}); return ({})(element);",
                serde_json::to_string(reference).map_err(|e| js_error_str(e.to_string()))?,
                script
            ),
            None => format!("return ({script});"),
        };
        let start = format!(
            "(() => {{ const api = window.__corral; if (!api) throw new Error('corral browser agent is not ready'); \
             api._start({id_literal}, async () => {{ {body} }}); }})()"
        );
        webview.eval(start).map_err(js_error)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let poll = format!("JSON.stringify(window.__corral._poll({id_literal}))");
            let polled = eval_json(&webview, poll, Duration::from_secs(5))?;
            if polled.get("done").and_then(Value::as_bool) == Some(true) {
                if polled.get("ok").and_then(Value::as_bool) == Some(true) {
                    return Ok(polled.get("value").cloned().unwrap_or(Value::Null));
                }
                return Err(js_error_str(
                    polled
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("browser evaluate failed")
                        .to_string(),
                ));
            }
            if Instant::now() > deadline {
                return Err(js_error_str("browser evaluate timed out"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn wait(&self, tab_id: &str, action: &Value) -> Result<Value, BrowserError> {
        let condition = action
            .get("condition")
            .and_then(Value::as_str)
            .unwrap_or("load");
        let text = action.get("text").and_then(Value::as_str).unwrap_or("");
        let timeout = action
            .get("timeoutMs")
            .and_then(Value::as_u64)
            .unwrap_or(10_000)
            .clamp(1, 30_000);
        let deadline = Instant::now() + Duration::from_millis(timeout);
        loop {
            // During a navigation the new document may not have run the
            // agent script yet; treat that as "not ready" and keep polling.
            let ready = self
                .agent_call(
                    tab_id,
                    "waitFor",
                    json!({ "condition": condition, "text": text }),
                )
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if ready {
                return self.tab_json(tab_id).ok_or_else(tab_unavailable);
            }
            if Instant::now() > deadline {
                return Err((
                    "unsupported".to_string(),
                    format!("browser.wait timed out after {timeout} ms"),
                ));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    pub fn screenshot(
        &self,
        tab_id: &str,
        action: &Value,
    ) -> Result<(Value, Vec<Value>), BrowserError> {
        self.ensure_ui()?;
        // The skill asks the caller to focus the tab first; focusing here as
        // well keeps a visible view to snapshot even if a model forgets.
        let _ = self.focus_tab(tab_id);
        let webview = self.webview(tab_id)?;
        let element_ref = action.get("ref").and_then(Value::as_str);
        let full_page = action
            .get("fullPage")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && element_ref.is_none();
        let max_width = action
            .get("maxWidth")
            .and_then(Value::as_u64)
            .map(|value| value.clamp(100, 4000) as u32);
        let mut png = snapshot_png(&webview, full_page)?;
        if let Some(reference) = element_ref {
            let (x, y, width, height, dpr) = self.element_box(tab_id, reference)?;
            png = crop_png(&png, x, y, width, height, dpr)?;
        }
        if let Some(max_width) = max_width {
            png = limit_width_png(&png, max_width)?;
        }
        let tab = self.tab_json(tab_id).ok_or_else(tab_unavailable)?;
        let file = json!({
            "id": format!("file_{}", Uuid::new_v4()),
            "name": "screenshot.png",
            "mime": "image/png",
            "data": base64::engine::general_purpose::STANDARD.encode(&png),
        });
        Ok((json!({ "tab": tab }), vec![file]))
    }

    fn element_box(
        &self,
        tab_id: &str,
        reference: &str,
    ) -> Result<(f64, f64, f64, f64, f64), BrowserError> {
        let value = self.agent_call(tab_id, "elementBox", json!({ "ref": reference }))?;
        let number = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_f64)
                .ok_or_else(|| js_error_str(format!("browser screenshot: missing {key}")))
        };
        Ok((
            number("x")?,
            number("y")?,
            number("width")?,
            number("height")?,
            value
                .get("dpr")
                .and_then(Value::as_f64)
                .unwrap_or(1.0)
                .max(1.0),
        ))
    }
}

pub fn rect(bounds: Bounds) -> Rect {
    Rect {
        position: LogicalPosition::new(bounds.x, bounds.y).into(),
        size: LogicalSize::new(bounds.width.max(MIN_WIDTH), bounds.height.max(MIN_HEIGHT)).into(),
    }
}

pub fn clamp_bounds(bounds: Bounds) -> Bounds {
    Bounds {
        x: bounds.x.max(0.0),
        y: bounds.y.max(0.0),
        width: bounds.width.max(1.0),
        height: bounds.height.max(1.0),
    }
}

fn tab_json(tab: &Tab) -> Value {
    json!({
        "id": tab.id,
        "url": tab.url,
        "title": tab.title.chars().take(2048).collect::<String>(),
        "loading": tab.loading,
        "canGoBack": tab.history_index > 0,
        "canGoForward": tab.history_index + 1 < tab.history.len(),
        "generation": tab.generation,
    })
}

fn tab_unavailable() -> BrowserError {
    (
        "unsupported".to_string(),
        "corral does not have that browser tab. Call browser.tabs.list({}) and use an exact \
         returned tabID; tabs are closed when the user closes corral's browser window."
            .to_string(),
    )
}

fn js_error(error: tauri::Error) -> BrowserError {
    js_error_str(error.to_string())
}

fn js_error_str(message: impl Into<String>) -> BrowserError {
    ("unsupported".to_string(), message.into())
}

/// Only web pages (and about:blank) are allowed in the embedded browser. A
/// page can navigate itself, so this is enforced on every navigation, not only
/// on agent-supplied URLs.
fn navigation_allowed(url: &Url) -> bool {
    matches!(url.scheme(), "http" | "https" | "about" | "data" | "blob")
}

/// Normalize an agent- or UI-supplied URL. Mirrors the server plugin's own
/// normalization: bare hosts get https (http for localhost), and only web
/// URLs plus about:blank are accepted.
pub fn page_url(raw: &str) -> Result<Url, BrowserError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("about:blank") {
        return Url::parse("about:blank").map_err(|e| js_error_str(e.to_string()));
    }
    // A filesystem path would otherwise gain a scheme and parse as a hostname.
    if trimmed.starts_with('/') || trimmed.starts_with('\\') || trimmed.starts_with('.') {
        return Err((
            "unsupported".to_string(),
            format!("Not a browser URL: {raw:?}. Use http:// or https://."),
        ));
    }
    let local = trimmed.starts_with("localhost")
        || trimmed.starts_with("127.")
        || trimmed.starts_with("[::1]");
    let has_scheme = trimmed
        .find("://")
        .map(|index| {
            trimmed[..index]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
        })
        .unwrap_or(false);
    let with_scheme = if has_scheme {
        trimmed.to_string()
    } else {
        format!("{}{}", if local { "http://" } else { "https://" }, trimmed)
    };
    let url = Url::parse(&with_scheme).map_err(|_| {
        (
            "unsupported".to_string(),
            format!("Not a browser URL: {raw:?}. Use http:// or https://."),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err((
            "unsupported".to_string(),
            format!("Not a browser URL: {raw:?}. Use http:// or https://."),
        ));
    }
    Ok(url)
}

fn eval_json(webview: &Webview, script: String, timeout: Duration) -> Result<Value, BrowserError> {
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(script, move |result| {
            let _ = tx.send(result);
        })
        .map_err(js_error)?;
    let raw = rx.recv_timeout(timeout).map_err(|_| {
        (
            "unsupported".to_string(),
            "browser evaluation timed out".to_string(),
        )
    })?;
    // The injected script returns `JSON.stringify(...)`. `eval_with_callback`
    // then JSON-serializes that returned string once more, so the callback
    // receives a quoted JSON string that must be decoded before parsing the
    // actual payload.
    let inner: String = serde_json::from_str(&raw)
        .map_err(|e| js_error_str(format!("browser agent returned invalid JSON: {e}")))?;
    serde_json::from_str(&inner)
        .map_err(|e| js_error_str(format!("browser agent returned invalid JSON: {e}")))
}

fn snapshot_png(webview: &Webview, full_page: bool) -> Result<Vec<u8>, BrowserError> {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
    let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
    webview
        .with_webview(move |platform| {
            let view = platform.inner();
            let region = if full_page {
                SnapshotRegion::FullDocument
            } else {
                SnapshotRegion::Visible
            };
            view.snapshot(
                region,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |result| {
                    // A cairo Surface is main-thread-only, so encode it here
                    // and send plain bytes across the channel.
                    let bytes = match result {
                        Ok(surface) => {
                            let mut png = Vec::new();
                            surface.write_to_png(&mut png).ok().map(|_| png)
                        }
                        Err(_) => None,
                    };
                    let _ = tx.send(bytes);
                },
            );
        })
        .map_err(js_error)?;
    rx.recv_timeout(Duration::from_secs(15))
        .map_err(|_| {
            (
                "unsupported".to_string(),
                "screenshot timed out".to_string(),
            )
        })?
        .ok_or_else(|| js_error_str("screenshot failed"))
}

fn crop_png(
    png: &[u8],
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    dpr: f64,
) -> Result<Vec<u8>, BrowserError> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| js_error_str(format!("screenshot decode failed: {e}")))?;
    let max_x = image.width();
    let max_y = image.height();
    let left = (x * dpr).round().max(0.0) as u32;
    let top = (y * dpr).round().max(0.0) as u32;
    let right = ((x + width) * dpr).round().max(0.0) as u32;
    let bottom = ((y + height) * dpr).round().max(0.0) as u32;
    let crop_w = right.saturating_sub(left).min(max_x.saturating_sub(left));
    let crop_h = bottom.saturating_sub(top).min(max_y.saturating_sub(top));
    if crop_w == 0 || crop_h == 0 {
        return Err(js_error_str("screenshot element has no visible area"));
    }
    let cropped = image.crop_imm(left, top, crop_w, crop_h);
    let mut out = Vec::new();
    cropped
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| js_error_str(format!("screenshot encode failed: {e}")))?;
    Ok(out)
}

fn limit_width_png(png: &[u8], max_width: u32) -> Result<Vec<u8>, BrowserError> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .map_err(|e| js_error_str(format!("screenshot decode failed: {e}")))?;
    if image.width() <= max_width {
        return Ok(png.to_vec());
    }
    let height = ((image.height() as u64 * max_width as u64) / image.width() as u64).max(1) as u32;
    let resized = image.resize(max_width, height, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::new();
    resized
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| js_error_str(format!("screenshot encode failed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_url_normalizes_bare_hosts() {
        assert_eq!(
            page_url("example.com").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            page_url("localhost:3000").unwrap().as_str(),
            "http://localhost:3000/"
        );
        assert_eq!(
            page_url("127.0.0.1:5173").unwrap().as_str(),
            "http://127.0.0.1:5173/"
        );
        assert_eq!(page_url("about:blank").unwrap().as_str(), "about:blank");
    }

    #[test]
    fn page_url_rejects_non_web_urls() {
        assert!(page_url("file:///etc/passwd").is_err());
        assert!(page_url("javascript:alert(1)").is_err());
        assert!(page_url("/tmp/page.html").is_err());
    }

    #[test]
    fn navigation_allowed_covers_web_pages_only() {
        assert!(navigation_allowed(
            &Url::parse("https://example.com").unwrap()
        ));
        assert!(navigation_allowed(
            &Url::parse("http://localhost:3000").unwrap()
        ));
        assert!(navigation_allowed(&Url::parse("about:blank").unwrap()));
        assert!(!navigation_allowed(
            &Url::parse("file:///etc/passwd").unwrap()
        ));
        assert!(!navigation_allowed(
            &Url::parse("javascript:alert(1)").unwrap()
        ));
    }

    #[test]
    fn clamp_bounds_keeps_positive_dimensions() {
        let bounds = clamp_bounds(Bounds {
            x: -5.0,
            y: -1.0,
            width: 0.0,
            height: 0.0,
        });
        assert_eq!(bounds.x, 0.0);
        assert_eq!(bounds.y, 0.0);
        assert_eq!(bounds.width, 1.0);
        assert_eq!(bounds.height, 1.0);
    }
}
