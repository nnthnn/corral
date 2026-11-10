import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { LassoSpinner } from "./LassoSpinner";
import styles from "./LassoSpinner.module.css";

describe("LassoSpinner", () => {
  it("applies the spin animation to a plain wrapper, not the SVG, so rotation renders in engines that mishandle SVG transforms", () => {
    const { container } = render(<LassoSpinner className="size-4" />);

    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper.tagName).toBe("SPAN");
    expect(wrapper).toHaveClass(styles.spin);

    // The lasso icon itself stays transform-free: `transform-origin: 50% 50%`
    // and CSS transform animations are unreliable directly on SVG elements
    // (webkit2gtk renders the icon but never visibly rotates it).
    const svg = wrapper.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).not.toHaveClass(styles.spin);
    expect(svg).toHaveClass("size-4");
  });
});
