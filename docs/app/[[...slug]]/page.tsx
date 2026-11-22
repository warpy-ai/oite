import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { MobileSidebar, PageToc, Sidebar } from "@/components/docs-nav";
import { getDoc, getDocs, getNeighbours, getSidebar } from "@/lib/content";
import { renderMarkdown } from "@/lib/markdown";
import { site } from "@/lib/site";

type Props = { params: Promise<{ slug?: string[] }> };

const routeOf = (slug?: string[]) => `/${(slug ?? []).join("/")}`;

export const dynamicParams = false;

export function generateStaticParams() {
  return getDocs().map((doc) => ({ slug: doc.route.split("/").filter(Boolean) }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const doc = getDoc(routeOf((await params).slug));
  if (!doc) return {};
  return {
    title: doc.route === "/" ? { absolute: site.title } : doc.title,
    description: doc.description,
    keywords: doc.keywords,
    alternates: { canonical: doc.route },
    openGraph: { title: doc.title, description: doc.description, url: doc.route },
  };
}

export default async function DocPage({ params }: Props) {
  const doc = getDoc(routeOf((await params).slug));
  if (!doc) notFound();

  const { content, toc, hasTitle } = renderMarkdown(doc.body, doc.route);
  const sidebar = getSidebar(doc.section);
  const { prev, next } = getNeighbours(doc);

  return (
    <div className="mx-auto flex max-w-[90rem] gap-10 px-4 sm:px-6">
      <aside className="sticky top-16 hidden h-[calc(100vh-4rem)] w-60 shrink-0 overflow-y-auto py-10 lg:block">
        <Sidebar items={sidebar} />
      </aside>

      <main className="min-w-0 flex-1 py-10">
        <MobileSidebar items={sidebar} label={`${doc.section.label} menu`} />
        <article className="prose mx-auto max-w-3xl dark:prose-invert">
          {!hasTitle && <h1>{doc.title}</h1>}
          {content}
        </article>

        <div className="mx-auto mt-16 max-w-3xl">
          <a
            href={`${site.editBase}/${doc.file}`}
            target="_blank"
            rel="noreferrer"
            className="text-sm text-muted-foreground hover:text-foreground"
          >
            Edit this page
          </a>
          <nav aria-label="Pages" className="mt-6 grid gap-4 sm:grid-cols-2">
            {prev ? (
              <Link href={prev.route} className="rounded-xl border p-4 transition-colors hover:border-brand">
                <span className="text-xs text-muted-foreground">Previous</span>
                <span className="mt-1 block font-medium">{prev.title}</span>
              </Link>
            ) : (
              <span />
            )}
            {next && (
              <Link
                href={next.route}
                className="rounded-xl border p-4 text-right transition-colors hover:border-brand"
              >
                <span className="text-xs text-muted-foreground">Next</span>
                <span className="mt-1 block font-medium">{next.title}</span>
              </Link>
            )}
          </nav>
        </div>
      </main>

      <PageToc headings={toc} className="py-10" />
    </div>
  );
}
