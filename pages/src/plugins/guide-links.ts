import type { SatteriProcessorOptions } from "@astrojs/markdown-satteri";

/** One entry of what `satteri({ hastPlugins })` accepts. */
type HastPlugin = NonNullable<SatteriProcessorOptions["hastPlugins"]>[number];

/**
 * Rewrites the guide's relative markdown links into the paths the site serves.
 *
 * A page cross-references its neighbors as `switching.md#cycling`, which GitHub
 * resolves and a browser cannot (ADR one-thing-renders-the-site). Anything of
 * another shape is refused rather than guessed at: neither this nor
 * `tests/publication.rs` would catch a link rewritten to somewhere not there.
 */
export function rewriteGuideLinks({ base }: { base: string }): HastPlugin {
  const page = /^([a-z0-9-]+)\.md(#.+)?$/;
  const prefix = base.endsWith("/") ? base : `${base}/`;

  return {
    name: "perch:guide-links",
    element: {
      filter: ["a"],
      visit(node, ctx) {
        const href = node.properties?.["href"];
        if (typeof href !== "string" || !href.includes(".md")) return;
        // A URL is somebody else's to resolve, and the splash page links to
        // `CONTEXT.md` on GitHub by one.
        if (href.startsWith("/") || /^[a-z][a-z0-9+.-]*:/i.test(href)) return;

        const named = page.exec(href);
        if (!named) {
          throw new Error(
            `${href} is a link to markdown that this site cannot serve. A guide page links to a sibling page as \`accounts.md\` or \`accounts.md#anchor\`, and to anything outside the guide by its https:// URL.`,
          );
        }
        const [, name, anchor = ""] = named;
        ctx.setProperty(node, "href", `${prefix}${name}/${anchor}`);
      },
    },
  };
}
