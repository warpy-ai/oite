# Oite docs

The documentation site for Oite: a small [Next.js](https://nextjs.org) app that
renders the Markdown files in this folder to a static site. UI components come
from [Rare UI](https://rareui.com).

## Develop

```bash
npm install
npm run dev      # http://localhost:3000
npm run build    # static site in ./out
```

## Writing docs

Content is plain Markdown (GFM) with optional front matter (`title`,
`description`, `keywords`, `sidebar_label`, `slug`).

| Folder           | URL            |
| ---------------- | -------------- |
| `docs/`          | `/…`           |
| `docs-compiler/` | `/compiler/…`  |
| `docs-unroll/`   | `/unroll/…`    |
| `docs-rolls/`    | `/rolls/…`     |
| `blog/`          | `/blog/…`      |

- **New page:** add `name.md` to a folder, then list `name` in that section's
  `nav` in [`lib/site.ts`](lib/site.ts). The build fails if a file is not listed.
- **New blog post:** add `blog/YYYY-MM-DD-name.md`. Text above
  `<!-- truncate -->` is the excerpt on the blog index.
- **Callouts:** `:::tip Optional title` … `:::` (also `note`, `info`,
  `caution`, `warning`, `danger`).
- **Links:** relative (`./other-page`) or absolute (`/compiler/intro`).

## Layout

- `app/` — routes: docs pages, blog, RSS feed, sitemap
- `lib/site.ts` — site metadata and sidebar structure
- `lib/content.ts`, `lib/markdown.tsx` — Markdown loading and rendering
- `components/ui/` — Rare UI components (code block, sidebar, table of
  contents, scroll progress, nav, theme selector), fetched from the
  [Rare UI registry](https://github.com/swamimalode07/rare-ui). They are
  licensed under MIT + Commons Clause + Attribution
  ([`components/ui/LICENSE.rare-ui`](components/ui/LICENSE.rare-ui)): keep the
  footer link to rareui.com and the notices in those files.
