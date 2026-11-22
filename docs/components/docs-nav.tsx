"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { ChevronDown } from "lucide-react";
import { BounceSidebar, type BounceSidebarItem } from "@/components/ui/bounce-sidebar";
import { RailToc } from "@/components/ui/rail-toc";
import { ScrollProgress } from "@/components/ui/scroll-progress";
import { cn } from "@/lib/utils";

const BRAND = "#FC4C01";

function useActiveIndex(items: BounceSidebarItem[]) {
  const pathname = usePathname();
  return items.findIndex(
    (item) => typeof item !== "string" && "href" in item && item.href === pathname,
  );
}

/** Section sidebar (Rare UI BounceSidebar), pinned beside the article on desktop. */
export function Sidebar({ items }: { items: BounceSidebarItem[] }) {
  return <BounceSidebar items={items} value={useActiveIndex(items)} dotColor={BRAND} />;
}

/** The same sidebar behind a disclosure button, for narrow screens. */
export function MobileSidebar({ items, label }: { items: BounceSidebarItem[]; label: string }) {
  const [open, setOpen] = useState(false);
  const pathname = usePathname();
  const active = useActiveIndex(items);
  useEffect(() => setOpen(false), [pathname]);

  return (
    <div className="mb-8 rounded-xl border lg:hidden">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center justify-between px-4 py-3 text-sm font-medium"
      >
        {label}
        <ChevronDown className={cn("size-4 transition-transform", open && "rotate-180")} />
      </button>
      {/* mounted on open so the sidebar measures itself while visible */}
      {open && (
        <div className="border-t px-2 py-4">
          <BounceSidebar items={items} value={active} dotColor={BRAND} />
        </div>
      )}
    </div>
  );
}

type Heading = { id: string; label: string; depth: number };

const clip = (label: string, max: number) =>
  label.length > max ? `${label.slice(0, max - 1).trimEnd()}…` : label;

/** "On this page": Rare UI RailToc column on wide screens, ScrollProgress pill below that. */
export function PageToc({ headings, className }: { headings: Heading[]; className?: string }) {
  const sections = headings.filter((h) => h.depth === 0);
  const show = headings.length > 1;
  return (
    <>
      <aside
        className={cn(
          "sticky top-16 hidden h-[calc(100vh-4rem)] w-64 shrink-0 overflow-y-auto xl:block",
          className,
        )}
      >
        {show && (
          <RailToc
            items={headings.map((h) => ({ ...h, label: clip(h.label, 30) }))}
            offset={96}
            style={{ "--rail": BRAND } as React.CSSProperties}
          />
        )}
      </aside>
      {show && sections.length > 1 && (
        <ScrollProgress
          className="xl:hidden"
          sections={sections.map((h) => ({ id: h.id, label: clip(h.label, 28) }))}
          offset={110}
        />
      )}
    </>
  );
}
