"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { GooeyNav } from "@/components/ui/gooey-nav";
import { ThemeMenu } from "@/components/theme";
import { cn } from "@/lib/utils";

export type HeaderLink = { label: string; href: string; base: string };

function GitHubMark() {
  return (
    <svg viewBox="0 0 24 24" className="size-4" fill="currentColor" aria-hidden>
      <path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.36-3.88-1.36-.52-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.24-1.28-5.24-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.04 0 0 .97-.31 3.18 1.18a11 11 0 0 1 5.78 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.58.23 2.75.12 3.04.74.81 1.18 1.83 1.18 3.09 0 4.41-2.69 5.38-5.25 5.67.41.36.78 1.06.78 2.13v3.16c0 .31.2.67.8.56A11.5 11.5 0 0 0 12 .5Z" />
    </svg>
  );
}

export function SiteHeader({ links, repo }: { links: HeaderLink[]; repo: string }) {
  const pathname = usePathname();
  // longest matching base wins, so "/" (Guides) only matches when nothing else does
  const active = links.reduce(
    (best, link, i) =>
      (pathname === link.base || pathname.startsWith(`${link.base}/`) || link.base === "") &&
      (best === -1 || link.base.length > links[best].base.length)
        ? i
        : best,
    -1,
  );

  return (
    <header className="sticky top-0 z-40 border-b bg-background/80 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-[90rem] items-center gap-6 px-4 sm:px-6">
        <Link href="/" className="flex shrink-0 items-center gap-2 font-semibold tracking-tight">
          {/* eslint-disable-next-line @next/next/no-img-element */}
          <img src="/img/owl-light.svg" alt="" className="size-7" />
          Oite
        </Link>

        <GooeyNav
          aria-label="Sections"
          className="hidden md:block"
          size="sm"
          items={links.map(({ label, href }) => ({ label, href }))}
          value={active}
        />

        <div className="ml-auto flex items-center gap-1">
          <a
            href={repo}
            target="_blank"
            rel="noreferrer"
            aria-label="GitHub repository"
            className="flex size-9 items-center justify-center rounded-lg text-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
          >
            <GitHubMark />
          </a>
          <ThemeMenu />
        </div>
      </div>

      <nav
        aria-label="Sections"
        className="flex gap-1 overflow-x-auto border-t px-4 py-2 text-sm md:hidden"
      >
        {links.map((link, i) => (
          <Link
            key={link.href}
            href={link.href}
            className={cn(
              "shrink-0 rounded-lg px-3 py-1.5",
              i === active ? "bg-brand font-medium text-white" : "text-foreground/60",
            )}
          >
            {link.label}
          </Link>
        ))}
      </nav>
    </header>
  );
}
