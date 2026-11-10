import clsx from "clsx";
import { useCallback, useEffect, useRef } from "react";
import { saveSettings } from "../lib/settings";
import { useStore } from "../store";
import type { Settings } from "../types";
import { clampWidth } from "../utils";

type WidthKey = "sidebar_width" | "changes_width";

interface ResizeHandleProps {
  settingsKey: WidthKey;
  min: number;
  max: number;
  defaultWidth: number;
  /** Which edge of the pane the handle sits on — determines the sign of
   * the drag delta (a "left" handle's pane grows as the pointer moves
   * left, a "right" handle's pane grows as it moves right). */
  side: "left" | "right";
}

function persist(): void {
  saveSettings().catch((err) =>
    console.error("[corral] settings save failed", err),
  );
}

function setWidth(settingsKey: WidthKey, width: number): void {
  const s = useStore.getState();
  useStore.setState({
    settings: { ...s.settings, [settingsKey]: width } as Settings,
  });
}

export function ResizeHandle({
  settingsKey,
  min,
  max,
  defaultWidth,
  side,
}: ResizeHandleProps) {
  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);

  const handleMove = useCallback(
    (e: PointerEvent) => {
      const drag = dragRef.current;
      if (!drag) return;
      const delta = e.clientX - drag.startX;
      const signedDelta = side === "right" ? delta : -delta;
      setWidth(
        settingsKey,
        clampWidth(drag.startWidth + signedDelta, min, max),
      );
    },
    [settingsKey, min, max, side],
  );

  // Self-referential useCallback: safe because the body only runs when the
  // event fires, by which point `handleUp` (the const itself) is already
  // assigned — same pattern as a named recursive function expression.
  const handleUp = useCallback(() => {
    if (!dragRef.current) return;
    dragRef.current = null;
    document.body.classList.remove("resizing");
    window.removeEventListener("pointermove", handleMove);
    window.removeEventListener("pointerup", handleUp);
    persist();
  }, [handleMove]);

  useEffect(() => {
    return () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
    };
  }, [handleMove, handleUp]);

  function handlePointerDown(e: React.PointerEvent<HTMLDivElement>) {
    dragRef.current = {
      startX: e.clientX,
      startWidth: useStore.getState().settings[settingsKey],
    };
    document.body.classList.add("resizing");
    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
  }

  function handleDoubleClick() {
    setWidth(settingsKey, defaultWidth);
    persist();
  }

  const KEY_STEP = 10;

  function handleKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    let delta: number;
    if (e.key === "ArrowRight") {
      delta = KEY_STEP;
    } else if (e.key === "ArrowLeft") {
      delta = -KEY_STEP;
    } else {
      return;
    }
    e.preventDefault();
    const signedDelta = side === "right" ? delta : -delta;
    const current = useStore.getState().settings[settingsKey];
    setWidth(settingsKey, clampWidth(current + signedDelta, min, max));
    persist();
  }

  const width = useStore((s) => s.settings[settingsKey]);

  return (
    // biome-ignore lint/a11y/useSemanticElements: focusable, keyboard-operable "window splitter" separator (WAI-ARIA) — <hr> can't be focusable or take tabIndex/onKeyDown
    <div
      data-testid={`resize-handle-${settingsKey}`}
      role="separator"
      aria-orientation="vertical"
      aria-valuenow={width}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-label={`Resize ${settingsKey === "sidebar_width" ? "sidebar" : "changes pane"}`}
      tabIndex={0}
      className={clsx(
        "resize-handle absolute inset-y-0 w-1 cursor-col-resize z-10",
        side === "right"
          ? "right-0 translate-x-1/2"
          : "left-0 -translate-x-1/2",
      )}
      onPointerDown={handlePointerDown}
      onDoubleClick={handleDoubleClick}
      onKeyDown={handleKeyDown}
    />
  );
}
