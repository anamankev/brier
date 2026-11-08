import { renderTime } from "@/lib/server";
import { Watchlist } from "./Watchlist";

export const dynamic = "force-dynamic";
export const metadata = { title: "Watchlist" };

export default function WatchlistPage() {
  return (
    <section aria-labelledby="watch-h" style={{ marginTop: 24 }}>
      <h1 id="watch-h" className="display" style={{ marginBottom: 16 }}>
        Watchlist
      </h1>
      <Watchlist now={renderTime()} />
    </section>
  );
}
