import { themes as prismThemes } from "prism-react-renderer";
import type { Config } from "@docusaurus/types";
import type * as Preset from "@docusaurus/preset-classic";

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const config: Config = {
  title: "Script Language",
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
          "script language, programming language, native code, javascript alternative, high performance, compiler, memory safety, borrow checker",
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
        name: "Script Language",
        description:
          "A high-performance JavaScript-like programming language with native code execution and memory safety.",
        url: "https://docs.script-lang.org",
        codeRepository: "https://github.com/warpy-ai/script",
        programmingLanguage: {
          "@type": "ComputerLanguage",
          name: "Script",
        },
        author: {
          "@type": "Organization",
          name: "Warpy AI",
          url: "https://github.com/warpy-ai",
        },
      }),
    },
  ],

  presets: [
    [
      "classic",
      {
        docs: {
          sidebarPath: "./sidebars.ts",
          editUrl: "https://github.com/warpy-ai/script/tree/main/docs",
          showLastUpdateTime: false,
          showLastUpdateAuthor: false,
        },
        blog: {
          showReadingTime: true,
          blogTitle: "Script Language Blog",
          blogDescription:
            "Updates, tutorials, and insights about Script programming language development",
          feedOptions: {
            type: ["rss", "atom"],
            xslt: true,
            title: "Script Language Blog",
            description:
              "Updates, tutorials, and insights about Script programming language",
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
          "Script is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
      },
      // OpenGraph tags must use 'property', not 'name'
      { property: "og:type", content: "website" },
      { property: "og:title", content: "Script Language" },
      {
        property: "og:description",
        content:
          "Script is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
      },
      {
        property: "og:image",
        content: "https://docs.script-lang.org/img/owl-light.png",
      },
      { property: "og:url", content: "https://docs.script-lang.org" },
      { property: "og:site_name", content: "Script Language" },
      // Twitter Card tags
      { name: "twitter:card", content: "summary_large_image" },
      { name: "twitter:site", content: "@warpy_ai" },
      { name: "twitter:title", content: "Script Language" },
      {
        name: "twitter:description",
        content:
          "Script is a high-performance JavaScript-like programming language with native code execution, memory safety, and a self-hosting compiler.",
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
      title: "Script",
      logo: {
        alt: "Script Logo",
        src: "img/owl_light.png",
        srcDark: "img/owl-light.svg",
      },
      hideOnScroll: false,
      items: [
        {
          type: "docSidebar",
          sidebarId: "tutorialSidebar",
          position: "left",
          label: "Docs",
        },
        {
          to: "/docs/getting-started",
          label: "Guides",
          position: "left",
        },
        {
          to: "/docs/standard-library",
          label: "Reference",
          position: "left",
        },
        {
          to: "/blog",
          label: "Blog",
          position: "left",
        },
        {
          type: "search",
          position: "right",
        },
        {
          to: "/docs/getting-started",
          label: "Get Started",
          position: "right",
          className: "navbar__item--cta",
        },
        {
          href: "https://github.com/warpy-ai/script",
          label: "GitHub",
          position: "right",
        },
      ],
    },
    footer: {
      style: "dark",
      links: [
        {
          title: "Docs",
          items: [
            {
              label: "Getting Started",
              to: "/docs/getting-started",
            },
            {
              label: "Language Features",
              to: "/docs/language-features",
            },
            {
              label: "Architecture",
              to: "/docs/architecture",
            },
          ],
        },
        {
          title: "Resources",
          items: [
            {
              label: "Development Status",
              to: "/docs/development-status",
            },
            {
              label: "Contributing",
              to: "/docs/contributing",
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
      copyright: `Copyright © ${new Date().getFullYear()} Script Language. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.dracula,
      additionalLanguages: ["bash", "rust", "typescript", "json"],
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
