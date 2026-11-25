import type { ReactNode } from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
import Link from "next/link";
import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkRehype from "remark-rehype";
import rehypeSlug from "rehype-slug";
import { toJsxRuntime } from "hast-util-to-jsx-runtime";
import type { Element, ElementContent, Nodes, Root as HastRoot } from "hast";
import type { Root as MdastRoot, RootContent } from "mdast";
import { Code } from "@/components/code";
import { Callout } from "@/components/callout";

export type TocItem = { id: string; label: string; depth: number };

const OPEN = "ADMONITIONOPEN";
const CLOSE = "ADMONITIONCLOSE";

/**
 * Turns `:::tip Optional title` … `:::` lines into standalone marker
 * paragraphs so they survive markdown parsing; `admonitions` regroups them.
 */
function markAdmonitions(source: string) {
  let fence: string | null = null;
  return source
    .split("\n")
    .map((line) => {
      const marker = line.match(/^\s*(`{3,}|~{3,})/)?.[1];
      if (marker) {
        if (!fence) fence = marker;
        else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null;
        return line;
      }
      if (fence) return line;
      const open = line.match(/^:::(\w+)[ \t]*(.*)$/);
      if (open) {
        const title = Buffer.from(open[2].trim()).toString("hex");
        return `\n${OPEN}x${open[1]}x${title}\n`;
      }
      return line.trim() === ":::" ? `\n${CLOSE}\n` : line;
    })
    .join("\n");
}

const markerText = (node: RootContent) =>
  node.type === "paragraph" && node.children.length === 1 && node.children[0].type === "text"
    ? node.children[0].value
    : "";

function admonitions() {
  return (tree: MdastRoot) => {
    const out: RootContent[] = [];
    let open: { kind: string; title: string; children: RootContent[] } | null = null;
    for (const node of tree.children) {
      const text = markerText(node);
      const start = text.match(new RegExp(`^${OPEN}x(\\w+?)x([0-9a-f]*)$`));
      if (start && !open) {
        open = { kind: start[1], title: Buffer.from(start[2], "hex").toString(), children: [] };
      } else if (text === CLOSE && open) {
        out.push({
          type: "blockquote",
          children: open.children as never,
          data: {
            hName: "aside",
            hProperties: { "data-kind": open.kind, "data-title": open.title },
          },
        });
        open = null;
      } else (open ? open.children : out).push(node);
    }
    if (open) out.push(...open.children);
    tree.children = out;
  };
}

const textOf = (node: Nodes): string =>
  node.type === "text" ? node.value : "children" in node ? node.children.map(textOf).join("") : "";

function resolveHref(href: string, route: string) {
  if (/^([a-z][a-z0-9+.-]*:|#|\/\/)/i.test(href)) return href;
  const dir = route.slice(0, route.lastIndexOf("/") + 1);
  const url = new URL(href, `http://x${dir}`);
  const pathname = url.pathname.replace(/\.mdx?$/, "").replace(/(.)\/$/, "$1");
  return pathname + url.hash;
}

export function renderMarkdown(source: string, route: string) {
  const processor = unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(admonitions)
    .use(remarkRehype)
    .use(rehypeSlug);
  const tree = processor.runSync(processor.parse(markAdmonitions(source))) as HastRoot;

  const toc: TocItem[] = [];
  let hasTitle = false;
  const walk = (node: Nodes) => {
    if (node.type === "element") {
      if (node.tagName === "h1") hasTitle = true;
      if ((node.tagName === "h2" || node.tagName === "h3") && node.properties.id) {
        toc.push({
          id: String(node.properties.id),
          label: textOf(node),
          depth: node.tagName === "h2" ? 0 : 1,
        });
      }
    }
    if ("children" in node) node.children.forEach(walk);
  };
  walk(tree);

  const content: ReactNode = toJsxRuntime(tree, {
    Fragment,
    jsx,
    jsxs,
    passNode: true,
    components: {
      a({ href = "", children, node: _node, ...rest }) {
        const to = resolveHref(href, route);
        return to.startsWith("/") ? (
          <Link href={to} {...rest}>
            {children}
          </Link>
        ) : to.startsWith("#") ? (
          <a href={to} {...rest}>
            {children}
          </a>
        ) : (
          <a href={to} target="_blank" rel="noreferrer" {...rest}>
            {children}
          </a>
        );
      },
      pre({ node }) {
        const code = node?.children.find(
          (c: ElementContent): c is Element => c.type === "element" && c.tagName === "code",
        );
        const classes = (code?.properties.className as string[] | undefined) ?? [];
        const language = classes.find((c) => c.startsWith("language-"))?.slice(9);
        return <Code code={code ? textOf(code) : ""} language={language} />;
      },
      table({ children, node: _node, ...rest }) {
        return (
          <div className="overflow-x-auto">
            <table {...rest}>{children}</table>
          </div>
        );
      },
      aside({ children, node }) {
        return (
          <Callout
            kind={String(node?.properties["data-kind"] ?? "note")}
            title={String(node?.properties["data-title"] ?? "")}
          >
            {children}
          </Callout>
        );
      },
    },
  });

  return { content, toc, hasTitle };
}
