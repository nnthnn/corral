import clsx from "clsx";
import { Bell, Cog, Wrench } from "lucide-react";
import styles from "./spinners.module.css";

/** A rotating four-sail windmill — the ranch-y "something's running" cue,
 * an alternative to the plain `LassoSpinner`. Slow, mechanical turn. */
export function WindmillSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.turn, "inline-flex")} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        className={className}
        aria-hidden="true"
      >
        <path d="M12 12 4.2 4.2" />
        <path d="M12 12l7.8-7.8" />
        <path d="M12 12l7.8 7.8" />
        <path d="M12 12l-7.8 7.8" />
        <circle cx="12" cy="12" r="1.6" fill="currentColor" stroke="none" />
      </svg>
    </span>
  );
}

/** Tapered spoke outlines for the wagon wheel, as path data — shared by the
 * color and monochrome variants so their geometry can't drift. Generated
 * from the chosen 8-spoke layout (0.6 wide at the hub, 1.3 at the felloe,
 * spanning r=3.1..8.9) so the outer ends tuck under the rim band. */
const WAGON_SPOKES = [
  "M15.10 12.30 L20.90 12.65 L20.90 11.35 L15.10 11.70 Z",
  "M13.98 14.40 L17.83 18.75 L18.75 17.83 L14.40 13.98 Z",
  "M11.70 15.10 L11.35 20.90 L12.65 20.90 L12.30 15.10 Z",
  "M9.60 13.98 L5.25 17.83 L6.17 18.75 L10.02 14.40 Z",
  "M8.90 11.70 L3.10 11.35 L3.10 12.65 L8.90 12.30 Z",
  "M10.02 9.60 L6.17 5.25 L5.25 6.17 L9.60 10.02 Z",
  "M12.30 8.90 L12.65 3.10 L11.35 3.10 L11.70 8.90 Z",
  "M14.40 10.02 L18.75 6.17 L17.83 5.25 L13.98 9.60 Z",
];

/** Wagon-wheel palette — warm wood, a darker iron tire and a dark hub with a
 * pale ring. Deliberately fixed-color rather than `currentColor`: the
 * generated references showed a wheel only reads as a *wagon* wheel through
 * its materials, not its spoke geometry, and monochrome versions collapse to
 * a generic wheel. */
const WHEEL_SPOKE = "#d3a24f";
const WHEEL_FELLOE = "#b07f3a";
const WHEEL_TIRE = "#46331f";
const WHEEL_HUB = "#2a2015";
const WHEEL_BOLT = "#f2e2b0";

/** A rolling wagon wheel — warm wooden felloe, dark iron tire, a dark hub
 * with a pale ring, and tapered spokes. Rotates via the shared `.turn`
 * wrapper animation (see `spinners.module.css`). Chosen from the generated
 * reference pass (see `wheel-variants.tsx`): 8 spokes and a ring hub are the
 * cut that stays legible at the 14px sidebar size. */
export function WagonWheelSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.turn, "inline-flex")} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        className={className}
        aria-hidden="true"
      >
        <circle
          cx="12"
          cy="12"
          r="9.75"
          stroke={WHEEL_TIRE}
          strokeWidth={0.9}
        />
        <circle
          cx="12"
          cy="12"
          r="8.7"
          stroke={WHEEL_FELLOE}
          strokeWidth={1.9}
        />
        {WAGON_SPOKES.map((d) => (
          <path key={d} d={d} fill={WHEEL_SPOKE} />
        ))}
        <circle cx="12" cy="12" r="2.5" fill={WHEEL_HUB} />
        <circle
          cx="12"
          cy="12"
          r="1.6"
          fill="none"
          stroke={WHEEL_BOLT}
          strokeWidth={0.6}
        />
      </svg>
    </span>
  );
}

/** Monochrome fallback for [`WagonWheelSpinner`] — the same geometry drawn in
 * `currentColor`, for anywhere a fixed-color icon would clash with the
 * surrounding monochrome set. Noticeably less wagon-wheel-ish without the
 * wood/iron tone difference. */
export function WagonWheelSpinnerMono({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.turn, "inline-flex")} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        className={className}
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9.75" strokeWidth={0.9} />
        <circle cx="12" cy="12" r="8.7" strokeWidth={1.9} opacity={0.5} />
        {WAGON_SPOKES.map((d) => (
          <path key={d} d={d} fill="currentColor" opacity={0.9} />
        ))}
        <circle cx="12" cy="12" r="2.5" strokeWidth={1.5} />
        <circle cx="12" cy="12" r="0.9" fill="currentColor" stroke="none" />
      </svg>
    </span>
  );
}

/** A rocking wrench — "working on it" / setup in progress. */
export function WrenchSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.rock, "inline-flex")} aria-hidden="true">
      <Wrench size={16} className={className} />
    </span>
  );
}

/** A swinging dinner bell — a livelier alternative to the static 🔔 for the
 * "waiting for input" cue. The bell pivots from its top, so it reads as a
 * hung bell rather than a shape spinning in place. */
export function BellSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.swing, "inline-flex")} aria-hidden="true">
      <Bell size={16} className={className} />
    </span>
  );
}

/** A rotating gear — a universal "busy" cue for a build/job that's running. */
export function GearSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.spin, "inline-flex")} aria-hidden="true">
      <Cog size={16} className={className} />
    </span>
  );
}

/** A rotating dashed ring — the classic indeterminate progress ring, meant
 * for CI/check runs specifically so it doesn't share a glyph with the Run or
 * agent spinners. */
export function RingSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.spin, "inline-flex")} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.4}
        strokeLinecap="round"
        className={className}
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="9" strokeDasharray="3.6 3.6" />
      </svg>
    </span>
  );
}

/** A softly breathing dot — a calm "waiting / idle-but-attentive" pulse,
 * the gentlest option for states that shouldn't demand attention. */
export function PulseSpinner({ className }: { className?: string }) {
  return (
    <span className={clsx(styles.breathe, "inline-flex")} aria-hidden="true">
      <svg
        viewBox="0 0 24 24"
        fill="currentColor"
        className={className}
        aria-hidden="true"
      >
        <circle cx="12" cy="12" r="6" />
      </svg>
    </span>
  );
}
