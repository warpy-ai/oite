import type { NextConfig } from "next";

const config: NextConfig = {
  // Fully static site: `npm run build` writes plain HTML to ./out
  output: "export",
  images: { unoptimized: true },
};

export default config;
