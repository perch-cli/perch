import { docsLoader } from "@astrojs/starlight/loaders";
import { docsSchema } from "@astrojs/starlight/schema";
import { defineCollection } from "astro:content";

// `docsLoader()` reads `src/content/docs/` and takes no option for another
// directory, which is why the guide lives there rather than in `docs/guide/`
// (ADR 0062).
export const collections = {
  docs: defineCollection({ loader: docsLoader(), schema: docsSchema() }),
};
