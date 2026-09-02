// The icon at each size a platform asks for by name. The names are what the
// head and `site.webmanifest` say, so a rename here is a rename there.
import type { APIRoute, GetStaticPaths } from "astro";
import { iconPng } from "../lib/icon";

export const sizes = {
  "favicon-32x32": 32,
  "apple-touch-icon": 180,
  "icon-192": 192,
  "icon-512": 512,
} as const;

export const getStaticPaths = (() =>
  Object.keys(sizes).map((icon) => ({ params: { icon } }))) satisfies GetStaticPaths;

export const GET: APIRoute<never, { icon: keyof typeof sizes }> = ({ params }) =>
  new Response(iconPng(sizes[params.icon]), { headers: { "Content-Type": "image/png" } });
