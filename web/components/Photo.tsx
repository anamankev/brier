import Image from "next/image";
import { photoFor } from "@/lib/photos";

/** An event's photo, square with rounded corners. Decorative: the title next to it says what
 * the market is, so the image has empty alt text. The files are already cropped and sized
 * (256px squares, 1200px banners), so they are served as they are, without Next's resizer. */
export function Photo({ of, size = "md" }: { of: Parameters<typeof photoFor>[0]; size?: "sm" | "md" | "lg" }) {
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
