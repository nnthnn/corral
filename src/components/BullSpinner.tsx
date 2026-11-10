import clsx from "clsx";
import bullRun from "../assets/bull/bull-run.webp";

/** A running-bull loop — an alternate "working" cue to `LassoSpinner`, same
 * call sites/sizing. Pre-baked animated WebP (see `assets/bull/`) rather
 * than a CSS-driven transform, since the run cycle itself (not just a
 * rotation) is the animation. */
export function BullSpinner({ className }: { className?: string }) {
  return (
    <img
      src={bullRun}
      alt=""
      className={clsx("h-5 w-auto", className)}
      aria-hidden="true"
    />
  );
}
