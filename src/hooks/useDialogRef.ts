import { type RefObject, useEffect, useRef } from "react";

/** Keeps a native `<dialog>` element's open/closed state in sync with a
 * boolean prop. Only drives the store-to-DOM direction — pair this with an
 * `onClose` handler on the `<dialog>` itself (native `close`, fired on
 * Escape too) that flips the same boolean back off, or the two can drift. */
export function useDialogRef(
  open: boolean,
): RefObject<HTMLDialogElement | null> {
  const ref = useRef<HTMLDialogElement | null>(null);
  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  }, [open]);
  return ref;
}
