//! Events (G9; docs/rulebook.md schema 2): markets grouped as the outcomes of one question,
//! with the statistics a browsing page needs.
//!
//! A market listed without an event (schema 1) is shown as an event of one market, with the
//! slug `market-{id}`, so clients read one shape for everything.
//!
//! Journal commands carry no wall-clock time, so 24-hour figures come from the minute samples
//! (`score_samples`): the price and cumulative volume recorded nearest before 24 hours ago. A
//! market younger than a day is measured from its first sampled price, and all its volume is
//! within the day.
#![allow(clippy::missing_errors_doc)] // handler errors are the documented API responses

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use brier_core::{Answer, Query};
use brier_engine::MarketState;
use brier_types::MarketId;
use brier_wire::state_name;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::clock::Unix;
use crate::error::ApiError;
use crate::markets::iso;
use crate::routes::{ApiResult, Req, Shared};

/// The most events one list returns.
pub const MAX_EVENTS: usize = 200;
const DAY: Unix = 86_400;
const STANDALONE_PREFIX: &str = "market-";

/// One market's row for event pages and cards.
#[derive(Debug, Clone)]
pub(crate) struct MarketRow {
    market: i64,
    title: String,
    event: Option<String>,
    outcome: Option<String>,
    category: Option<&'static str>,
    observation: String,
    close: Unix,
    release: Unix,
    listed: Unix,
    last_price: Option<i16>,
    volume: i64,
    price_day_ago: Option<i16>,
    volume_day_ago: Option<i64>,
    range_start: Option<i128>,
}

/// The market rows, reused for `events_ttl` after a read so that concurrent requests share it.
async fn cached_rows(s: &AppState) -> Result<Arc<Vec<MarketRow>>, ApiError> {
    if s.events_ttl.is_zero() {
        return Ok(Arc::new(market_rows(s).await?));
    }
    let mut cache = s.events_cache.lock().await;
    if let Some((at, rows)) = cache.as_ref()
        && at.elapsed() < s.events_ttl
    {
        return Ok(Arc::clone(rows));
    }
    let rows = Arc::new(market_rows(s).await?);
    *cache = Some((std::time::Instant::now(), Arc::clone(&rows)));
    Ok(rows)
}

async fn market_rows(s: &AppState) -> Result<Vec<MarketRow>, ApiError> {
    let day_ago = s.clock.now().saturating_sub(DAY);
    let rows = s
        .db
        .query(
            "SELECT b.market, b.canonical, b.event_slug, b.outcome, \
                    extract(epoch FROM b.close_at)::bigint, extract(epoch FROM b.release_at)::bigint, \
                    extract(epoch FROM b.listed_at)::bigint, \
                    (SELECT f.price FROM fills f WHERE f.market = b.market ORDER BY f.seq DESC, f.ordinal DESC LIMIT 1), \
                    (SELECT coalesce(sum(f.qty), 0)::bigint FROM fills f WHERE f.market = b.market), \
                    coalesce(d.price, f.price), d.volume \
             FROM rulebooks b LEFT JOIN LATERAL ( \
                 SELECT price, volume FROM score_samples ss \
                 WHERE ss.market = b.market AND ss.at <= to_timestamp($1::bigint) ORDER BY ss.at DESC LIMIT 1 \
             ) d ON true LEFT JOIN LATERAL ( \
                 SELECT price FROM score_samples ss WHERE ss.market = b.market ORDER BY ss.at LIMIT 1 \
             ) f ON true",
            &[&day_ago],
        )
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let canonical: Value =
            serde_json::from_str(r.get(1)).map_err(|e| ApiError::internal(&e))?;
        let validated = brier_rules::validate(&canonical).map_err(|e| ApiError::internal(&e))?;
        out.push(MarketRow {
            market: r.get(0),
            title: validated.rulebook.title.clone(),
            event: r.get(2),
            outcome: r.get(3),
            category: Some(validated.series.category),
            observation: validated.rulebook.observation.clone(),
            close: r.get(4),
            release: r.get(5),
            listed: r.get(6),
            last_price: r.get(7),
            volume: r.get(8),
            price_day_ago: r.get(9),
            volume_day_ago: r.get(10),
            range_start: validated.yes_range().0,
        });
    }
    Ok(out)
}

/// What the engine says about a market now: its state and best prices.
async fn live(
    s: &AppState,
    market: i64,
) -> Result<(Option<MarketState>, Option<u8>, Option<u8>), ApiError> {
    let id = MarketId::new(u32::try_from(market).map_err(|e| ApiError::internal(&e))?);
    let Answer::Book {
        state, bids, asks, ..
    } = s.core.query(Query::Book(id)).await?
    else {
        return Err(ApiError::internal(&"book query answered another shape"));
    };
    Ok((
        state,
        bids.first().map(|(p, _)| p.ticks()),
        asks.first().map(|(p, _)| p.ticks()),
    ))
}

/// The market's chance in percent: the last trade, else the middle of the book.
fn chance(last: Option<i16>, bid: Option<u8>, ask: Option<u8>) -> Option<i16> {
    last.or_else(|| match (bid, ask) {
        (Some(b), Some(a)) => Some(i16::from(b).saturating_add(i16::from(a)).saturating_add(1) / 2),
        _ => None,
    })
}

async fn market_json(
    s: &AppState,
    m: &MarketRow,
) -> Result<(Value, Option<MarketState>), ApiError> {
    let (state, bid, ask) = live(s, m.market).await?;
    let p = chance(m.last_price, bid, ask);
    let change = p
        .zip(m.price_day_ago)
        .map(|(now, then)| now.saturating_sub(then));
    let volume_24h = m.volume.saturating_sub(m.volume_day_ago.unwrap_or(0));
    Ok((
        json!({
            "market": m.market, "title": m.title, "outcome": m.outcome,
            "state": state.map(state_name), "chance": p, "change_24h": change,
            "last_price": m.last_price, "best_bid": bid, "best_ask": ask,
            "volume": m.volume, "volume_24h": volume_24h,
        }),
        state,
    ))
}

/// An event's status from its markets' states.
fn status(states: &[Option<MarketState>]) -> &'static str {
    use MarketState::{Halted, Open, PreOpen, Resolved, Voided};
    if states.iter().any(|s| matches!(s, Some(Open | Halted))) {
        "open"
    } else if states.iter().any(|s| matches!(s, Some(PreOpen))) {
        "upcoming"
    } else if !states.is_empty() && states.iter().all(|s| matches!(s, Some(Resolved | Voided))) {
        "settled"
    } else {
        "closed"
    }
}

type EventInfo = (String, Option<String>, Vec<String>, bool);

async fn event_info(s: &AppState) -> Result<HashMap<String, EventInfo>, ApiError> {
    Ok(s.db
        .query(
            "SELECT slug, title, series, tags, exclusive FROM events",
            &[],
        )
        .await?
        .iter()
        .map(|r| (r.get(0), (r.get(1), r.get(2), r.get(3), r.get(4))))
        .collect())
}

/// Every event, each with its markets in range order, as JSON with sort keys.
async fn all_events(s: &AppState) -> Result<Vec<(Value, EventKeys)>, ApiError> {
    let info = event_info(s).await?;
    let mut groups: BTreeMap<String, Vec<MarketRow>> = BTreeMap::new();
    for m in cached_rows(s).await?.iter().cloned() {
        let slug = m
            .event
            .clone()
            .unwrap_or_else(|| format!("{STANDALONE_PREFIX}{}", m.market));
        groups.entry(slug).or_default().push(m);
    }
    let mut out = Vec::with_capacity(groups.len());
    for (slug, mut markets) in groups {
        markets.sort_by_key(|m| (m.range_start.unwrap_or(i128::MIN), m.market));
        let first = markets
            .first()
            .ok_or_else(|| ApiError::internal(&"an event without markets"))?
            .clone();
        let (title, series, tags, exclusive) = info
            .get(&slug)
            .cloned()
            .unwrap_or_else(|| (first.title.clone(), None, Vec::new(), false));
        let mut items = Vec::with_capacity(markets.len());
        let mut states = Vec::with_capacity(markets.len());
        for m in &markets {
            let (j, st) = market_json(s, m).await?;
            items.push(j);
            states.push(st);
        }
        let volume: i64 = markets.iter().map(|m| m.volume).sum();
        let volume_24h: i64 = markets
            .iter()
            .map(|m| m.volume.saturating_sub(m.volume_day_ago.unwrap_or(0)))
            .sum();
        let biggest_move = items
            .iter()
            .filter_map(|i| i["change_24h"].as_i64())
            .map(i64::abs)
            .max()
            .unwrap_or(0);
        let event_status = status(&states);
        let keys = EventKeys {
            title: title.to_lowercase(),
            category: first.category,
            tags: tags.clone(),
            series: series.clone(),
            status: event_status,
            volume,
            volume_24h,
            biggest_move,
            listed: markets.iter().map(|m| m.listed).max().unwrap_or(0),
            close: first.close,
        };
        out.push((
            json!({
                "slug": slug, "title": title, "category": first.category, "tags": tags,
                "series": series, "exclusive": exclusive, "standalone": first.event.is_none(),
                "observation": first.observation, "close": iso(first.close), "release": iso(first.release),
                "status": event_status, "volume": volume, "volume_24h": volume_24h, "markets": items,
            }),
            keys,
        ));
    }
    Ok(out)
}

struct EventKeys {
    title: String,
    category: Option<&'static str>,
    tags: Vec<String>,
    series: Option<String>,
    status: &'static str,
    volume: i64,
    volume_24h: i64,
    biggest_move: i64,
    listed: Unix,
    close: Unix,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventFilter {
    category: Option<String>,
    tag: Option<String>,
    series: Option<String>,
    status: Option<String>,
    sort: Option<String>,
    q: Option<String>,
    limit: Option<usize>,
}

/// `GET /v1/events`: events with their markets.
///
/// Filters: `category`, `tag`, `series`, `status` (`open` by default; `upcoming`, `closed`,
/// `settled`, `all`), `q` (words in the title). Sort: `volume` (default), `volume_24h`,
/// `movers` (largest 24-hour price move), `newest`, `closing`. At most `limit` (200).
pub async fn list(State(s): Shared, req: Req) -> ApiResult {
    let f: EventFilter = req.query()?;
    let status = f.status.as_deref().unwrap_or("open");
    if !["open", "upcoming", "closed", "settled", "all"].contains(&status) {
        return Err(ApiError::bad_request(
            "status must be open, upcoming, closed, settled, or all",
        ));
    }
    let sort = f.sort.as_deref().unwrap_or("volume");
    if !["volume", "volume_24h", "movers", "newest", "closing"].contains(&sort) {
        return Err(ApiError::bad_request(
            "sort must be volume, volume_24h, movers, newest, or closing",
        ));
    }
    let words: Vec<String> =
        f.q.as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
    let mut events: Vec<(Value, EventKeys)> = all_events(&s)
        .await?
        .into_iter()
        .filter(|(_, k)| {
            (status == "all" || k.status == status)
                && f.category.as_deref().is_none_or(|c| k.category == Some(c))
                && f.tag
                    .as_deref()
                    .is_none_or(|t| k.tags.iter().any(|x| x == t))
                && f.series
                    .as_deref()
                    .is_none_or(|x| k.series.as_deref() == Some(x))
                && words.iter().all(|w| k.title.contains(w.as_str()))
        })
        .collect();
    match sort {
        "volume_24h" => events.sort_by_key(|(_, k)| std::cmp::Reverse(k.volume_24h)),
        "movers" => events.sort_by_key(|(_, k)| std::cmp::Reverse(k.biggest_move)),
        "newest" => events.sort_by_key(|(_, k)| std::cmp::Reverse(k.listed)),
        "closing" => events.sort_by_key(|(_, k)| k.close),
        _ => events.sort_by_key(|(_, k)| std::cmp::Reverse(k.volume)),
    }
    let limit = f.limit.unwrap_or(MAX_EVENTS).min(MAX_EVENTS);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, k) in &events {
        if let Some(c) = k.category {
            let n = counts.entry(c).or_default();
            *n = n.saturating_add(1);
        }
    }
    let counts = json!(counts);
    let items: Vec<Value> = events.into_iter().take(limit).map(|(v, _)| v).collect();
    Ok(Json(json!({ "events": items, "categories": counts })).into_response())
}

/// `GET /v1/events/{slug}`: one event with its markets, and the other events of its series
/// (for date tabs).
pub async fn get(State(s): Shared, Path(slug): Path<String>) -> ApiResult {
    let mut all = all_events(&s).await?;
    let at = all
        .iter()
        .position(|(v, _)| v["slug"] == slug.as_str())
        .ok_or_else(|| ApiError::not_found("no such event"))?;
    let (mut event, keys) = all.swap_remove(at);
    if let Some(series) = &keys.series {
        let mut siblings: Vec<Value> = all
            .iter()
            .filter(|(_, k)| k.series.as_deref() == Some(series.as_str()))
            .map(|(v, k)| json!({ "slug": v["slug"], "observation": v["observation"], "status": k.status }))
            .collect();
        siblings.push(json!({ "slug": event.get("slug"), "observation": event.get("observation"), "status": keys.status }));
        siblings.sort_by(|a, b| a["observation"].as_str().cmp(&b["observation"].as_str()));
        if let Some(obj) = event.as_object_mut() {
            obj.insert("siblings".to_owned(), Value::Array(siblings));
        }
    }
    Ok(Json(event).into_response())
}
