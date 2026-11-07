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
    ("unknown_op", mut(cpi, ["condition", "op"], "=="), "malformed"),
    ("unknown_provider", mut(cpi, ["source", "provider"], "yahoo"), "malformed"),
    ("schema_2", mut(cpi, ["schema"], 2), "unsupported_schema"),
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
out = []
for name, rb, err in cases:
    v = {"name": name, "input": rb, "error": err}
    if err is None:
        c = json.dumps(rb, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
        v["canonical"] = c
        v["sha256"] = hashlib.sha256(c.encode()).hexdigest()
    out.append(v)
with open(sys.argv[1], "w") as f:
    json.dump(out, f, indent=1, ensure_ascii=False)
    f.write("\n")
print(len(out), "vectors")
