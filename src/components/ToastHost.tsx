import clsx from "clsx";
import { X } from "lucide-react";
import { dismissToast } from "../lib/toasts";
import { useStore } from "../store";
import { Button } from "./Button";

/** Renders the stack of non-blocking error toasts pushed via `showError` —
 * mounted once in `App.tsx` alongside the other always-mounted overlays. */
export function ToastHost() {
  const toasts = useStore((s) => s.toasts);
  if (toasts.length === 0) return null;

  return (
    <div className="toast-host fixed bottom-4 right-4 z-50 flex flex-col gap-2 w-[min(360px,calc(100vw-2rem))]">
      {toasts.map((t) => (
        <div
          key={t.id}
          role="alert"
          className="toast flex items-start gap-2 rounded-md border border-border bg-surface text-text text-body p-3"
        >
          <span
            className={clsx(
              "flex-1",
              t.variant === "success" ? "text-success" : "text-danger",
            )}
          >
            {t.message}
          </span>
          <Button
            variant="ghost"
            square
            className="flex-none size-5 p-0 text-muted"
            title="Dismiss"
            onClick={() => dismissToast(t.id)}
          >
            <X size={13} />
          </Button>
        </div>
      ))}
    </div>
  );
}
