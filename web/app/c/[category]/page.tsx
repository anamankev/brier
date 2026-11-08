import Link from "next/link";
import { notFound } from "next/navigation";
import { renderTime } from "@/lib/server";
import { CATEGORIES, SORTS, TAG_NAMES, type EventQuery } from "@/lib/events";
import { listEvents } from "@/lib/events-server";
import { EventCard } from "@/components/EventCard";
import { CategoryTile } from "@/components/Icons";

export const dynamic = "force-dynamic";

export async function generateMetadata({ params }: { params: Promise<{ category: string }> }) {
  const { category } = await params;
  return { title: CATEGORIES.find((c) => c.slug === category)?.name ?? "Markets" };
}

type Search = { tag?: string; sort?: string; status?: string };

export default async function Category({ params, searchParams }: { params: Promise<{ category: string }>; searchParams: Promise<Search> }) {
  const { category } = await params;
  const meta = CATEGORIES.find((c) => c.slug === category);
  if (!meta) notFound();
  const s = await searchParams;
  const sort = SORTS.find((x) => x.slug === s.sort)?.slug ?? "volume_24h";
  const status: EventQuery["status"] = s.status === "settled" ? "settled" : "open";
  const now = renderTime();
  const [all, shown] = await Promise.all([
    listEvents({ category, status, limit: 200 }),
    listEvents({ category, status, sort, tag: s.tag, limit: 200 }),
  ]);
  const tags = new Map<string, number>();
  for (const e of all?.events ?? []) for (const t of e.tags) if (TAG_NAMES[t]) tags.set(t, (tags.get(t) ?? 0) + 1);
  const link = (patch: Partial<Search>) => {
    const q = new URLSearchParams();
    const next = { tag: s.tag, sort: s.sort, status: s.status, ...patch };
    for (const [k, v] of Object.entries(next)) if (v) q.set(k, v);
    const text = q.toString();
    return `/c/${category}${text ? `?${text}` : ""}`;
  };
  const kindOf = category === "science" && s.tag === "space-weather" ? "space" : category === "science" ? "earthquakes" : category;

  return (
    <div className="browse">
      <aside className="side" aria-label={`${meta.name} topics`}>
        <Link href={link({ tag: undefined })} className="side-item" aria-current={!s.tag ? "page" : undefined}>
          All <span>{all?.events.length ?? 0}</span>
        </Link>
        {[...tags.entries()]
          .sort((a, b) => TAG_NAMES[a[0]]!.localeCompare(TAG_NAMES[b[0]]!))
          .map(([t, n]) => (
            <Link key={t} href={link({ tag: t })} className="side-item" aria-current={s.tag === t ? "page" : undefined}>
              {TAG_NAMES[t]} <span>{n}</span>
            </Link>
          ))}
      </aside>

      <section aria-labelledby="cat-h">
        <header className="browse-head">
          <CategoryTile category={kindOf} />
          <div>
            <h1 id="cat-h" className="display">
              {s.tag ? `${TAG_NAMES[s.tag] ?? s.tag}` : meta.name}
            </h1>
            <p className="muted">{meta.blurb}</p>
          </div>
        </header>
        <div className="spread wrap-gap toolbar">
          <nav className="tabs" aria-label="Sort">
            {SORTS.map((x) => (
              <Link key={x.slug} href={link({ sort: x.slug })} aria-current={sort === x.slug ? "page" : undefined}>
                {x.name}
              </Link>
            ))}
          </nav>
          <nav className="seg-links" aria-label="Status">
            <Link href={link({ status: undefined })} aria-current={status === "open" ? "page" : undefined}>
              Open
            </Link>
            <Link href={link({ status: "settled" })} aria-current={status === "settled" ? "page" : undefined}>
              Settled
            </Link>
          </nav>
        </div>
        {shown && shown.events.length > 0 ? (
          <div className="grid-3">
            {shown.events.map((e) => (
              <EventCard key={e.slug} e={e} now={now} />
            ))}
          </div>
        ) : (
          <p className="card empty">{status === "settled" ? "Nothing here has settled yet." : "No open markets here right now. New days are listed as soon as forecasts are out."}</p>
        )}
      </section>
    </div>
  );
}
