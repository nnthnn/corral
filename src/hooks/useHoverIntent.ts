import { useEffect, useRef, useState } from "react";

interface HoverIntentOptions {
  openDelayMs?: number;
  closeDelayMs?: number;
}

interface HoverIntent {
  open: boolean;
  /** Start the open-delay timer (pointer entered the trigger). */
  scheduleOpen: () => void;
  /** Open immediately, no delay — e.g. keyboard focus. */
  openNow: () => void;
  /** Start the close-delay timer (pointer/focus left the trigger or card). */
  scheduleClose: () => void;
  /** Cancel a pending close (pointer re-entered the trigger or card before
   * the close delay elapsed). */
  cancelClose: () => void;
}

/** Delayed open/close intent for a hover card: opening waits
 * `openDelayMs` so a fast mouse pass-over doesn't flash it; closing waits
 * `closeDelayMs` so moving the pointer from the trigger to the card itself
 * doesn't close it in transit. Callers own their own DOM event wiring
 * (see `WorktreeRow`/`WorktreeHoverCard`) — this hook only owns the
 * open/closed boolean and its timers. */
export function useHoverIntent({
  openDelayMs = 400,
  closeDelayMs = 150,
}: HoverIntentOptions = {}): HoverIntent {
  const [open, setOpen] = useState(false);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);

  function clearOpenTimer() {
    if (openTimer.current !== null) {
      window.clearTimeout(openTimer.current);
      openTimer.current = null;
    }
  }
  function clearCloseTimer() {
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  }

  useEffect(() => {
    return () => {
      if (openTimer.current !== null) window.clearTimeout(openTimer.current);
      if (closeTimer.current !== null) window.clearTimeout(closeTimer.current);
    };
  }, []);

  return {
    open,
    scheduleOpen: () => {
      clearCloseTimer();
      clearOpenTimer();
      openTimer.current = window.setTimeout(() => setOpen(true), openDelayMs);
    },
    openNow: () => {
      clearOpenTimer();
      clearCloseTimer();
      setOpen(true);
    },
    scheduleClose: () => {
      clearOpenTimer();
      clearCloseTimer();
      closeTimer.current = window.setTimeout(
        () => setOpen(false),
        closeDelayMs,
      );
    },
    cancelClose: () => {
      clearCloseTimer();
    },
  };
}
