// What Android's "Add to Home Screen" reads. A route rather than a file in
// `public/`, because `public/` is the installers and a push there deploys.
import { sizes } from "./[icon].png";

const base = "/perch";

export function GET(): Response {
  const manifest = {
    name: "Perch",
    short_name: "Perch",
    description:
      "Run Claude Code as whichever Claude account you want, without going through the login flow again.",
    start_url: `${base}/`,
    scope: `${base}/`,
    display: "minimal-ui",
    background_color: "#17181c",
    theme_color: "#17181c",
    icons: (["icon-192", "icon-512"] as const).map((icon) => ({
      src: `${base}/${icon}.png`,
      sizes: `${sizes[icon]}x${sizes[icon]}`,
      type: "image/png",
    })),
  };
  return new Response(JSON.stringify(manifest), {
    headers: { "Content-Type": "application/manifest+json" },
  });
}
