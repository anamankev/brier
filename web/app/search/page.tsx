import { renderTime } from "@/lib/server";
import { listEvents } from "@/lib/events-server";
import { EventCard } from "@/components/EventCard";

export const dynamic = "force-dynamic";
export const metadata = { title: "Search" };

export default async function SearchPage({ searchParams }: { searchParams: Promise<{ q?: string; all?: string }> }) {
  const { q = "", all } = await searchParams;
  const query = q.trim();
  const now = renderTime();
  const list = query ? await listEvents({ q: query, status: all ? "all" : "open", limit: 60 }) : null;
  return (
    <section aria-labelledby="search-h" style={{ marginTop: 24 }}>
      <h1 id="search-h" className="display" style={{ marginBottom: 6 }}>
        {query ? `Results for “${query}”` : "Search"}
      </h1>
      <form className="search search-page" action="/search" role="search">
        <label className="sr-only" htmlFor="q-page">
          Search markets
        </label>
        <input id="q-page" name="q" type="search" defaultValue={query} placeholder="Search markets" autoComplete="off" />
      </form>
      {query && (
        <p className="muted" style={{ marginTop: 0 }}>
          {list?.events.length ?? 0} {all ? "markets" : "open markets"}.{" "}
          <a className="link" href={`/search?q=${encodeURIComponent(query)}${all ? "" : "&all=1"}`}>
            {all ? "Open markets only" : "Include closed and settled"}
          </a>
        </p>
      )}
      {!query ? (
        <p className="card empty">Search by city, topic, or date: “Denver”, “earthquake”, “storm Nov 12”.</p>
      ) : list && list.events.length ? (
        <div className="grid-3">
          {list.events.map((e) => (
            <EventCard key={e.slug} e={e} now={now} />
          ))}
        </div>
      ) : (
        <p className="card empty">No markets match “{query}”. Every word must appear in the title.</p>
      )}
    </section>
  );
}
