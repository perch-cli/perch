import type { SatteriProcessorOptions } from "@astrojs/markdown-satteri";

/** One entry of what `satteri({ hastPlugins })` accepts. */
type HastPlugin = NonNullable<SatteriProcessorOptions["hastPlugins"]>[number];

/**
 * Rewrites the guide's relative markdown links into the paths the site serves.
 *
 * A guide page cross-references its neighbors as `switching.md#cycling`, because
 * the guide is read on GitHub as well as rendered here — one copy of the markdown
 * is what ADR 0035 promised and ADR 0062 carried forward, and GitHub resolves
 * that path against the directory the file is in. A browser cannot: the site
 * serves `/perch/switching/`, and nothing on it has an extension.
 *
 * So the two spellings are reconciled here rather than written twice.
 * `tests/publication.rs` asserts the markdown side — that every one of these
 * names a file that exists, and that none of them leaves the guide — and this
 * refuses to guess at anything of another shape, because a link quietly rewritten
 * to somewhere that is not there is what neither side would catch.
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
        // A URL is somebody else's to resolve, and several of these pages link to
        // markdown on GitHub by one — `CONTEXT.md`, the ADRs, the README.
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
