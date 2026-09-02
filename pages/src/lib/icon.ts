// The app icon rasterized, for every place an SVG is refused: Google's result
// page, an iOS home screen, a social card. `docs/assets/icon.svg` stays the
// one drawing, and each PNG is rendered from it at build.
//
// resvg rather than Sharp: it ships a prebuilt binary and runs no install
// script, which is what `pnpm-workspace.yaml` refuses everything but esbuild.
import { Resvg } from "@resvg/resvg-js";
import icon from "../../../docs/assets/icon.svg?raw";

// The icon's own background, which the social card extends to its edges.
const background = ["#25234F", "#12172E"] as const;

export function iconPng(size: number): Uint8Array<ArrayBuffer> {
  return render(icon, size);
}

// A 1200x630 card: the icon centered on its own gradient. No wordmark, because
// text needs a font the build machine may not have, and a card that silently
// dropped its title would be worse than one that never carried it.
export function cardPng(): Uint8Array<ArrayBuffer> {
  const [width, height, side] = [1200, 630, 400];
  const nested = icon
    .replace(/<\?xml[^>]*\?>\s*/, "")
    .replace(
      /<svg\b[^>]*>/,
      `<svg x="${(width - side) / 2}" y="${(height - side) / 2}" width="${side}" height="${side}" viewBox="0 0 1024 1024">`,
    );
  const card = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
  <defs>
    <linearGradient id="card" x1="0" y1="0" x2="${width}" y2="${height}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${background[0]}"/>
      <stop offset="1" stop-color="${background[1]}"/>
    </linearGradient>
  </defs>
  <rect width="${width}" height="${height}" fill="url(#card)"/>
  ${nested}
</svg>`;
  return render(card, width);
}

// Windows' icon container around one PNG, which every browser and Google's
// crawler read. A bitmap-encoded ICO would need a second rasterizer.
export function iconIco(size: number): Uint8Array<ArrayBuffer> {
  const png = iconPng(size);
  const header = new DataView(new ArrayBuffer(22));
  header.setUint16(2, 1, true);
  header.setUint16(4, 1, true);
  header.setUint8(6, size);
  header.setUint8(7, size);
  header.setUint16(10, 1, true);
  header.setUint16(12, 32, true);
  header.setUint32(14, png.length, true);
  header.setUint32(18, 22, true);
  const ico = new Uint8Array(22 + png.length);
  ico.set(new Uint8Array(header.buffer));
  ico.set(png, 22);
  return ico;
}

// Copied out of resvg's Buffer, which is what types it as a Response body.
function render(svg: string, width: number): Uint8Array<ArrayBuffer> {
  return new Uint8Array(
    new Resvg(svg, { fitTo: { mode: "width", value: width } }).render().asPng(),
  );
}
