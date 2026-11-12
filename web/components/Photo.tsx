import Image from "next/image";
import { photoFor } from "@/lib/photos";
import { matchup } from "@/lib/teams";

type Subject = Parameters<typeof photoFor>[0] & { slug?: string };

/** A game's badge: the two teams' colours split on the diagonal, away above home, each with its
 * abbreviation. Decorative, like the photos: the title beside it names the teams. */
function GameBadge({ slug, size }: { slug: string; size: "sm" | "md" | "lg" }) {
  const g = matchup(slug)!;
  return (
    <span className={`photo game ${size}`} aria-hidden="true">
      <span className="team away" style={{ background: g.away.bg, color: g.away.fg }}>
        {g.away.code.toUpperCase()}
      </span>
      <span className="team home" style={{ background: g.home.bg, color: g.home.fg }}>
        {g.home.code.toUpperCase()}
      </span>
    </span>
  );
}

/** An event's photo, square with rounded corners, or a game's team badge. Decorative: the title
 * next to it says what the market is, so the image has empty alt text. The files are already
 * cropped and sized (256px squares, 1200px banners), so they are served as they are. */
export function Photo({ of, size = "md" }: { of: Subject; size?: "sm" | "md" | "lg" }) {
  if (matchup(of.slug)) return <GameBadge slug={of.slug!} size={size} />;
  const px = size === "lg" ? 64 : size === "sm" ? 36 : 44;
  return <Image className={`photo ${size}`} src={`/photos/${photoFor(of)}.webp`} width={px} height={px} alt="" unoptimized />;
}

/** A wide crop of the same photo, faded into the card below it: the featured event and topic
 * pages. Decorative, like the square. */
export function Banner({ of, priority = false }: { of: Parameters<typeof photoFor>[0]; priority?: boolean }) {
  return (
    <div className="banner" aria-hidden="true">
      <Image src={`/photos/wide/${photoFor(of)}.webp`} fill alt="" priority={priority} unoptimized />
    </div>
  );
}
