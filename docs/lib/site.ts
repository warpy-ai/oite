export const site = {
  name: "Oite",
  title: "Oite Language",
  tagline: "Write fast. Run faster.",
  description:
    "Oite is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
  url: "https://docs.script-lang.org",
  repo: "https://github.com/warpy-ai/oite",
  editBase: "https://github.com/warpy-ai/oite/edit/master/docs",
  ogImage: "https://docs.script-lang.org/img/owl-light.png",
};

/** A sidebar entry: a doc id (file name without `.md`) or a labelled group of them. */
export type NavEntry = string | { label: string; items: string[] };

export type Section = {
  id: string;
  label: string;
  /** Folder holding this section's markdown files. */
  dir: string;
  /** URL prefix ("" for the root section). */
  base: string;
  nav: NavEntry[];
};

/**
 * The whole docs structure. To add a page: drop a `.md` file in the section's
 * folder and list its file name here.
 */
export const sections: Section[] = [
  {
    id: "guides",
    label: "Guides",
    dir: "docs",
    base: "",
    nav: [
      "intro",
      "getting-started",
      "lifetimes",
      "javascript-interop",
      "ecmascript-conformance",
      "platform-architecture",
      "performance",
      "object-model",
      "types",
      "development-status",
      "contributing",
    ],
  },
  {
    id: "compiler",
    label: "Compiler",
    dir: "docs-compiler",
    base: "/compiler",
    nav: [
      "intro",
      "language-features",
      "type-system",
      "architecture",
      "ssa-ir",
      "standard-library",
      "abi",
      "self-hosting",
    ],
  },
  {
    id: "unroll",
    label: "Unroll",
    dir: "docs-unroll",
    base: "/unroll",
    nav: [
      "intro",
      { label: "Getting Started", items: ["installation", "getting-started"] },
      { label: "Guides", items: ["dependencies", "building", "testing", "formatting-linting"] },
      { label: "Registry", items: ["registry", "publishing", "authentication"] },
      {
        label: "Reference",
        items: ["cli-reference", "manifest-reference", "lockfile-reference", "configuration"],
      },
      { label: "Toolchain", items: ["upgrade"] },
      { label: "Architecture", items: ["design", "modules", "file-type-interop"] },
    ],
  },
  {
    id: "rolls",
    label: "Rolls",
    dir: "docs-rolls",
    base: "/rolls",
    nav: ["intro", "design", "memory-model"],
  },
];
