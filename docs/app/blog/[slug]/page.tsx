import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { PageToc } from "@/components/docs-nav";
import { formatDate, getPosts } from "@/lib/content";
import { renderMarkdown } from "@/lib/markdown";

type Props = { params: Promise<{ slug: string }> };

export const dynamicParams = false;

export function generateStaticParams() {
  return getPosts().map(({ slug }) => ({ slug }));
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const { slug } = await params;
  const post = getPosts().find((p) => p.slug === slug);
  if (!post) return {};
  return {
    title: post.title,
    description: post.description,
    alternates: { canonical: post.route },
    openGraph: {
      type: "article",
      title: post.title,
      description: post.description,
      url: post.route,
      publishedTime: post.date,
    },
  };
}

export default async function BlogPost({ params }: Props) {
  const { slug } = await params;
  const post = getPosts().find((p) => p.slug === slug);
  if (!post) notFound();
  const { content, toc } = renderMarkdown(post.body, post.route);

  return (
    <div className="mx-auto flex max-w-[90rem] justify-center gap-10 px-4 sm:px-6">
      <main className="min-w-0 max-w-3xl flex-1 py-12">
        <Link href="/blog" className="text-sm text-muted-foreground hover:text-foreground">
          ← All posts
        </Link>
        <header className="mt-6">
          <p className="text-sm text-muted-foreground">
            {formatDate(post.date)} · {post.readingMinutes} min read
          </p>
          <h1 className="mt-2 text-4xl font-semibold tracking-tight">{post.title}</h1>
          <div className="mt-6 flex flex-wrap items-center gap-4">
            {post.authors.map((author) => (
              <a
                key={author.name}
                href={author.url}
                target="_blank"
                rel="noreferrer"
                className="flex items-center gap-3"
              >
                {author.image_url && (
                  // eslint-disable-next-line @next/next/no-img-element
                  <img src={author.image_url} alt="" className="size-10 rounded-full" />
                )}
                <span className="text-sm">
                  <span className="block font-medium">{author.name}</span>
                  <span className="text-muted-foreground">{author.title}</span>
                </span>
              </a>
            ))}
          </div>
        </header>

        <article className="prose mt-10 dark:prose-invert">{content}</article>

        {post.tags.length > 0 && (
          <ul className="mt-12 flex flex-wrap gap-2">
            {post.tags.map((tag) => (
              <li key={tag} className="rounded-full border px-3 py-1 text-xs text-muted-foreground">
                {tag}
              </li>
            ))}
          </ul>
        )}
      </main>

      <PageToc headings={toc} className="py-12" />
    </div>
  );
}
