import { X } from "lucide-react";
import { closeWindow } from "../lib/window-chrome";
import { useStore } from "../store";
import { Button } from "./Button";

/**
 * Fallback close-only control for window managers that don't draw their
 * own decorations (known tiling WMs — see `lib.rs`'s `should_decorate()`).
 * No minimize/maximize buttons: tiling WMs have no taskbar to restore a
 * minimized window to, and no meaningful "maximize" when tiling already
 * manages window size. Anchored to the OS window's actual top-right corner
 * rather than any of the three per-pane headers (Sidebar/MainHead/
 * ChangesPane), since which pane's header sits at that corner depends on
 * sidebar/changes-pane collapsed state.
 */
export function WindowControls() {
  const needsCustomChrome = useStore((s) => s.needsCustomChrome);

  if (!needsCustomChrome) return null;

  return (
    <div className="fixed top-0 right-0 z-50 flex items-center">
      <Button
        variant="ghost"
        square
        className="window-close-btn"
        title="Close Corral"
        onClick={() => closeWindow()}
      >
        <X size={14} />
      </Button>
    </div>
  );
}
