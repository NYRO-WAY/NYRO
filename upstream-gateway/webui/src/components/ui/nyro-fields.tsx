import { useId, useState } from "react";
import type { InputHTMLAttributes, TextareaHTMLAttributes } from "react";

import { cn } from "@/lib/utils";

type InputValue = string | number | readonly string[] | undefined;

function hasValue(value: InputValue): boolean {
  if (value === undefined) return false;
  if (Array.isArray(value)) return value.length > 0;
  return String(value).length > 0;
}

type BaseFieldProps = {
  label?: string;
  className?: string;
  fullWidth?: boolean;
};

export type NyroTextFieldProps = BaseFieldProps &
  Omit<InputHTMLAttributes<HTMLInputElement>, "size"> & {
    numbersOnly?: boolean;
  };

export function NyroTextField({
  label,
  className,
  fullWidth = true,
  value,
  placeholder,
  numbersOnly = false,
  onChange,
  onFocus,
  onBlur,
  ...rest
}: NyroTextFieldProps) {
  const inputId = useId();
  const [focused, setFocused] = useState(false);
  const showFloating = focused || hasValue(value);
  const displayLabel = label ?? placeholder ?? "";

  return (
    <div
      className={cn(
        "nyro-field",
        fullWidth && "nyro-field-full",
        focused && "nyro-field-focused",
        showFloating && "nyro-field-has-value",
        rest.disabled && "nyro-field-disabled",
        className,
      )}
    >
      <fieldset className="nyro-field-outline" aria-hidden>
        <legend className="nyro-field-legend">
          <span>{displayLabel || " "}</span>
        </legend>
      </fieldset>
      {displayLabel && (
        <label className="nyro-field-label" htmlFor={inputId}>
          {displayLabel}
        </label>
      )}
      <input
        {...rest}
        id={inputId}
        value={value}
        placeholder=""
        className="nyro-field-input"
        inputMode={numbersOnly ? "numeric" : rest.inputMode}
        pattern={numbersOnly ? "[0-9]*" : rest.pattern}
        onChange={(event) => {
          if (numbersOnly) {
            const sanitized = event.target.value.replace(/[^0-9]/g, "");
            if (sanitized !== event.target.value) {
              event.target.value = sanitized;
            }
          }
          onChange?.(event);
        }}
        onFocus={(event) => {
          setFocused(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setFocused(false);
          onBlur?.(event);
        }}
      />
    </div>
  );
}

export type NyroTextareaFieldProps = BaseFieldProps &
  TextareaHTMLAttributes<HTMLTextAreaElement>;

export function NyroTextareaField({
  label,
  className,
  fullWidth = true,
  value,
  placeholder,
  onFocus,
  onBlur,
  rows = 5,
  ...rest
}: NyroTextareaFieldProps) {
  const inputId = useId();
  const [focused, setFocused] = useState(false);
  const showFloating = focused || hasValue(value);
  const displayLabel = label ?? placeholder ?? "";

  return (
    <div
      className={cn(
        "nyro-field nyro-field-textarea",
        fullWidth && "nyro-field-full",
        focused && "nyro-field-focused",
        showFloating && "nyro-field-has-value",
        rest.disabled && "nyro-field-disabled",
        className,
      )}
    >
      <fieldset className="nyro-field-outline" aria-hidden>
        <legend className="nyro-field-legend">
          <span>{displayLabel || " "}</span>
        </legend>
      </fieldset>
      {displayLabel && (
        <label className="nyro-field-label" htmlFor={inputId}>
          {displayLabel}
        </label>
      )}
      <textarea
        {...rest}
        id={inputId}
        value={value}
        rows={rows}
        placeholder=""
        className="nyro-field-input nyro-field-textarea-input"
        onFocus={(event) => {
          setFocused(true);
          onFocus?.(event);
        }}
        onBlur={(event) => {
          setFocused(false);
          onBlur?.(event);
        }}
      />
    </div>
  );
}
