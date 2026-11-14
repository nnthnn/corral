// Corral's in-page agent for the embedded browser.
//
// Injected by `browser_window.rs` as an initialization script into every frame
// of every page webview. It deliberately exposes only plain data across the
// Tauri boundary (no DOM nodes), and all control comes from Rust -> page. The
// page never gets to call a Tauri command.
(() => {
  if (window.__corral) return;

  const MAX_TEXT = 100_000;
  const MAX_BODY = 20_000;
  const MAX_CONSOLE = 2_000;
  const MAX_NETWORK = 2_000;

  const refs = new Map();
  const consoleEntries = [];
  const requests = [];
  const requestById = new Map();
  const pending = new Map();
  let refCounter = 0;
  let consoleCounter = 0;
  let requestCounter = 0;
  let consoleDropped = 0;
  let networkDropped = 0;
  let dialog = null;

  const now = () => Date.now();
  const absoluteUrl = (value) => {
    try {
      return new URL(String(value), location.href).href;
    } catch {
      return String(value);
    }
  };
  const collapse = (value) => String(value ?? "").replace(/\s+/g, " ").trim();
  const escape = (value) => collapse(value).replace(/"/g, "'");

  function pushCapped(list, item, cap, onDrop) {
    list.push(item);
    while (list.length > cap) {
      list.shift();
      onDrop();
    }
  }

  // ---------------------------------------------------------------- console
  const LEVELS = { debug: 0, info: 1, warning: 2, error: 3 };
  const levelFor = (name) =>
    name === "debug" ? "debug" : name === "warn" || name === "warning" ? "warning" : name === "error" ? "error" : "info";

  function recordConsole(level, args, source) {
    let text;
    try {
      text = args
        .map((arg) => {
          if (typeof arg === "string") return arg;
          if (arg instanceof Error) return `${arg.name}: ${arg.message}`;
          try {
            return JSON.stringify(arg);
          } catch {
            return String(arg);
          }
        })
        .join(" ");
    } catch {
      text = String(args);
    }
    const plain = collapse(text);
    // Tauri injects its plugin IPC initialization into every webview,
    // including these deliberately permission-less page webviews. Its
    // expected ACL denials are not page console output.
    if (plain.includes("not allowed on window") && plain.includes("referenced by: capability")) return;
    pushCapped(
      consoleEntries,
      {
        id: `c${++consoleCounter}`,
        timestampMs: now(),
        level,
        text: plain.slice(0, MAX_TEXT),
        textTruncated: plain.length > MAX_TEXT,
        ...(source ? { source } : {}),
      },
      MAX_CONSOLE,
      () => {
        consoleDropped += 1;
      },
    );
  }

  for (const name of ["debug", "info", "log", "warn", "error"]) {
    const original = console[name]?.bind(console);
    if (!original) continue;
    console[name] = (...args) => {
      recordConsole(levelFor(name), args);
      return original(...args);
    };
  }
  window.addEventListener("error", (event) => {
    recordConsole("error", [event.message], {
      url: event.filename || location.href,
      line: event.lineno || 0,
      column: event.colno || 0,
    });
  });
  window.addEventListener("unhandledrejection", (event) => {
    recordConsole("error", [event.reason]);
  });

  // ---------------------------------------------------------------- network
  function resourceTypeFor(value) {
    const type = String(value || "").toLowerCase();
    if (type.includes("fetch")) return "fetch";
    if (type.includes("xmlhttprequest") || type === "xhr") return "xhr";
    if (type === "script") return "script";
    if (type === "img" || type === "image") return "image";
    if (type === "css" || type === "link") return "stylesheet";
    if (type === "font") return "font";
    if (type === "media" || type === "video" || type === "audio") return "media";
    if (type === "websocket") return "websocket";
    if (type === "eventsource") return "eventsource";
    if (type === "manifest") return "manifest";
    return "other";
  }

  function headersFrom(value) {
    const out = [];
    try {
      if (!value) return out;
      if (typeof value.forEach === "function" && !Array.isArray(value)) {
        value.forEach((headerValue, headerName) => {
          out.push({ name: String(headerName), value: String(headerValue) });
        });
      } else if (Array.isArray(value)) {
        for (const pair of value) {
          if (Array.isArray(pair)) out.push({ name: String(pair[0]), value: String(pair[1]) });
        }
      } else if (typeof value === "object") {
        for (const [name, headerValue] of Object.entries(value)) {
          out.push({ name, value: String(headerValue) });
        }
      }
    } catch {
      // A cross-origin Headers object can throw on iteration; leave it empty.
    }
    return out;
  }

  function bodyState(text, truncated, binary) {
    if (binary) return { state: "unavailable", reason: "binary" };
    if (text === undefined || text === null) return { state: "unavailable", reason: "notCaptured" };
    if (text === "") return { state: "empty" };
    const plain = String(text).slice(0, MAX_BODY);
    return { state: "text", text: plain, truncated };
  }

  function addRequest(record) {
    pushCapped(requests, record, MAX_NETWORK, () => {
      networkDropped += 1;
    });
    requestById.set(record.id, record);
    return record;
  }

  const originalFetch = window.fetch?.bind(window);
  if (originalFetch) {
    window.fetch = async (...args) => {
      const started = now();
      const request = args[0];
      const init = args[1] || {};
      const rawUrl = typeof request === "string" ? request : request?.url || String(request);
      const url = absoluteUrl(rawUrl);
      const method = String(init.method || request?.method || "GET").toUpperCase();
      const record = addRequest({
        id: `n${++requestCounter}`,
        url,
        method,
        resourceType: "fetch",
        timestampMs: started,
        state: "pending",
        requestHeaders: headersFrom(init.headers),
        requestBody: init.body === undefined ? { state: "empty" } : bodyState(String(init.body), false),
        responseHeaders: [],
        responseBody: { state: "notRequested" },
        statusCode: undefined,
      });
      try {
        const response = await originalFetch(...args);
        record.statusCode = response.status;
        record.responseHeaders = headersFrom(response.headers);
        const mime = response.headers.get("content-type") || "";
        const binary = !/^(text\/|application\/(json|javascript|xml|x-www-form-urlencoded))/i.test(mime);
        try {
          const clone = response.clone();
          if (binary) {
            record.responseBody = { state: "unavailable", reason: "binary" };
          } else {
            const text = await clone.text();
            record.responseBody = bodyState(text, text.length > MAX_BODY, false);
          }
        } catch {
          record.responseBody = { state: "unavailable", reason: "notCaptured" };
        }
        record.state = "completed";
        record.durationMs = now() - started;
        return response;
      } catch (error) {
        record.state = "failed";
        record.durationMs = now() - started;
        record.failure = String(error?.message || error).slice(0, 400);
        throw error;
      }
    };
  }

  const XHR = window.XMLHttpRequest;
  if (XHR) {
    const open = XHR.prototype.open;
    const send = XHR.prototype.send;
    const setRequestHeader = XHR.prototype.setRequestHeader;
    XHR.prototype.open = function (method, url, ...rest) {
      this.__corral = {
        id: `n${++requestCounter}`,
        url: absoluteUrl(url),
        method: String(method || "GET").toUpperCase(),
        resourceType: "xhr",
        timestampMs: now(),
        state: "pending",
        requestHeaders: [],
        responseHeaders: [],
        requestBody: { state: "empty" },
        responseBody: { state: "notRequested" },
      };
      return open.call(this, method, url, ...rest);
    };
    XHR.prototype.setRequestHeader = function (name, value) {
      if (this.__corral) this.__corral.requestHeaders.push({ name, value: String(value) });
      return setRequestHeader.call(this, name, value);
    };
    XHR.prototype.send = function (body) {
      const record = this.__corral;
      if (record) {
        if (body !== undefined && body !== null) record.requestBody = bodyState(String(body), false);
        addRequest(record);
        this.addEventListener("loadend", () => {
          record.state = this.status ? "completed" : "failed";
          record.durationMs = now() - record.timestampMs;
          record.statusCode = this.status || undefined;
          record.responseHeaders = [];
          try {
            const raw = this.getAllResponseHeaders() || "";
            for (const line of raw.trim().split(/[\r\n]+/)) {
              if (!line) continue;
              const index = line.indexOf(":");
              if (index < 0) continue;
              record.responseHeaders.push({
                name: line.slice(0, index).trim(),
                value: line.slice(index + 1).trim(),
              });
            }
          } catch {
            // response headers unavailable
          }
          try {
            const responseType = this.responseType;
            if (responseType === "" || responseType === "text" || responseType === "json") {
              const text = this.responseText;
              record.responseBody = bodyState(text, text.length > MAX_BODY, false);
            } else {
              record.responseBody = { state: "unavailable", reason: "binary" };
            }
          } catch {
            record.responseBody = { state: "unavailable", reason: "notCaptured" };
          }
          if (record.state === "failed") record.failure = `HTTP ${this.status || 0}`;
        });
      }
      return send.call(this, body);
    };
  }

  try {
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        if (!entry.name || entry.entryType !== "resource") continue;
        const timestampMs = now() - Math.max(0, (performance.now?.() ?? 0) - entry.startTime);
        // fetch/XHR are already recorded by their hooks; don't list them twice.
        if (
          requests.some(
            (request) =>
              request.url === entry.name &&
              (request.resourceType === "fetch" || request.resourceType === "xhr") &&
              Math.abs(request.timestampMs - timestampMs) < 5000,
          )
        ) {
          continue;
        }
        addRequest({
          id: `n${++requestCounter}`,
          url: entry.name,
          method: "GET",
          resourceType: resourceTypeFor(entry.initiatorType),
          timestampMs,
          state: entry.responseEnd > 0 ? "completed" : "pending",
          durationMs: Math.max(0, entry.duration || 0),
          statusCode: undefined,
          requestHeaders: [],
          responseHeaders: [],
          requestBody: { state: "unavailable", reason: "notCaptured" },
          responseBody: { state: "unavailable", reason: "notCaptured" },
        });
      }
    });
    observer.observe({ type: "resource", buffered: true });
  } catch {
    // PerformanceObserver is best-effort.
  }

  // ----------------------------------------------------------------- dialog
  for (const type of ["alert", "confirm", "prompt"]) {
    const original = window[type]?.bind(window);
    if (!original) continue;
    window[type] = (message = "", defaultValue = "") => {
      dialog = {
        type,
        message: String(message),
        defaultValue: String(defaultValue ?? ""),
      };
      // Native confirm/prompt are synchronous; there is no non-blocking way to
      // wait for the agent, so the page gets the conservative answer while the
      // dialog is recorded for browser.dialog to inspect.
      if (type === "confirm") return false;
      if (type === "prompt") return null;
      return original(message);
    };
  }

  // ------------------------------------------------------------------ refs
  function clearRefs() {
    document.querySelectorAll("[data-corral-ref]").forEach((element) => {
      element.removeAttribute("data-corral-ref");
    });
    refs.clear();
    refCounter = 0;
  }

  function elementFor(ref) {
    const element = refs.get(String(ref || "").replace(/^@/, ""));
    if (!element) throw new Error(`Unknown element ref ${ref}. Take a fresh snapshot.`);
    if (!element.isConnected) throw new Error(`Element ref ${ref} is no longer in the page. Take a fresh snapshot.`);
    return element;
  }

  const SKIP = new Set(["SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT", "META", "LINK", "HEAD", "TITLE", "BASE"]);
  const TEXT_SKIP = new Set([
    "BUTTON",
    "A",
    "SUMMARY",
    "OPTION",
    "LABEL",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
  ]);

  function isVisible(element) {
    if (element.hidden || element.getAttribute("aria-hidden") === "true") return false;
    if (element instanceof HTMLInputElement && element.type === "hidden") return false;
    try {
      const style = getComputedStyle(element);
      return style.display !== "none" && style.visibility !== "hidden" && style.opacity !== "0";
    } catch {
      return true;
    }
  }

  function implicitRole(element) {
    const tag = element.tagName.toLowerCase();
    const type = (element.getAttribute("type") || "text").toLowerCase();
    if (tag === "a" && element.hasAttribute("href")) return "link";
    if (tag === "button" || tag === "summary") return "button";
    if (tag === "input") {
      if (["button", "submit", "reset", "image"].includes(type)) return "button";
      if (type === "checkbox") return "checkbox";
      if (type === "radio") return "radio";
      if (type === "range") return "slider";
      if (type === "file") return "button";
      return "textbox";
    }
    if (tag === "textarea") return "textbox";
    if (tag === "select") return element.multiple ? "listbox" : "combobox";
    if (/^h[1-6]$/.test(tag)) return "heading";
    if (tag === "img") return "img";
    if (tag === "nav") return "navigation";
    if (tag === "main") return "main";
    if (tag === "header") return "banner";
    if (tag === "footer") return "contentinfo";
    if (tag === "aside") return "complementary";
    if (tag === "form") return "form";
    if (tag === "ul" || tag === "ol") return "list";
    if (tag === "li") return "listitem";
    if (tag === "table") return "table";
    if (tag === "tr") return "row";
    if (tag === "td") return "cell";
    if (tag === "th") return "columnheader";
    if (tag === "dialog") return "dialog";
    if (tag === "textarea") return "textbox";
    if (element.isContentEditable) return "textbox";
    return "generic";
  }

  function roleFor(element) {
    const explicit = element.getAttribute("role");
    if (explicit) return explicit.split(/\s+/)[0];
    return implicitRole(element);
  }

  function textOf(element) {
    return collapse(element.innerText ?? element.textContent ?? "");
  }

  function nameFor(element, role) {
    const labelledBy = element.getAttribute("aria-labelledby");
    if (labelledBy) {
      const parts = labelledBy
        .split(/\s+/)
        .map((id) => document.getElementById(id))
        .filter(Boolean)
        .map((target) => textOf(target));
      if (parts.length) return collapse(parts.join(" ")).slice(0, 400);
    }
    const aria = element.getAttribute("aria-label");
    if (aria) return collapse(aria).slice(0, 400);
    if (element instanceof HTMLInputElement && element.labels?.length) {
      const labels = Array.from(element.labels).map((label) => textOf(label)).filter(Boolean);
      if (labels.length) return collapse(labels.join(" ")).slice(0, 400);
    }
    if (element instanceof HTMLImageElement && element.alt) return collapse(element.alt).slice(0, 400);
    if (element instanceof HTMLInputElement && element.type === "button") return collapse(element.value).slice(0, 400);
    const title = element.getAttribute("title");
    if (title) return collapse(title).slice(0, 400);
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
      const placeholder = element.getAttribute("placeholder");
      if (placeholder) return collapse(placeholder).slice(0, 400);
    }
    if (["button", "link", "heading", "option", "tab", "menuitem", "listitem"].includes(role)) {
      return textOf(element).slice(0, 400);
    }
    return "";
  }

  function isActionable(element, role) {
    if (["button", "link", "checkbox", "radio", "textbox", "combobox", "listbox", "slider", "tab", "menuitem", "switch", "option"].includes(role)) return true;
    if (element.hasAttribute("onclick") || element.hasAttribute("tabindex")) return true;
    if (element.isContentEditable) return true;
    return false;
  }

  function stateBits(element, role) {
    const bits = [];
    if (role === "checkbox" || role === "radio" || role === "switch") {
      bits.push(`checked=${element.checked ? "true" : "false"}`);
    }
    if (element.disabled || element.getAttribute("aria-disabled") === "true") bits.push("disabled");
    if (element.getAttribute("aria-expanded")) bits.push(`expanded=${element.getAttribute("aria-expanded")}`);
    if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
      if (element.type !== "password" && element.value) bits.push(`value="${escape(element.value).slice(0, 200)}"`);
      const placeholder = element.getAttribute("placeholder");
      if (placeholder) bits.push(`placeholder="${escape(placeholder).slice(0, 120)}"`);
    }
    if (element instanceof HTMLSelectElement) {
      const selected = Array.from(element.selectedOptions).map((option) => option.value);
      if (selected.length) bits.push(`value="${escape(selected.join(", ")).slice(0, 200)}"`);
    }
    if (element instanceof HTMLAnchorElement && element.getAttribute("href")) {
      bits.push(`href="${escape(element.getAttribute("href")).slice(0, 300)}"`);
    }
    if (role === "heading") {
      const level = Number(element.tagName.slice(1)) || 2;
      bits.push(`level=${level}`);
    }
    return bits.length ? ` ${bits.join(" ")}` : "";
  }

  function boxFor(element) {
    const rect = element.getBoundingClientRect();
    if (!rect.width && !rect.height) return "";
    return ` box=[${Math.round(rect.x + scrollX)},${Math.round(rect.y + scrollY)},${Math.round(rect.width)},${Math.round(rect.height)}]`;
  }

  function snapshot(options = {}) {
    clearRefs();
    const maxDepth = Math.max(1, Math.min(Number(options.depth) || 20, 40));
    const includeBoxes = options.boxes === true;
    const root = options.ref ? elementFor(options.ref) : document.body;
    if (!root) throw new Error("No document body to snapshot.");
    const lines = [];
    let chars = 0;
    let truncated = false;

    function push(line) {
      if (truncated) return;
      chars += line.length + 1;
      if (chars > MAX_TEXT) {
        truncated = true;
        return;
      }
      lines.push(line);
    }

    function walk(node, depth) {
      if (truncated || depth > maxDepth) return;
      if (node.nodeType === Node.TEXT_NODE) {
        const text = collapse(node.nodeValue);
        if (!text || !node.parentElement) return;
        if (TEXT_SKIP.has(node.parentElement.tagName)) return;
        push(`${"  ".repeat(depth)}text "${escape(text).slice(0, 400)}"`);
        return;
      }
      if (node.nodeType !== Node.ELEMENT_NODE) return;
      if (SKIP.has(node.tagName) || !isVisible(node)) return;
      const role = roleFor(node);
      const name = nameFor(node, role);
      const actionable = isActionable(node, role);
      let line = `${"  ".repeat(depth)}- ${role}`;
      if (name) line += ` "${escape(name)}"`;
      line += stateBits(node, role);
      if (actionable) {
        const ref = `e${++refCounter}`;
        node.setAttribute("data-corral-ref", ref);
        refs.set(ref, node);
        line += ` [ref=${ref}]`;
      }
      if (includeBoxes) line += boxFor(node);
      push(line);
      for (const child of node.childNodes) walk(child, depth + 1);
    }

    walk(root, 0);
    return { content: lines.join("\n"), truncated };
  }

  function find(options = {}) {
    const text = String(options.text || "").toLowerCase();
    if (!text) throw new Error("browser.find needs non-empty text.");
    const page = snapshot({ depth: options.depth ?? 20, boxes: options.boxes === true });
    const content = page.content
      .split("\n")
      .filter((line) => line.toLowerCase().includes(text))
      .join("\n");
    return { content, truncated: page.truncated };
  }

  // ----------------------------------------------------------------- input
  function modifierState(modifiers = []) {
    const list = Array.isArray(modifiers) ? modifiers.map((modifier) => String(modifier).toLowerCase()) : [];
    return {
      ctrlKey: list.includes("control"),
      metaKey: list.includes("meta"),
      shiftKey: list.includes("shift"),
      altKey: list.includes("alt"),
    };
  }

  function mouseEvent(element, type, init) {
    return element.dispatchEvent(
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        view: window,
        ...init,
      }),
    );
  }

  function click(options = {}) {
    const element = elementFor(options.ref);
    element.scrollIntoView({ block: "center", inline: "center" });
    const rect = element.getBoundingClientRect();
    const x = rect.x + rect.width / 2;
    const y = rect.y + rect.height / 2;
    const button = options.button === "right" ? 2 : options.button === "middle" ? 1 : 0;
    const count = options.count === 2 ? 2 : 1;
    const mods = modifierState(options.modifiers);
    const base = { clientX: x, clientY: y, screenX: x, screenY: y, button, ...mods };
    try {
      element.focus?.();
    } catch {
      // Some elements are not focusable.
    }
    if (button === 0 && count === 1) {
      element.click();
      return true;
    }
    mouseEvent(element, "mousedown", { ...base, buttons: button === 0 ? 1 : button === 1 ? 4 : 2 });
    mouseEvent(element, "mouseup", { ...base, buttons: 0 });
    if (button === 2) {
      mouseEvent(element, "contextmenu", base);
    } else if (button === 1) {
      mouseEvent(element, "auxclick", base);
    } else {
      element.click();
      if (count === 2) mouseEvent(element, "dblclick", base);
    }
    return true;
  }

  function hover(options = {}) {
    const element = elementFor(options.ref);
    const rect = element.getBoundingClientRect();
    const base = {
      clientX: rect.x + rect.width / 2,
      clientY: rect.y + rect.height / 2,
      bubbles: true,
      cancelable: true,
      view: window,
    };
    element.dispatchEvent(new MouseEvent("mouseover", base));
    element.dispatchEvent(new MouseEvent("mouseenter", base));
    element.dispatchEvent(new MouseEvent("mousemove", base));
    return true;
  }

  function setNativeValue(element, value) {
    const proto =
      element instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : element instanceof HTMLSelectElement
          ? HTMLSelectElement.prototype
          : HTMLInputElement.prototype;
    const descriptor = Object.getOwnPropertyDescriptor(proto, "value");
    if (descriptor?.set) descriptor.set.call(element, value);
    else element.value = value;
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
  }

  function fill(options = {}) {
    const element = elementFor(options.ref);
    element.focus?.();
    if (element.isContentEditable && !(element instanceof HTMLInputElement) && !(element instanceof HTMLTextAreaElement)) {
      element.textContent = String(options.text ?? "");
      element.dispatchEvent(new Event("input", { bubbles: true }));
      element.dispatchEvent(new Event("change", { bubbles: true }));
      return true;
    }
    setNativeValue(element, String(options.text ?? ""));
    return true;
  }

  function fillForm(options = {}) {
    for (const field of options.fields || []) {
      const element = elementFor(field.ref);
      if (field.type === "text") {
        fill({ ref: field.ref, text: field.value });
      } else if (field.type === "select") {
        select({ ref: field.ref, values: field.values || [] });
      } else if (field.type === "check") {
        check({ ref: field.ref, checked: !!field.checked });
      }
    }
    return true;
  }

  function select(options = {}) {
    const element = elementFor(options.ref);
    const values = (options.values || []).map(String);
    if (!(element instanceof HTMLSelectElement)) throw new Error("Ref is not a select element.");
    for (const option of element.options) {
      option.selected = element.multiple ? values.includes(option.value) : option.value === values[0];
    }
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  }

  function check(options = {}) {
    const element = elementFor(options.ref);
    const desired = !!options.checked;
    if (element.checked !== desired) {
      element.click();
    }
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  }

  function press(options = {}) {
    const parts = String(options.key || "").split("+");
    const key = parts.pop() || "";
    const mods = modifierState(parts);
    const target = document.activeElement || document.body;
    const init = { key, code: key.length === 1 ? `Key${key.toUpperCase()}` : key, bubbles: true, cancelable: true, ...mods };
    target.dispatchEvent(new KeyboardEvent("keydown", init));
    target.dispatchEvent(new KeyboardEvent("keypress", init));
    target.dispatchEvent(new KeyboardEvent("keyup", init));
    if (key === "Enter" && target instanceof HTMLInputElement) {
      const form = target.form;
      if (form) form.requestSubmit?.();
    } else if (key === " " && target instanceof HTMLElement) {
      target.click();
    }
    return true;
  }

  function scroll(options = {}) {
    window.scrollBy({ left: Number(options.deltaX) || 0, top: Number(options.deltaY) || 0, behavior: "instant" });
    return true;
  }

  function waitFor(options = {}) {
    const condition = options.condition;
    if (condition === "load") return document.readyState === "complete";
    const text = String(options.text || "").toLowerCase();
    const present = (document.body?.innerText || "").toLowerCase().includes(text);
    return condition === "textGone" ? !present : present;
  }

  function elementBox(options = {}) {
    const element = elementFor(options.ref);
    element.scrollIntoView({ block: "center", inline: "center" });
    const rect = element.getBoundingClientRect();
    return {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
      dpr: window.devicePixelRatio || 1,
    };
  }

  function dialogAction(options = {}) {
    const current = dialog;
    if (options.action === "accept" || options.action === "dismiss") dialog = null;
    return { dialog: current ? { ...current } : null };
  }

  function frames() {
    const out = [{ id: "f0", url: location.href, name: "" }];
    for (const [index, frame] of Array.from(document.querySelectorAll("iframe")).entries()) {
      let url = "";
      try {
        url = frame.contentWindow?.location?.href || frame.src || "";
      } catch {
        url = frame.src || "";
      }
      out.push({ id: `f${index + 1}`, parentID: "f0", url, name: frame.name || "" });
    }
    return { frames: out };
  }

  function consoleRead(options = {}) {
    const minimum = LEVELS[options.level] ?? 0;
    const limit = Math.max(1, Math.min(Number(options.limit) || 100, 500));
    const filtered = consoleEntries.filter((entry) => (LEVELS[entry.level] ?? 0) >= minimum);
    const messages = filtered.slice(-limit);
    return {
      messages,
      truncated: filtered.length > limit,
      dropped: consoleDropped,
    };
  }

  function networkList(options = {}) {
    const needle = options.urlContains == null ? "" : String(options.urlContains);
    const type = options.resourceType;
    const limit = Math.max(1, Math.min(Number(options.limit) || 100, 500));
    const matched = requests.filter((request) => {
      if (needle && !request.url.includes(needle)) return false;
      if (type && request.resourceType !== type) return false;
      return true;
    });
    const list = matched.slice(-limit).map((request) => {
      const base = {
        id: request.id,
        url: request.url,
        method: request.method,
        resourceType: request.resourceType,
        timestampMs: request.timestampMs,
        ...(request.statusCode === undefined ? {} : { statusCode: request.statusCode }),
      };
      if (request.state === "completed") return { ...base, state: "completed", durationMs: request.durationMs || 0 };
      if (request.state === "failed") {
        return { ...base, state: "failed", durationMs: request.durationMs || 0, failure: request.failure || "request failed" };
      }
      return { ...base, state: "pending" };
    });
    return { requests: list, truncated: list.length < matched.length, dropped: networkDropped };
  }

  function networkGet(options = {}) {
    const request = requestById.get(String(options.id || ""));
    if (!request) throw new Error("Unknown request id. Call browser.network.list again.");
    const includeBody = options.includeBody === true;
    const maxChars = Math.max(1, Math.min(Number(options.maxBodyChars) || MAX_BODY, MAX_BODY));
    const body = (value) => {
      if (!includeBody) return { state: "notRequested" };
      if (!value) return { state: "unavailable", reason: "notCaptured" };
      if (value.state === "text") {
        return { ...value, text: String(value.text).slice(0, maxChars), truncated: value.truncated || value.text.length > maxChars };
      }
      return value;
    };
    return {
      request: listRequestForGet(request),
      requestHeaders: request.requestHeaders || [],
      responseHeaders: request.responseHeaders || [],
      headersTruncated: (request.requestHeaders?.length || 0) + (request.responseHeaders?.length || 0) > 100,
      requestBody: body(request.requestBody),
      responseBody: body(request.responseBody),
    };
  }

  function listRequestForGet(request) {
    const base = {
      id: request.id,
      url: request.url,
      method: request.method,
      resourceType: request.resourceType,
      timestampMs: request.timestampMs,
      ...(request.statusCode === undefined ? {} : { statusCode: request.statusCode }),
    };
    if (request.state === "completed") return { ...base, state: "completed", durationMs: request.durationMs || 0 };
    if (request.state === "failed") {
      return { ...base, state: "failed", durationMs: request.durationMs || 0, failure: request.failure || "request failed" };
    }
    return { ...base, state: "pending" };
  }

  function b64Bytes(value) {
    const binary = atob(String(value || ""));
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return bytes;
  }

  function transferFor(files) {
    const transfer = new DataTransfer();
    for (const file of files || []) {
      const bytes = b64Bytes(file.data);
      transfer.items.add(new File([bytes], file.name || "upload.bin", { type: file.mime || "application/octet-stream" }));
    }
    return transfer;
  }

  function upload(options = {}) {
    const element = elementFor(options.ref);
    if (!(element instanceof HTMLInputElement) || element.type !== "file") {
      throw new Error("Ref is not a file input.");
    }
    const transfer = transferFor(options.files);
    element.files = transfer.files;
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));
    return true;
  }

  function drop(options = {}) {
    const element = elementFor(options.ref);
    const transfer = transferFor(options.files);
    for (const type of ["dragenter", "dragover", "drop"]) {
      element.dispatchEvent(
        new DragEvent(type, {
          bubbles: true,
          cancelable: true,
          dataTransfer: transfer,
        }),
      );
    }
    return true;
  }

  // ------------------------------------------------------------- async bridge
  function _start(id, fn) {
    pending.set(id, { done: false });
    Promise.resolve()
      .then(fn)
      .then(
        (value) => {
          let safe;
          try {
            safe = JSON.parse(JSON.stringify(value ?? null));
          } catch {
            safe = null;
          }
          pending.set(id, { done: true, ok: true, value: safe });
        },
        (error) => {
          pending.set(id, { done: true, ok: false, error: String(error?.message || error) });
        },
      );
  }

  function _poll(id) {
    const result = pending.get(id);
    if (!result) return { done: false };
    if (result.done) pending.delete(id);
    return result;
  }

  window.__corral = {
    snapshot,
    find,
    click,
    hover,
    fill,
    elementBox,
    fillForm,
    select,
    check,
    press,
    scroll,
    waitFor,
    dialogAction,
    frames,
    consoleRead,
    networkList,
    networkGet,
    upload,
    drop,
    _element: elementFor,
    _start,
    _poll,
  };
})();
