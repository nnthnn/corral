import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useDialogRef } from "./useDialogRef";

function TestDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const ref = useDialogRef(open);
  return <dialog ref={ref} onClose={onClose} />;
}

describe("useDialogRef", () => {
  it("opens and closes the dialog to match the `open` prop", () => {
    const { rerender, container } = render(
      <TestDialog open={false} onClose={() => {}} />,
    );
    const dialog = container.querySelector("dialog") as HTMLDialogElement;
    expect(dialog.open).toBe(false);

    rerender(<TestDialog open={true} onClose={() => {}} />);
    expect(dialog.open).toBe(true);

    rerender(<TestDialog open={false} onClose={() => {}} />);
    expect(dialog.open).toBe(false);
  });
});
