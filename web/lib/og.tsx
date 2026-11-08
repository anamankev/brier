// Link-preview images (Open Graph, 1200x630), drawn with next/og. Shared by the site's default
// card and each event's card.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { photoFor } from "./photos";

export const OG_SIZE = { width: 1200, height: 630 };

/** The event's photo as a data URI (link previews must be self-contained). */
export async function ogPhoto(of: Parameters<typeof photoFor>[0]): Promise<string> {
  const bytes = await readFile(join(process.cwd(), "public", "photos", "og", `${photoFor(of)}.jpg`));
  return `data:image/jpeg;base64,${bytes.toString("base64")}`;
}

/** Brier's mark, drawn for next/og (no CSS variables there). */
export function OgMark({ size }: { size: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32">
      <rect width="32" height="32" rx="8" fill="#3ddc97" />
      <path d="M8 24 L24 8" stroke="#062317" strokeWidth="2.4" strokeLinecap="round" opacity="0.35" />
      <circle cx="10.5" cy="21.5" r="2.4" fill="#062317" />
      <circle cx="16" cy="15.6" r="2.4" fill="#062317" />
      <circle cx="21.5" cy="10.5" r="2.4" fill="#062317" />
    </svg>
  );
}
