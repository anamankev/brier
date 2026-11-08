import { ImageResponse } from "next/og";
import { getEvent } from "@/lib/events-server";
import { label, leading } from "@/lib/events";
import { OG_SIZE, OgMark, ogPhoto } from "@/lib/og";

export const size = OG_SIZE;
export const contentType = "image/png";
export const alt = "A Brier market: the question and its current chances";

// Text on a solid panel at left, the photo at right: legible on every photo.
export default async function Image({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const e = await getEvent(slug);
  const photo = await ogPhoto(e ?? { category: null, tags: [] });
  const top = e ? leading(e).slice(0, 3) : [];
  const single = e?.markets.length === 1;
  const title = e?.title ?? "Trade what happens next";
  return new ImageResponse(
    (
      <div style={{ width: 1200, height: 630, display: "flex", background: "#0b0e11", color: "#f3f5f7" }}>
        <div style={{ width: 700, height: 630, display: "flex", flexDirection: "column", justifyContent: "space-between", padding: "52px 56px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14, fontSize: 32 }}>
            <OgMark size={44} />
            Brier
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 30 }}>
            <div style={{ fontSize: title.length > 56 ? 46 : 56, lineHeight: 1.12, letterSpacing: -1 }}>{title}</div>
            {single ? (
              <div style={{ display: "flex", alignItems: "baseline", gap: 14 }}>
                <span style={{ fontSize: 80, color: "#3ddc97", letterSpacing: -2 }}>{top[0]?.chance == null ? "—" : `${top[0].chance}%`}</span>
                <span style={{ fontSize: 30, color: "#a9b3bd" }}>chance</span>
              </div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
                {top.map((m, i) => (
                  <div key={m.market} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "12px 20px", borderRadius: 14, background: i === 0 ? "#12352a" : "#161b21" }}>
                    <span style={{ fontSize: 28, color: "#c9d1d9" }}>{label(m)}</span>
                    <span style={{ fontSize: 36, color: i === 0 ? "#3ddc97" : "#f3f5f7" }}>{m.chance == null ? "—" : `${m.chance}%`}</span>
                  </div>
                ))}
              </div>
            )}
          </div>
          <div style={{ display: "flex", fontSize: 22, color: "#8b96a1" }}>Play-money prediction market · settled from public data</div>
        </div>
        <img src={photo} width={500} height={630} alt="" style={{ width: 500, height: 630, objectFit: "cover" }} />
      </div>
    ),
    size,
  );
}
