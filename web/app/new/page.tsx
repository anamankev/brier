import { renderTime } from "@/lib/server";
import { listEvents } from "@/lib/events-server";
import { EventCard } from "@/components/EventCard";

export const dynamic = "force-dynamic";
export const metadata = { title: "New markets" };

export default async function NewMarkets() {
  const now = renderTime();
  const list = await listEvents({ status: "open", sort: "newest", limit: 60 });
  return (
    <section aria-labelledby="new-h" style={{ marginTop: 24 }}>
      <h1 id="new-h" className="display" style={{ marginBottom: 6 }}>
        New markets
      </h1>
      <p className="muted" style={{ marginTop: 0 }}>The most recently listed, newest first. Daily series list two days ahead.</p>
      {list && list.events.length ? (
        <div className="grid-3">
          {list.events.map((e) => (
            <EventCard key={e.slug} e={e} now={now} />
          ))}
        </div>
      ) : (
        <p className="card empty">Nothing new is open right now.</p>
      )}
    </section>
  );
}
