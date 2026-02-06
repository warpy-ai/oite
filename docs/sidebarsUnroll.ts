import type { SidebarsConfig } from "@docusaurus/plugin-content-docs";

const sidebars: SidebarsConfig = {
  unrollSidebar: [
    "intro",
    {
      type: "category",
      label: "Getting Started",
      items: ["installation", "getting-started"],
    },
    {
      type: "category",
      label: "Guides",
      items: ["dependencies", "building", "testing", "formatting-linting"],
    },
    {
      type: "category",
      label: "Registry",
      items: ["registry", "publishing", "authentication"],
    },
    {
      type: "category",
      label: "Reference",
      items: [
        "cli-reference",
        "manifest-reference",
        "lockfile-reference",
        "configuration",
      ],
    },
    {
      type: "category",
      label: "Toolchain",
      items: ["upgrade"],
    },
    {
      type: "category",
      label: "Architecture",
      items: ["design", "modules", "file-type-interop"],
    },
  ],
};

export default sidebars;
