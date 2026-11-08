import { ImageResponse } from "next/og";
import { OG_SIZE, OgMark, ogPhoto } from "@/lib/og";

export const size = OG_SIZE;
export const contentType = "image/png";
export const alt = "Brier: a play-money prediction market on things public data settles";

export default async function Image() {
  const photo = await ogPhoto({ tags: ["nyc"] });
  return new ImageResponse(
    (
      <div style={{ width: 1200, height: 630, display: "flex", background: "#0b0e11", color: "#f3f5f7" }}>
        <div style={{ width: 700, height: 630, display: "flex", flexDirection: "column", justifyContent: "center", gap: 28, padding: "56px 64px" }}>
          <OgMark size={88} />
          <div style={{ fontSize: 88, letterSpacing: -2 }}>Brier</div>
          <div style={{ fontSize: 36, color: "#c9d1d9", lineHeight: 1.3 }}>
            Trade what happens next: weather, earthquakes, the economy and sports, settled from public data.
          </div>
        </div>
        <img src={photo} width={500} height={630} alt="" style={{ width: 500, height: 630, objectFit: "cover" }} />
      </div>
    ),
    size,
  );
}
