// Rare UI — theme-selector (https://rareui.com)
// Copyright (c) 2026 Swami Malode. MIT + Commons Clause + Attribution; see ./LICENSE.rare-ui
"use client";

import {
  useRef,
  useState,
  type ComponentProps,
  type CSSProperties,
  type KeyboardEvent,
} from "react";
import { cn } from "@/lib/utils";

export type ThemePalette = {
  sidebar: string;
  panel: string;
  border: string;
  strong: string;
  muted: string;
  tile: string;
};

export type ThemeOption = {
  value: string;
  label: string;
  colors: ThemePalette;
};

export const DEFAULT_THEMES: ThemeOption[] = [
  {
    value: "dark",
    label: "Dark",
    colors: {
      sidebar: "#1C1C1E",
      panel: "#111112",
      border: "transparent",
      strong: "#4A4A50",
      muted: "#2C2C30",
      tile: "#232326",
    },
  },
  {
    value: "light",
    label: "Light",
    colors: {
      sidebar: "#FAFAFC",
      panel: "#FFFFFF",
      border: "rgba(0, 0, 0, 0.06)",
      strong: "#D6D6DE",
      muted: "#EBEBF0",
      tile: "#F2F2F6",
    },
  },
];

const bar = "block h-[2.2cqw] rounded-full";

function Preview({ colors }: { colors: ThemePalette }) {
  const vars = {
    "--ts-sidebar": colors.sidebar,
    "--ts-panel": colors.panel,
    "--ts-border": colors.border,
    "--ts-strong": colors.strong,
    "--ts-muted": colors.muted,
    "--ts-tile": colors.tile,
  } as CSSProperties;

  return (
    <span
      aria-hidden
      data-slot="theme-selector-preview"
      style={vars}
      // bleeds past the right and bottom edges so only the top-left corner shows
      className="absolute -bottom-[10cqw] -right-[10cqw] left-[13cqw] top-[13cqw] flex rounded-tl-[5cqw] bg-(--ts-sidebar) shadow-[0_0_0_1px_var(--ts-border),0_1px_2px_rgba(0,0,0,0.06)]"
    >
      <span className="flex w-[25cqw] shrink-0 flex-col px-[3cqw] pb-[14cqw] pt-[4cqw]">
        <span className="flex gap-[1.3cqw] px-[1.2cqw]">
          {[0, 1, 2].map((dot) => (
            <span
              key={dot}
              className="size-[2cqw] rounded-full bg-(--ts-muted)"
            />
          ))}
        </span>
        <span className="mt-[4cqw] flex flex-col gap-[1cqw]">
          <span className="flex h-[4.6cqw] items-center rounded-[1.6cqw] bg-(--ts-muted) px-[1.2cqw]">
            <span className={cn(bar, "w-[10cqw] bg-(--ts-strong)")} />
          </span>
          <span className="flex h-[4.6cqw] items-center px-[1.2cqw]">
            <span className={cn(bar, "w-[13cqw] bg-(--ts-muted)")} />
          </span>
          <span className="flex h-[4.6cqw] items-center px-[1.2cqw]">
            <span className={cn(bar, "w-[8cqw] bg-(--ts-muted)")} />
          </span>
        </span>
        <span className="mt-auto flex items-center gap-[1.6cqw] px-[1.2cqw]">
          <span className="size-[4.4cqw] shrink-0 rounded-full bg-(--ts-strong)" />
          <span className={cn(bar, "w-[7cqw] bg-(--ts-muted)")} />
        </span>
      </span>
      <span className="mt-[3cqw] flex min-w-0 flex-1 flex-col overflow-hidden rounded-tl-[4cqw] bg-(--ts-panel) pl-[5cqw] pt-[5cqw] shadow-[0_0_0_1px_var(--ts-border)]">
        <span className="flex flex-col gap-[2cqw]">
          <span className={cn(bar, "w-[11cqw] bg-(--ts-strong)")} />
          <span className={cn(bar, "w-[19cqw] bg-(--ts-muted)")} />
        </span>
        <span className="mt-[5cqw] flex gap-[2.5cqw]">
          {[0, 1, 2].map((tile) => (
            <span
              key={tile}
              className="h-[12cqw] w-[19cqw] shrink-0 rounded-[2.4cqw] bg-(--ts-tile) shadow-[0_0_0_1px_var(--ts-border)]"
            />
          ))}
        </span>
        <span className="mt-[2.5cqw] h-[20cqw] w-[70cqw] shrink-0 rounded-[2.4cqw] bg-(--ts-tile) shadow-[0_0_0_1px_var(--ts-border)]" />
      </span>
    </span>
  );
}

const STEPS = new Map([
  ["ArrowRight", 1],
  ["ArrowDown", 1],
  ["ArrowLeft", -1],
  ["ArrowUp", -1],
]);

const ring =
  "pointer-events-none absolute -inset-[3.5px] rounded-[15.5px] border-[1.5px] sm:rounded-[19.5px] transition-opacity duration-150 motion-reduce:transition-none";

export type ThemeSelectorProps = Omit<
  ComponentProps<"div">,
  "defaultValue" | "onChange"
> & {
  themes?: ThemeOption[];
  value?: string | null;
  defaultValue?: string;
  onChange?: (value: string) => void;
  labels?: boolean;
  disabled?: boolean;
  itemClassName?: string;
};

export function ThemeSelector({
  themes = DEFAULT_THEMES,
  value,
  defaultValue,
  onChange,
  labels = true,
  disabled,
  className,
  itemClassName,
  ...props
}: ThemeSelectorProps) {
  const [uncontrolled, setUncontrolled] = useState(
    defaultValue ?? themes[0]?.value,
  );
  const items = useRef<(HTMLButtonElement | null)[]>([]);

  // null keeps it controlled with nothing picked, like before the theme is known on the client
  const selected = value === undefined ? uncontrolled : value;
  const selectedIndex = themes.findIndex((theme) => theme.value === selected);

  const select = (next: string) => {
    if (next === selected) return;
    if (value === undefined) setUncontrolled(next);
    onChange?.(next);
  };

  const onKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    index: number,
  ) => {
    const step = STEPS.get(event.key);
    let target: number;
    if (event.key === "Home") target = 0;
    else if (event.key === "End") target = themes.length - 1;
    else if (step) target = (index + step + themes.length) % themes.length;
    else return;
    event.preventDefault();
    items.current[target]?.focus();
    select(themes[target].value);
  };

  return (
    <div
      role="radiogroup"
      aria-label="Theme"
      aria-disabled={disabled || undefined}
      data-slot="theme-selector"
      {...props}
      className={cn(
        "flex min-w-0 gap-5",
        disabled && "pointer-events-none opacity-50",
        className,
      )}
    >
      {themes.map((theme, index) => {
        const active = theme.value === selected;

        return (
          <div
            key={theme.value}
            data-slot="theme-selector-option"
            className="flex min-w-0 flex-col items-center gap-3"
          >
            <button
              ref={(el) => {
                items.current[index] = el;
              }}
              type="button"
              role="radio"
              aria-checked={active}
              aria-label={theme.label}
              // with no match, the first item stays reachable by tab
              tabIndex={(selectedIndex === -1 ? index === 0 : active) ? 0 : -1}
              disabled={disabled}
              data-slot="theme-selector-item"
              data-state={active ? "checked" : "unchecked"}
              onClick={() => select(theme.value)}
              onKeyDown={(event) => onKeyDown(event, index)}
              className={cn(
                "group relative aspect-[4/3] w-40 max-w-full cursor-pointer outline-none",
                itemClassName,
              )}
            >
              <span className="@container absolute inset-0 overflow-hidden rounded-xl sm:rounded-2xl bg-[#F4F4F9] transition-colors duration-150 group-hover:bg-[#EDEDF3] motion-reduce:transition-none dark:bg-[#262626] dark:group-hover:bg-[#2C2C2C]">
                <Preview colors={theme.colors} />
              </span>
              <span
                className={cn(
                  ring,
                  "border-black/10 opacity-0 dark:border-white/15",
                  !active && "group-hover:opacity-100",
                )}
              />
              <span
                data-slot="theme-selector-ring"
                className={cn(
                  ring,
                  "border-black/35 dark:border-white/40",
                  active ? "opacity-100" : "opacity-0",
                )}
              />
              <span className="pointer-events-none absolute -inset-[7px] rounded-[19px] border-[1.5px] sm:rounded-[23px] border-[#FC4C01]/0 transition-colors group-focus-visible:border-[#FC4C01]/60" />
            </button>
            {labels && (
              <span
                aria-hidden
                data-slot="theme-selector-label"
                className={cn(
                  "text-xs font-medium transition-colors duration-150 motion-reduce:transition-none",
                  active ? "text-black dark:text-white" : "text-[#868593]",
                )}
              >
                {theme.label}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}

export default ThemeSelector;
