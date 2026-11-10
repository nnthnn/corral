import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  BellSpinner,
  GearSpinner,
  PulseSpinner,
  RingSpinner,
  WagonWheelSpinner,
  WagonWheelSpinnerMono,
  WindmillSpinner,
  WrenchSpinner,
} from "./spinners";
import styles from "./spinners.module.css";

/** Every animated icon: the component, the animation class its wrapper must
 * carry, and a human name for the failure message. */
const SPINNERS: {
  name: string;
  Component: typeof WindmillSpinner;
  animation: string;
}[] = [
  {
    name: "WindmillSpinner",
    Component: WindmillSpinner,
    animation: styles.turn,
  },
  {
    name: "WagonWheelSpinner",
    Component: WagonWheelSpinner,
    animation: styles.turn,
  },
  {
    name: "WagonWheelSpinnerMono",
    Component: WagonWheelSpinnerMono,
    animation: styles.turn,
  },
  { name: "WrenchSpinner", Component: WrenchSpinner, animation: styles.rock },
  { name: "BellSpinner", Component: BellSpinner, animation: styles.swing },
  { name: "GearSpinner", Component: GearSpinner, animation: styles.spin },
  { name: "RingSpinner", Component: RingSpinner, animation: styles.spin },
  { name: "PulseSpinner", Component: PulseSpinner, animation: styles.breathe },
];

describe("animated spinners", () => {
  it.each(SPINNERS)(
    "$name applies its animation to the wrapper span, not the svg",
    ({ Component, animation }) => {
      const { container } = render(<Component className="size-4" />);

      const wrapper = container.firstElementChild as HTMLElement;
      expect(wrapper.tagName).toBe("SPAN");
      expect(wrapper).toHaveClass(animation);

      // The icon itself stays transform-free — see spinners.module.css and
      // LassoSpinner for why (webkit2gtk mishandles SVG transforms).
      const svg = wrapper.querySelector("svg");
      expect(svg).not.toBeNull();
      expect(svg).not.toHaveClass(animation);
      // The caller's sizing/color class lands on the svg, matching
      // LassoSpinner's convention.
      expect(svg).toHaveClass("size-4");
    },
  );
});
