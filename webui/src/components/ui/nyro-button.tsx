import type { ButtonHTMLAttributes, ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type NyroButtonVariant = "primary" | "secondary" | "icon";
type NyroButtonSize = "default" | "compact";

export type NyroButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: NyroButtonVariant;
  size?: NyroButtonSize;
  children?: ReactNode;
};

export function NyroButton({
  variant = "primary",
  size = "default",
  className,
  type = "button",
  children,
  ...rest
}: NyroButtonProps) {
  const buttonVariant = variant === "primary"
    ? "default"
    : variant === "secondary"
      ? "secondary"
      : "outline";

  const buttonSize = variant === "icon" ? "icon" : size === "compact" ? "sm" : "default";

  return (
    <Button
      {...rest}
      type={type}
      variant={buttonVariant}
      size={buttonSize}
      className={cn(className)}
    >
      {children}
    </Button>
  );
}
