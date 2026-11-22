import { getPosts } from "@/lib/content";
import { site } from "@/lib/site";

export const dynamic = "force-static";

const esc = (s = "") =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export function GET() {
  const items = getPosts()
    .map(
      (post) => `<item>
<title>${esc(post.title)}</title>
<link>${site.url}${post.route}</link>
<guid>${site.url}${post.route}</guid>
<pubDate>${new Date(`${post.date}T00:00:00Z`).toUTCString()}</pubDate>
<description>${esc(post.description)}</description>
</item>`,
    )
    .join("\n");

  return new Response(
    `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0"><channel>
<title>${esc(site.title)} Blog</title>
<link>${site.url}/blog</link>
<description>Updates, tutorials, and insights about Oite programming language</description>
${items}
</channel></rss>`,
    { headers: { "Content-Type": "application/rss+xml; charset=utf-8" } },
  );
}
