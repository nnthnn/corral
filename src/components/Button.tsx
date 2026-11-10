import { clsx } from "clsx";
import type { ButtonHTMLAttributes } from "react";

export type ButtonVariant =
  | "default"
  | "primary"
  | "danger"
  | "danger-outlined"
  | "ghost"
  | "ghost-danger";

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  /** Icon-only button with tighter padding (`.btn.icon`). */
  icon?: boolean;
  /** Fixed-size square icon button (`.btn.icon-square`) — implies `icon`. */
  square?: boolean;
  /** Toggle-button pressed state (`.btn.icon.active`). */
  active?: boolean;
}

/** Wraps the `.btn` design system (see styles.css) so variant/icon/active
 * combinations aren't hand-assembled with clsx at every call site.
 *
 * Variants combine a color (neutral default / primary / danger) with a mode
 * (filled / outlined / ghost) — see DESIGN.md's Components section. Ghost
 * never shows a border, in any state. */
export function Button({
  variant = "default",
  icon,
  square,
  active,
  className,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={clsx(
        "btn",
        variant === "primary" && "primary",
        variant === "danger" && "danger",
        variant === "danger-outlined" && "danger-outlined",
        variant === "ghost" && "ghost",
        variant === "ghost-danger" && "ghost danger",
        (icon || square) && "icon",
        square && "icon-square",
        active && "active",
        className,
      )}
      {...rest}
    />
  );
}
