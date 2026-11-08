import type { Metadata } from "next";
import Image from "next/image";
import { LICENSE_URLS, PHOTOS, type PhotoKey } from "@/lib/photos";

export const metadata: Metadata = { title: "Photo credits" };

export default function Credits() {
  const keys = Object.keys(PHOTOS) as PhotoKey[];
  return (
    <article className="credits">
      <h1 className="display">Photo credits</h1>
      <p className="dim">
        Every photo on Brier comes from Wikimedia Commons, under the licence shown. Each has been cropped to a square
        and resized. Photos under a ShareAlike licence are shared under the same licence; the rest of Brier&apos;s
        content is not affected.
      </p>
      <ul className="credit-list">
        {keys.map((k) => {
          const p: (typeof PHOTOS)[PhotoKey] = PHOTOS[k];
          const url = LICENSE_URLS[p.license];
          return (
            <li key={k} className="card">
              <Image className="photo lg" src={`/photos/${k}.webp`} width={64} height={64} alt={p.subject} unoptimized />
              <div>
                <a href={p.source} className="t">
                  {p.subject}
                </a>
                <span className="muted">
                  {p.author} · {url ? <a href={url}>{p.license}</a> : p.license}
                </span>
              </div>
            </li>
          );
        })}
      </ul>
    </article>
  );
}
