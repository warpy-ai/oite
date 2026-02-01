import { themes as prismThemes } from "prism-react-renderer";
import type { Config } from "@docusaurus/types";
import type * as Preset from "@docusaurus/preset-classic";

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: "Oite Language",
  tagline: "Write fast. Run faster.",
  favicon: "img/owl.svg",

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  // Production URL
  url: "https://docs.script-lang.org",
  baseUrl: "/",

  // GitHub organization and repo
  organizationName: "warpy-ai",
  projectName: "script",

  onBrokenLinks: "throw",
  onBrokenAnchors: "warn",

  // Markdown configuration (v4 compatible)
  markdown: {
    mermaid: false,
  },

  // Internationalization
  i18n: {
    defaultLocale: "en",
    locales: ["en"],
  },

  // SEO: Head tags for all pages
  headTags: [
    {
      tagName: "meta",
      attributes: {
        name: "keywords",
        content:
          "oite language, programming language, native code, javascript alternative, high performance, compiler, memory safety, borrow checker",
      },
    },
    {
      tagName: "meta",
      attributes: {
        name: "author",
        content: "Warpy AI",
      },
    },
    {
      tagName: "link",
      attributes: {
        rel: "canonical",
        href: "https://docs.script-lang.org",
      },
    },
    // Structured data for the site
    {
      tagName: "script",
      attributes: {
        type: "application/ld+json",
      },
      innerHTML: JSON.stringify({
        "@context": "https://schema.org",
        "@type": "SoftwareSourceCode",
        name: "Oite Language",
        description:
          "A high-performance JavaScript-like programming language with native code execution and memory safety.",
        url: "https://docs.script-lang.org",
        codeRepository: "https://github.com/warpy-ai/script",
        programmingLanguage: {
          "@type": "ComputerLanguage",
          name: "Oite",
        },
        author: {
          "@type": "Organization",
          name: "Warpy AI",
          url: "https://github.com/warpy-ai",
        },
      }),
    },
  ],

  plugins: [
    [
      "@docusaurus/plugin-content-docs",
      {
        id: "compiler",
        path: "docs-compiler",
        routeBasePath: "compiler",
        sidebarPath: "./sidebarsCompiler.ts",
        editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
      },
    ],
    [
      "@docusaurus/plugin-content-docs",
      {
        id: "unroll",
        path: "docs-unroll",
        routeBasePath: "unroll",
        sidebarPath: "./sidebarsUnroll.ts",
        editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
      },
    ],
    [
      "@docusaurus/plugin-content-docs",
      {
        id: "rolls",
        path: "docs-rolls",
        routeBasePath: "rolls",
        sidebarPath: "./sidebarsRolls.ts",
        editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
      },
    ],
  ],

  presets: [
    [
      "classic",
      {
        docs: {
          sidebarPath: "./sidebars.ts",
          routeBasePath: "/",
          editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
          showLastUpdateTime: false,
          showLastUpdateAuthor: false,
        },
        blog: {
          showReadingTime: true,
          blogTitle: "Oite Language Blog",
          blogDescription:
            "Updates, tutorials, and insights about Oite programming language development",
          feedOptions: {
            type: ["rss", "atom"],
            xslt: true,
            title: "Oite Language Blog",
            description:
              "Updates, tutorials, and insights about Oite programming language",
            copyright: `Copyright © ${new Date().getFullYear()} Warpy AI`,
          },
          editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
          onInlineTags: "warn",
          onInlineAuthors: "warn",
          onUntruncatedBlogPosts: "warn",
        },
        theme: {
          customCss: "./src/css/custom.css",
        },
        sitemap: {
          lastmod: null,
          changefreq: "weekly",
          priority: 0.5,
          ignorePatterns: ["/tags/**"],
          filename: "sitemap.xml",
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    // SEO: Open Graph / Social card image (must be PNG/JPG, not SVG)
    // This should be an absolute URL for OpenGraph to work properly
    image: "https://docs.script-lang.org/img/owl-light.png",
    // SEO: Site metadata
    metadata: [
      {
        name: "description",
        content:
          "Oite is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
      },
      // OpenGraph tags must use 'property', not 'name'
      { property: "og:type", content: "website" },
      { property: "og:title", content: "Oite Language" },
      {
        property: "og:description",
        content:
          "Oite is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
      },
      {
        property: "og:image",
        content: "https://docs.script-lang.org/img/owl-light.png",
      },
      { property: "og:url", content: "https://docs.script-lang.org" },
      { property: "og:site_name", content: "Oite Language" },
      // Twitter Card tags
      { name: "twitter:card", content: "summary_large_image" },
      { name: "twitter:site", content: "@warpy_ai" },
      { name: "twitter:title", content: "Oite Language" },
      {
        name: "twitter:description",
        content:
          "Oite is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
      },
      {
        name: "twitter:image",
        content: "https://docs.script-lang.org/img/owl-light.png",
      },
      { name: "robots", content: "index, follow" },
      {
        name: "google-site-verification",
        content: "YOUR_GOOGLE_VERIFICATION_CODE",
      },
    ],
    colorMode: {
      respectPrefersColorScheme: true,
    },
    navbar: {
      title: "Oite",
      logo: {
        alt: "Oite Logo",
        src: "img/owl-light.svg",
      },
      hideOnScroll: false,
      items: [
        // Secondary row - left side (doc tabs)
        {
          to: "/compiler/intro",
          label: "Compiler",
          position: "left",
          className: "navbar__item--tab",
          activeBaseRegex: "^/compiler/",
        },
        {
          to: "/unroll/intro",
          label: "Unroll",
          position: "left",
          className: "navbar__item--tab",
          activeBaseRegex: "^/unroll/",
        },
        {
          to: "/rolls/intro",
          label: "Rolls",
          position: "left",
          className: "navbar__item--tab",
          activeBaseRegex: "^/rolls/",
        },
        // Secondary row - right side
        {
          type: "docSidebar",
          sidebarId: "tutorialSidebar",
          position: "right",
          label: "Guides",
          className: "navbar__item--secondary",
        },
        {
          to: "/blog",
          label: "Blog",
          position: "right",
          className: "navbar__item--secondary",
        },
        {
          href: "https://github.com/warpy-ai/script",
          label: "GitHub",
          position: "right",
          className: "navbar__item--secondary",
        },
        // Primary row - right side
        {
          type: "search",
          position: "right",
          className: "navbar__item--primary",
        },
        {
          to: "/getting-started",
          label: "Get Started",
          position: "right",
          className: "navbar__item--cta navbar__item--primary",
        },
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Documentation",
          items: [
            {
              label: "Getting Started",
              to: "/getting-started",
            },
            {
              label: "Compiler",
              to: "/compiler/intro",
            },
            {
              label: "Unroll",
              to: "/unroll/intro",
            },
            {
              label: "Rolls",
              to: "/rolls/intro",
            },
          ],
        },
        {
          title: "Resources",
          items: [
            {
              label: "Development Status",
              to: "/development-status",
            },
            {
              label: "Contributing",
              to: "/contributing",
            },
          ],
        },
        {
          title: "More",
          items: [
            {
              label: "Blog",
              to: "/blog",
            },
            {
              label: "GitHub",
              href: "https://github.com/warpy-ai/script",
            },
          ],
        },
      ],
      copyright: `Copyright © ${new Date().getFullYear()} Oite Language. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ["bash", "rust", "typescript", "json"],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
