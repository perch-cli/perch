// The favicon Google's result page shows: it reads no SVG, and looks for this
// path on the server whether or not the head names it.
import { iconIco } from "../lib/icon";

export function GET(): Response {
  return new Response(iconIco(48), { headers: { "Content-Type": "image/x-icon" } });
}
