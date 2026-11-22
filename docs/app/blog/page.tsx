import type { Metadata } from "next";
import Link from "next/link";
import { formatDate, getPosts } from "@/lib/content";
import { renderMarkdown } from "@/lib/markdown";

export const metadata: Metadata = {
  title: "Blog",
  description: "Updates, tutorials, and insights about Oite programming language development",
  alternates: { canonical: "/blog" },
};

export default function BlogIndex() {
  return (
    <main className="mx-auto max-w-3xl px-4 py-12 sm:px-6">
      <h1 className="text-4xl font-semibold tracking-tight">Blog</h1>
      <p className="mt-3 text-muted-foreground">
        Updates, tutorials, and insights about Oite development.
      </p>

      <div className="mt-12 space-y-16">
        {getPosts().map((post) => (
          <article key={post.slug}>
            <p className="text-sm text-muted-foreground">
              {formatDate(post.date)} · {post.readingMinutes} min read
            </p>
            <h2 className="mt-2 text-2xl font-semibold tracking-tight">
              <Link href={post.route} className="hover:text-brand">
                {post.title}
              </Link>
            </h2>
            <div className="prose mt-4 dark:prose-invert">
              {renderMarkdown(post.excerpt, post.route).content}
            </div>
            <Link href={post.route} className="mt-4 inline-block text-sm font-medium text-brand">
              Read more →
            </Link>
          </article>
        ))}
      </div>
    </main>
  );
}
