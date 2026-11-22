import type { Metadata } from "next";
import Link from "next/link";
import { GeistSans } from "geist/font/sans";
import { GeistMono } from "geist/font/mono";
import { SiteHeader } from "@/components/site-header";
import { ThemeProvider } from "@/components/theme";
import { getDocs } from "@/lib/content";
import { sections, site } from "@/lib/site";
import "./globals.css";

export const metadata: Metadata = {
  metadataBase: new URL(site.url),
  title: { default: site.title, template: `%s | ${site.title}` },
  description: site.description,
  authors: [{ name: "Warpy AI" }],
  icons: { icon: "/img/owl.svg" },
  openGraph: { type: "website", siteName: site.title, images: [site.ogImage] },
  twitter: { card: "summary_large_image", site: "@warpy_ai", images: [site.ogImage] },
  alternates: { types: { "application/rss+xml": "/blog/rss.xml" } },
};

const FOOTER = [
  {
    title: "Documentation",
    links: [
      { label: "Getting Started", href: "/getting-started" },
      { label: "Compiler", href: "/compiler/intro" },
      { label: "Unroll", href: "/unroll/intro" },
      { label: "Rolls", href: "/rolls/intro" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "Development Status", href: "/development-status" },
      { label: "Contributing", href: "/contributing" },
    ],
  },
  {
    title: "More",
    links: [
      { label: "Blog", href: "/blog" },
      { label: "GitHub", href: site.repo },
    ],
  },
];

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const docs = getDocs();
  const links = [
    ...sections.map((s) => ({
      label: s.label,
      href: docs.find((d) => d.section === s)!.route,
      base: s.base,
    })),
    { label: "Blog", href: "/blog", base: "/blog" },
  ];

  return (
    <html
      lang="en"
      className={`${GeistSans.variable} ${GeistMono.variable}`}
      suppressHydrationWarning
    >
      <body className="flex min-h-screen flex-col">
        <ThemeProvider>
          <SiteHeader links={links} repo={site.repo} />
          <div className="flex-1">{children}</div>
          <footer className="mt-24 border-t">
            <div className="mx-auto grid max-w-[90rem] gap-10 px-4 py-12 sm:grid-cols-3 sm:px-6">
              {FOOTER.map((group) => (
                <div key={group.title}>
                  <h2 className="mb-3 text-sm font-semibold">{group.title}</h2>
                  <ul className="space-y-2 text-sm text-muted-foreground">
                    {group.links.map((link) => (
                      <li key={link.href}>
                        <Link href={link.href} className="hover:text-foreground">
                          {link.label}
                        </Link>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
            <p className="border-t px-4 py-6 text-center text-sm text-muted-foreground">
              Copyright © {new Date().getFullYear()} Oite Language. Components by{" "}
              <a
                href="https://rareui.com"
                target="_blank"
                rel="noreferrer"
                className="underline underline-offset-4 hover:text-foreground"
              >
                Rare UI
              </a>
              .
            </p>
          </footer>
        </ThemeProvider>
      </body>
    </html>
  );
}
