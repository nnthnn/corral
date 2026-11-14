import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import "./browser.css";

interface BrowserTab {
  id: string;
  url: string;
  title: string;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  generation: number;
}

interface BrowserState {
  session: string;
  tabs: BrowserTab[];
  focusedTabID: string | null;
}

const session = new URLSearchParams(location.search).get("session") ?? "";
const EMPTY: BrowserState = { session, tabs: [], focusedTabID: null };

function sendBounds() {
  const top =
    document.querySelector<HTMLElement>(".browser-bar")?.offsetHeight ?? 44;
  void invoke("browser_ui_bounds", {
    session,
    x: 0,
    y: top,
    width: window.innerWidth,
    height: Math.max(0, window.innerHeight - top),
  }).catch(() => {});
}

function BrowserChrome() {
  const [state, setState] = useState<BrowserState>(EMPTY);
  const [address, setAddress] = useState("");
  const editingRef = useRef(false);
  const barRef = useRef<HTMLDivElement>(null);
  const focused =
    state.tabs.find((tab) => tab.id === state.focusedTabID) ?? null;

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void listen<BrowserState>("browser-state", (event) => {
      if (event.payload.session !== session) return;
      setState(event.payload);
      if (!editingRef.current && event.payload.focusedTabID) {
        const tab = event.payload.tabs.find(
          (candidate) => candidate.id === event.payload.focusedTabID,
        );
        if (tab) setAddress(tab.url);
      }
    }).then((un) => {
      if (disposed) un();
      else unlisten = un;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const barHeight = barRef.current?.offsetHeight ?? 44;
    void invoke("browser_ui_ready", {
      session,
      x: 0,
      y: barHeight,
      width: window.innerWidth,
      height: Math.max(0, window.innerHeight - barHeight),
    }).catch(() => {});
    sendBounds();
    window.addEventListener("resize", sendBounds);
    return () => window.removeEventListener("resize", sendBounds);
  }, []);

  return (
    <div className="browser-shell">
      <div className="browser-bar" ref={barRef}>
        <div className="browser-nav">
          <button
            type="button"
            aria-label="Back"
            disabled={!focused?.canGoBack}
            onClick={() =>
              focused &&
              void invoke("browser_go_back", { session, tabId: focused.id })
            }
          >
            ←
          </button>
          <button
            type="button"
            aria-label="Forward"
            disabled={!focused?.canGoForward}
            onClick={() =>
              focused &&
              void invoke("browser_go_forward", { session, tabId: focused.id })
            }
          >
            →
          </button>
          <button
            type="button"
            aria-label="Reload"
            disabled={!focused}
            onClick={() =>
              focused &&
              void invoke("browser_reload", { session, tabId: focused.id })
            }
          >
            ⟳
          </button>
        </div>
        <div className="browser-tabs">
          {state.tabs.map((tab) => (
            <div
              key={tab.id}
              className={
                tab.id === state.focusedTabID
                  ? "browser-tab browser-tab-active"
                  : "browser-tab"
              }
            >
              <button
                type="button"
                className="browser-tab-title"
                title={tab.url}
                onClick={() =>
                  void invoke("browser_tab_focus", { session, tabId: tab.id })
                }
              >
                {tab.title || tab.url || "New tab"}
              </button>
              <button
                type="button"
                className="browser-tab-close"
                aria-label="Close tab"
                onClick={() =>
                  void invoke("browser_tab_close", { session, tabId: tab.id })
                }
              >
                ×
              </button>
            </div>
          ))}
          <button
            type="button"
            className="browser-new-tab"
            aria-label="New tab"
            onClick={() =>
              void invoke("browser_tab_new", { session, url: null })
            }
          >
            +
          </button>
        </div>
        <input
          className="browser-address"
          value={address}
          spellCheck={false}
          placeholder="http://localhost:3000"
          onFocus={() => {
            editingRef.current = true;
          }}
          onBlur={() => {
            editingRef.current = false;
          }}
          onChange={(event) => setAddress(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            const value = address.trim();
            if (!value) return;
            editingRef.current = false;
            void invoke("browser_navigate", { session, url: value }).catch(
              () => {},
            );
          }}
        />
      </div>
      {state.tabs.length === 0 && (
        <div className="browser-empty">
          <p>No page open</p>
          <p className="browser-empty-hint">
            The agent opens pages here. You can also type an address above.
          </p>
        </div>
      )}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(<BrowserChrome />);
