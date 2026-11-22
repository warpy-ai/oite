import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

const KINDS: Record<string, { label: string; className: string }> = {
  note: { label: "Note", className: "border-l-foreground/40" },
  info: { label: "Info", className: "border-l-sky-500" },
  tip: { label: "Tip", className: "border-l-emerald-500" },
  caution: { label: "Caution", className: "border-l-amber-500" },
  warning: { label: "Warning", className: "border-l-amber-500" },
  danger: { label: "Danger", className: "border-l-red-500" },
};

export function Callout({
  kind,
  title,
  children,
}: {
  kind: string;
  title?: string;
  children: ReactNode;
}) {
  const { label, className } = KINDS[kind] ?? KINDS.note;
  return (
    <aside
      className={cn(
        "my-6 rounded-lg border border-l-4 bg-card px-4 py-3 text-[0.925em] [&>:first-child]:mt-0 [&>:last-child]:mb-0",
        className,
      )}
    >
      <p className="mb-1 font-semibold">
        <span className="text-xs uppercase tracking-wider text-muted-foreground">{label}</span>
        {title && <span className="ml-2">{title}</span>}
      </p>
      {children}
    </aside>
  );
}
