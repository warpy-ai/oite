import type { MetadataRoute } from "next";
import { getDocs, getPosts } from "@/lib/content";
import { site } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  return [...getDocs(), { route: "/blog" }, ...getPosts()].map(({ route }) => ({
    url: `${site.url}${route === "/" ? "" : route}`,
    changeFrequency: "weekly",
    priority: 0.5,
  }));
}
