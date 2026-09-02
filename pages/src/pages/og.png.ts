// The image a shared link carries. `astro.config.ts` names this path in
// `og:image`, as an absolute URL, because a card fetcher resolves nothing.
import { cardPng } from "../lib/icon";

export function GET(): Response {
  return new Response(cardPng(), { headers: { "Content-Type": "image/png" } });
}
