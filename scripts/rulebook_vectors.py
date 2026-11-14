"""Regenerate (python3 scripts/rulebook_vectors.py crates/rules/tests/vectors.json) crates/rules/tests/vectors.json: inputs, expected error codes, and for valid ones the
canonical JSON (here via json.dumps with sorted keys, valid as RFC 8785 for ASCII keys and no
floats) and its SHA-256."""
import copy
import hashlib
import json
import sys

cpi = {
    "schema": 1, "market": 101,
    "title": "CPI inflation above 3.0% in September 2026",
    "question": "Will the 12-month change in CPI-U for September 2026 be above 3.0%?",
    "source": {"provider": "bls", "series": "CUUR0000SA0", "url": "https://data.bls.gov/timeseries/CUUR0000SA0"},
    "observation": "2026-09",
    "condition": {"transform": "pct_change_12m", "op": ">", "threshold": "3.0"},
    "schedule": {"open": "2026-09-01T13:30:00Z", "close": "2026-10-15T12:00:00Z",
                 "release": "2026-10-15T12:30:00Z", "deadline": "2026-12-15T12:30:00Z"},
    "revision": "first_release",
    "void": ["no_value_by_deadline", "source_discontinued"],
}
unrate = {**copy.deepcopy(cpi), "market": 102, "title": "Unemployment at or above 4.5% in October 2026",
          "question": "Will the October 2026 unemployment rate be at least 4.5%?",
          "source": {"provider": "bls", "series": "LNS14000000", "url": "https://data.bls.gov/timeseries/LNS14000000"},
          "observation": "2026-10", "condition": {"transform": "level", "op": ">=", "threshold": "4.5"},
          "schedule": {"open": "2026-10-01T13:30:00Z", "close": "2026-11-06T13:00:00Z",
                       "release": "2026-11-06T13:30:00Z", "deadline": "2027-01-06T13:30:00Z"},
          "void": ["no_value_by_deadline"]}
fomc = {**copy.deepcopy(cpi), "market": 103, "title": "Fed upper bound above 3.75% after December 2026",
        "question": "Will the federal funds target range upper limit be above 3.75% on 17 December 2026?",
        "source": {"provider": "fred", "series": "DFEDTARU", "url": "https://fred.stlouisfed.org/series/DFEDTARU"},
        "observation": "2026-12-17", "condition": {"transform": "level", "op": ">", "threshold": "3.75"},
        "schedule": {"open": "2026-11-01T14:00:00Z", "close": "2026-12-16T18:55:00Z",
                     "release": "2026-12-16T19:00:00Z", "deadline": "2027-01-16T19:00:00Z"},
        "void": ["no_value_by_deadline"]}
dgs10 = {**copy.deepcopy(cpi), "market": 104, "title": "10-year yield below 4.00% on 31 December 2026",
         "question": "Will the 10-year Treasury yield for 31 December 2026 be below 4.00%?",
         "source": {"provider": "fred", "series": "DGS10", "url": "https://fred.stlouisfed.org/series/DGS10"},
         "observation": "2026-12-31", "condition": {"transform": "level", "op": "<", "threshold": "4"},
         "schedule": {"open": "2026-12-01T14:30:00Z", "close": "2026-12-31T00:00:00Z",
                      "release": "2027-01-04T21:15:00Z", "deadline": "2027-02-04T21:15:00Z"},
         "void": ["no_value_by_deadline"]}
tmax = {**copy.deepcopy(cpi), "market": 105, "title": "Central Park reaches 90 °F on 4 July 2027 ✓",
        "question": "Will the maximum temperature at Central Park on 4 July 2027 be at least 90 °F?",
        "source": {"provider": "noaa", "series": "USW00094728/TMAX",
                   "url": "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries&stations=USW00094728&dataTypes=TMAX"},
        "observation": "2027-07-04", "condition": {"transform": "level", "op": ">=", "threshold": "90"},
        "schedule": {"open": "2027-06-01T12:00:00Z", "close": "2027-07-03T23:59:00Z",
                     "release": "2027-07-06T00:00:00Z", "deadline": "2027-08-05T00:00:00Z"},
        "void": ["no_value_by_deadline"]}

# Schema 2: one bracket of an exclusive temperature event, confirmed automatically.
bracket = {**copy.deepcopy(tmax), "schema": 2, "market": 2001,
           "title": "Central Park high 88-89 °F on 4 July 2027",
           "question": "Will the maximum temperature at Central Park on 4 July 2027 be between 88 and 89 °F?",
           "condition": {"transform": "level", "op": "between", "threshold": "88", "upper": "89"},
           "event": {"slug": "nyc-high-2027-07-04", "title": "Highest temperature in New York on 4 July 2027?",
                     "tags": ["daily", "new-york"], "series": "nyc-daily-high", "exclusive": True, "outcome": "88-89 °F"},
           "review": "automatic"}
outer = {**copy.deepcopy(bracket), "market": 2002, "condition": {"transform": "level", "op": ">=", "threshold": "90"},
         "event": {**bracket["event"], "outcome": "90 °F or higher"}, "review": "manual"}
outer["event"].pop("series")

OPTIONAL = {(): ("event", "review"), ("condition",): ("upper",), ("event",): ("series",)}


def without_nulls(rb):
    """The form serde serializes: optional fields that are null are left out."""
    r = copy.deepcopy(rb)
    for path, keys in OPTIONAL.items():
        node = r
        for k in path:
            node = node.get(k) if isinstance(node, dict) else None
        if isinstance(node, dict):
            for k in keys:
                if k in node and node[k] is None:
                    del node[k]
    return r


def mut(base, path, value, delete=False):
    r = copy.deepcopy(base); node = r
    for k in path[:-1]:
        node = node[k]
    if delete:
        del node[path[-1]]
    else:
        node[path[-1]] = value
    return r

cases = [
    ("cpi_valid", cpi, None), ("unrate_valid", unrate, None), ("fomc_valid_release_before_effective_day", fomc, None),
    ("dgs10_valid_close_at_period_start", dgs10, None), ("tmax_valid_non_ascii_title", tmax, None),
    ("unknown_field", mut(cpi, ["notes"], "x"), "malformed"),
    ("unknown_nested_field", mut(cpi, ["source", "dataset"], "x"), "malformed"),
    ("missing_field", mut(cpi, ["revision"], None, delete=True), "malformed"),
    ("threshold_as_number", mut(cpi, ["condition", "threshold"], 3), "malformed"),
    ("unknown_op", mut(cpi, ["condition", "op"], "!="), "malformed"),
    ("unknown_provider", mut(cpi, ["source", "provider"], "yahoo"), "malformed"),
    ("schema_4", mut(cpi, ["schema"], 4), "unsupported_schema"),
    ("schema_0", mut(cpi, ["schema"], 0), "unsupported_schema"),
    ("v2_bracket_valid", bracket, None),
    ("v2_outer_bracket_valid_no_series", outer, None),
    ("v1_explicit_nulls_valid", {**copy.deepcopy(cpi), "event": None, "review": None}, None),
    ("v1_with_event", {**copy.deepcopy(cpi), "event": bracket["event"]}, "schema_fields"),
    ("v1_with_review", {**copy.deepcopy(cpi), "review": "manual"}, "schema_fields"),
    ("v1_between", mut(tmax, ["condition"], bracket["condition"]), "schema_fields"),
    ("v2_missing_event", mut(bracket, ["event"], None, delete=True), "schema_fields"),
    ("v2_missing_review", mut(bracket, ["review"], None, delete=True), "schema_fields"),
    ("v2_unknown_review", mut(bracket, ["review"], "auto"), "malformed"),
    ("v2_event_unknown_field", mut(bracket, ["event", "category"], "weather"), "malformed"),
    ("v2_exclusive_not_bool", mut(bracket, ["event", "exclusive"], "yes"), "malformed"),
    ("v2_between_without_upper", mut(bracket, ["condition", "upper"], None, delete=True), "range"),
    ("v2_upper_not_above", mut(bracket, ["condition", "upper"], "88"), "range"),
    ("v2_upper_below", mut(bracket, ["condition", "upper"], "87"), "range"),
    ("v2_upper_too_precise", mut(bracket, ["condition", "upper"], "89.5"), "range"),
    ("v2_upper_with_gt", mut(outer, ["condition", "upper"], "95"), "range"),
    ("v2_slug_uppercase", mut(bracket, ["event", "slug"], "NYC-high"), "event"),
    ("v2_slug_double_hyphen", mut(bracket, ["event", "slug"], "nyc--high"), "event"),
    ("v2_slug_trailing_hyphen", mut(bracket, ["event", "slug"], "nyc-"), "event"),
    ("v2_tags_unsorted", mut(bracket, ["event", "tags"], ["new-york", "daily"]), "event"),
    ("v2_tags_repeated", mut(bracket, ["event", "tags"], ["daily", "daily"]), "event"),
    ("v2_too_many_tags", mut(bracket, ["event", "tags"], [f"t{i}" for i in range(9)]), "event"),
    ("v2_outcome_too_long", mut(bracket, ["event", "outcome"], "x" * 41), "event"),
    ("v2_event_title_empty", mut(bracket, ["event", "title"], ""), "event"),
    ("v2_series_bad_slug", mut(bracket, ["event", "series"], "NYC daily"), "event"),
    ("market_zero", mut(cpi, ["market"], 0), "market_id"),
    ("title_padded", mut(cpi, ["title"], " CPI"), "title"),
    ("title_too_long", mut(cpi, ["title"], "x" * 121), "title"),
    ("title_control_char", mut(cpi, ["title"], "CPI\nabove"), "title"),
    ("question_empty", mut(cpi, ["question"], ""), "question"),
    ("series_not_registered", mut(cpi, ["source", "series"], "CUSR0000SA0"), "unknown_series"),
    ("series_wrong_provider", mut(cpi, ["source", "provider"], "fred"), "unknown_series"),
    ("url_mismatch", mut(cpi, ["source", "url"], "https://example.com/cpi"), "source_url"),
    ("transform_not_allowed", mut(unrate, ["condition", "transform"], "pct_change_12m"), "transform"),
    ("threshold_too_precise", mut(cpi, ["condition", "threshold"], "3.00"), "threshold"),
    ("threshold_leading_zero", mut(cpi, ["condition", "threshold"], "03.0"), "threshold"),
    ("threshold_negative_zero", mut(cpi, ["condition", "threshold"], "-0.0"), "threshold"),
    ("threshold_trailing_point", mut(cpi, ["condition", "threshold"], "3."), "threshold"),
    ("threshold_plus_sign", mut(cpi, ["condition", "threshold"], "+3.0"), "threshold"),
    ("threshold_too_large", mut(cpi, ["condition", "threshold"], "1000000"), "threshold"),
    ("observation_unpadded", mut(cpi, ["observation"], "2026-9"), "observation"),
    ("observation_wrong_kind", mut(cpi, ["observation"], "2026-09-01"), "observation"),
    ("time_with_offset", mut(cpi, ["schedule", "close"], "2026-10-15T12:00:00+00:00"), "time"),
    ("time_fraction", mut(cpi, ["schedule", "open"], "2026-09-01T13:30:00.000Z"), "time"),
    ("close_equals_release", mut(cpi, ["schedule", "close"], "2026-10-15T12:30:00Z"), "schedule_order"),
    ("open_after_close", mut(cpi, ["schedule", "open"], "2026-10-15T12:10:00Z"), "schedule_order"),
    ("live_close_after_period_start", mut(dgs10, ["schedule", "close"], "2026-12-31T15:00:00Z"), "close_after_observable"),
    ("release_inside_period", mut(mut(cpi, ["schedule", "close"], "2026-09-29T12:00:00Z"), ["schedule", "release"], "2026-09-30T12:30:00Z"), "release_before_period_end"),
    ("deadline_too_far", mut(cpi, ["schedule", "deadline"], "2027-01-16T12:30:01Z"), "deadline_too_far"),
    ("void_unsorted", mut(cpi, ["void"], ["source_discontinued", "no_value_by_deadline"]), "void_conditions"),
    ("void_repeated", mut(cpi, ["void"], ["no_value_by_deadline", "no_value_by_deadline"]), "void_conditions"),
    ("void_missing_deadline", mut(cpi, ["void"], ["source_discontinued"]), "void_conditions"),
]
# Schema 3 (G10): one market per scheduled game, settled from a sports source.
game = {
    "schema": 3, "market": 30001,
    "title": "Lakers at Celtics: Celtics win",
    "question": "Will the Boston Celtics beat the Los Angeles Lakers on 12 November 2026?",
    "source": {"provider": "balldontlie", "series": "NBA/MARGIN", "url": "https://www.balldontlie.io/",
               "game": {"id": "18447001", "starts": "2026-11-13T00:30:00Z"}},
    "observation": "2026-11-13",
    "condition": {"transform": "level", "op": ">", "threshold": "0"},
    "schedule": {"open": "2026-11-10T00:30:00Z", "close": "2026-11-13T00:30:00Z",
                 "release": "2026-11-13T04:30:00Z", "deadline": "2026-11-20T04:30:00Z"},
    "revision": "first_release",
    "void": ["no_value_by_deadline"],
    "event": {"slug": "nba-lal-bos-2026-11-12", "title": "Lakers at Celtics", "tags": ["basketball", "nba"],
              "series": "nba-games", "exclusive": True, "outcome": "Celtics"},
    "review": "manual",
}
draw = {**copy.deepcopy(game), "market": 30102, "title": "Arsenal v Chelsea: draw",
        "question": "Will Arsenal v Chelsea on 14 November 2026 end in a draw after 90 minutes?",
        "source": {"provider": "football_data", "series": "PL/MARGIN", "url": "https://www.football-data.org/",
                   "game": {"id": "537001", "starts": "2026-11-14T15:00:00Z"}},
        "observation": "2026-11-14", "condition": {"transform": "level", "op": "==", "threshold": "0"},
        "schedule": {"open": "2026-11-10T15:00:00Z", "close": "2026-11-14T15:00:00Z",
                     "release": "2026-11-14T17:30:00Z", "deadline": "2026-11-21T17:30:00Z"},
        "event": {"slug": "pl-ars-che-2026-11-14", "title": "Arsenal v Chelsea", "tags": ["premier-league", "soccer"],
                  "series": "premier-league", "exclusive": True, "outcome": "Draw"}}
cases += [
    ("v3_game_valid", game, None),
    ("v3_draw_valid", draw, None),
    ("v3_bracket_valid", mut(bracket, ["schema"], 3), None),
    ("v2_with_game", mut(bracket, ["source", "game"], game["source"]["game"]), "schema_fields"),
    ("v2_equal_op", mut(outer, ["condition", "op"], "=="), "schema_fields"),
    ("v3_game_missing", mut(game, ["source", "game"], None, delete=True), "game"),
    ("v3_game_on_daily_series", mut(mut(bracket, ["schema"], 3), ["source", "game"], game["source"]["game"]), "game"),
    ("v3_game_id_bad_char", mut(game, ["source", "game", "id"], "184 47"), "game"),
    ("v3_game_id_too_long", mut(game, ["source", "game", "id"], "9" * 33), "game"),
    ("v3_game_start_not_instant", mut(game, ["source", "game", "starts"], "2026-11-13T00:30Z"), "game"),
    ("v3_observation_not_start_date", mut(game, ["observation"], "2026-11-12"), "game"),
    ("v3_close_after_start", mut(game, ["schedule", "close"], "2026-11-13T00:31:00Z"), "game"),
    ("v3_release_at_start", mut(mut(game, ["schedule", "close"], "2026-11-13T00:00:00Z"), ["schedule", "release"], "2026-11-13T00:30:00Z"), "game"),
    ("v3_game_unknown_field", mut(game, ["source", "game", "venue"], "TD Garden"), "malformed"),
    ("v3_automatic_review", mut(game, ["review"], "automatic"), None),  # checked by an independent source
]
out = []
for name, rb, err in cases:
    v = {"name": name, "input": rb, "error": err}
    if err is None:
        c = json.dumps(without_nulls(rb), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        v["canonical"] = c
        v["sha256"] = hashlib.sha256(c.encode()).hexdigest()
    out.append(v)
with open(sys.argv[1], "w") as f:
    json.dump(out, f, indent=1, ensure_ascii=False)
    f.write("\n")
print(len(out), "vectors")
