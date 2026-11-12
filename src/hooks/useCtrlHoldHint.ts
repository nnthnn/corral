import { useEffect, useState } from "react";

/** How long a modifier must be held before the sidebar reveals its
 * `Ctrl+1–9` shortcut numbers. */
export const CTRL_HOLD_HINT_DELAY_MS = 2000;

const MODIFIER_KEYS = new Set(["Control", "Meta"]);

/** True once Ctrl (or ⌘) has been held alone for `delay` ms. Releasing it, or
 * the window losing focus, hides the hint again. `WorktreeTree` uses this to
 * swap each row's git icon for its shortcut number.
 *
 * Keyup for a modifier is easy to miss (Alt+Tab, another app grabbing focus),
 * so `blur` also resets — otherwise the hint could latch on forever. */
export function useCtrlHoldHint(delay = CTRL_HOLD_HINT_DELAY_MS): boolean {
  const [revealed, setRevealed] = useState(false);

  useEffect(() => {
    const held = new Set<string>();
    let timer: number | undefined;

    function clearTimer() {
      if (timer === undefined) return;
      window.clearTimeout(timer);
      timer = undefined;
    }

    function onKeyDown(e: KeyboardEvent) {
      if (!MODIFIER_KEYS.has(e.key)) return;
      // Auto-repeat fires continuously while held; only the first keydown
      // starts the timer.
      if (held.has(e.key)) return;
      held.add(e.key);
      if (held.size === 1) {
        timer = window.setTimeout(() => {
          timer = undefined;
          setRevealed(true);
        }, delay);
      }
    }

    function onKeyUp(e: KeyboardEvent) {
      if (!MODIFIER_KEYS.has(e.key)) return;
      held.delete(e.key);
      if (held.size === 0) {
        clearTimer();
        setRevealed(false);
      }
    }

    function onBlur() {
      held.clear();
      clearTimer();
      setRevealed(false);
    }

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("blur", onBlur);
    return () => {
      clearTimer();
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("blur", onBlur);
    };
  }, [delay]);

  return revealed;
}
