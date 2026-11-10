import { useStore } from "../store";

const AUTO_DISMISS_MS = 5000;
const STICKY_DISMISS_MS = 15000;

let nextId = 0;

/** Surface a non-blocking error toast — the single place any failed command,
 * event listener, or background task should report a failure the user needs
 * to know about (replaces ad hoc `alert()`/`console.error`/silently-swallowed
 * `.catch()`). Auto-dismisses after 5s, or 15s for `sticky`; multiple toasts
 * stack. */
export function showError(message: string, opts?: { sticky?: boolean }): void {
  const id = `${Date.now()}-${nextId++}`;
  useStore.setState((s) => ({ toasts: [...s.toasts, { id, message }] }));
  window.setTimeout(
    () => dismissToast(id),
    opts?.sticky ? STICKY_DISMISS_MS : AUTO_DISMISS_MS,
  );
}

/** Surface a non-blocking success toast — same stacking/auto-dismiss
 * behavior as `showError`, styled distinctly by `ToastHost`. */
export function showSuccess(
  message: string,
  opts?: { sticky?: boolean },
): void {
  const id = `${Date.now()}-${nextId++}`;
  useStore.setState((s) => ({
    toasts: [...s.toasts, { id, message, variant: "success" }],
  }));
  window.setTimeout(
    () => dismissToast(id),
    opts?.sticky ? STICKY_DISMISS_MS : AUTO_DISMISS_MS,
  );
}

export function dismissToast(id: string): void {
  useStore.setState((s) => ({
    toasts: s.toasts.filter((t) => t.id !== id),
  }));
}
