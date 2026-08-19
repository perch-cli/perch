// The favicon, served from the same file as the logo and the hero image.
//
// Starlight's `favicon` option is a URL, not an import: whatever it points at has
// to be a path this site actually serves, and the only paths a bare file gets are
// the ones under `public/`. Putting the icon there would make a second copy of it
// for a third use of one drawing, so this route serves the first copy instead.
//
// `?raw` rather than `readFile`, so a rename of the icon is a build error here
// rather than an empty response.
import icon from "../../../docs/assets/icon.svg?raw";

export function GET(): Response {
  return new Response(icon, { headers: { "Content-Type": "image/svg+xml" } });
}
