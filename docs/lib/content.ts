import fs from "node:fs";
import path from "node:path";
import matter from "gray-matter";
import { sections, type Section } from "./site";

const ROOT = process.cwd();

export type Doc = {
  section: Section;
  id: string;
  route: string;
  /** Path relative to the docs app root, for "edit this page". */
  file: string;
  title: string;
  description?: string;
  keywords?: string[];
  body: string;
};

export type SidebarItem = { label: string; href: string } | { label: string; heading: true };

const firstHeading = (body: string) => body.match(/^#\s+(.+)$/m)?.[1]?.trim();

function loadDoc(section: Section, id: string): Doc {
  const file = path.join(section.dir, `${id}.md`);
  const { data, content } = matter(fs.readFileSync(path.join(ROOT, file), "utf8"));
  const slug = typeof data.slug === "string" ? data.slug.replace(/^\/|\/$/g, "") : id;
  return {
    section,
    id,
    route: `${section.base}/${slug}`.replace(/(.)\/$/, "$1"),
    file,
    title: data.sidebar_label ?? data.title ?? firstHeading(content) ?? id,
    description: data.description,
    keywords: Array.isArray(data.keywords) ? data.keywords : undefined,
    body: content,
  };
}

let cache: Doc[] | undefined;

/** Every doc, in sidebar order. Throws if a file is missing from a section's nav. */
export function getDocs(): Doc[] {
  if (cache) return cache;
  cache = sections.flatMap((section) => {
    const ids = section.nav.flatMap((entry) => (typeof entry === "string" ? [entry] : entry.items));
    const onDisk = fs
      .readdirSync(path.join(ROOT, section.dir))
      .filter((f) => f.endsWith(".md"))
      .map((f) => f.slice(0, -3));
    const unlisted = onDisk.filter((id) => !ids.includes(id));
    if (unlisted.length) {
      throw new Error(
        `${section.dir}: ${unlisted.join(", ")} not listed in lib/site.ts — add to the "${section.id}" nav`,
      );
    }
    return ids.map((id) => loadDoc(section, id));
  });
  return cache;
}

export const getDoc = (route: string) => getDocs().find((d) => d.route === route);

export function getSidebar(section: Section): SidebarItem[] {
  const docs = getDocs().filter((d) => d.section === section);
  const item = (id: string) => {
    const doc = docs.find((d) => d.id === id)!;
    return { label: doc.title, href: doc.route };
  };
  return section.nav.flatMap((entry): SidebarItem[] =>
    typeof entry === "string"
      ? [item(entry)]
      : [{ label: entry.label, heading: true }, ...entry.items.map(item)],
  );
}

export function getNeighbours(doc: Doc) {
  const docs = getDocs().filter((d) => d.section === doc.section);
  const i = docs.indexOf(doc);
  return { prev: docs[i - 1], next: docs[i + 1] };
}

// ── Blog ────────────────────────────────────────────────────────────────────

export type Author = { name: string; title?: string; url?: string; image_url?: string };

export type Post = {
  slug: string;
  route: string;
  file: string;
  date: string;
  title: string;
  description?: string;
  authors: Author[];
  tags: string[];
  body: string;
  excerpt: string;
  readingMinutes: number;
};

const TRUNCATE = /<!--\s*truncate\s*-->/;

function readYaml<T>(file: string): Record<string, T> {
  const full = path.join(ROOT, file);
  if (!fs.existsSync(full)) return {};
  return matter(`---\n${fs.readFileSync(full, "utf8")}\n---`).data as Record<string, T>;
}

let posts: Post[] | undefined;

/** Blog posts (`blog/YYYY-MM-DD-name.md`), newest first. */
export function getPosts(): Post[] {
  if (posts) return posts;
  const authors = readYaml<Author>("blog/authors.yml");
  const tagLabels = readYaml<{ label?: string }>("blog/tags.yml");
  posts = fs
    .readdirSync(path.join(ROOT, "blog"))
    .filter((f) => /^\d{4}-\d{2}-\d{2}-.+\.md$/.test(f))
    .map((f) => {
      const file = path.join("blog", f);
      const { data, content } = matter(fs.readFileSync(path.join(ROOT, file), "utf8"));
      const slug = data.slug ?? f.slice(11, -3);
      return {
        slug,
        route: `/blog/${slug}`,
        file,
        date: f.slice(0, 10),
        title: data.title ?? firstHeading(content) ?? slug,
        description: data.description,
        authors: ((data.authors as string[]) ?? []).map((a) => authors[a] ?? { name: a }),
        tags: ((data.tags as string[]) ?? []).map((t) => tagLabels[t]?.label ?? t),
        body: content.replace(new RegExp(TRUNCATE, "g"), ""),
        excerpt: content.split(TRUNCATE)[0],
        readingMinutes: Math.max(1, Math.round(content.split(/\s+/).length / 220)),
      };
    })
    .sort((a, b) => b.date.localeCompare(a.date) || b.slug.localeCompare(a.slug));
  return posts;
}

export const formatDate = (date: string) =>
  new Date(`${date}T00:00:00Z`).toLocaleDateString("en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    timeZone: "UTC",
  });
