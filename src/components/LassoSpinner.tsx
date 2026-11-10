import clsx from "clsx";
import { Lasso } from "lucide-react";
import styles from "./LassoSpinner.module.css";

/** A rolling lasso loop — the ranch-themed "working" cue for the sidebar's
 * icon slot and the Agent tab. Its loop-plus-knot shape is asymmetric, so
 * rotation is actually visible at small sizes (a closed ring would look
 * identical at every angle).
 *
 * The spin is applied to a plain HTML wrapper, not the `<svg>` itself: SVG
 * elements resolve CSS `transform-origin` (especially percentage-based ones)
 * and transform animations inconsistently across engines — webkit2gtk renders
 * the icon but never visibly rotates it — whereas a regular element spins
 * reliably everywhere. */
export function LassoSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.spin, "inline-flex")} aria-hidden="true">
      <Lasso size={20} className={className} />
    </span>
  );
}
