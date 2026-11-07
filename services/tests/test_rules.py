"""The Python rulebook twin against the shared vectors and registry.

Both files (crates/rules/tests/vectors.json, crates/rules/registry.json) are also read by the
Rust crate's tests.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import pytest
from hypothesis import given
from hypothesis import strategies as st

from brier_services import rules

ROOT = Path(__file__).resolve().parents[2]
VECTORS: list[dict[str, Any]] = json.loads(
    (ROOT / "crates" / "rules" / "tests" / "vectors.json").read_text()
)


@pytest.mark.parametrize("vector", VECTORS, ids=[v["name"] for v in VECTORS])
def test_vector(vector: dict[str, Any]) -> None:
    if vector["error"] is None:
        rb = rules.validate(vector["input"])
        assert rb.canonical == vector["canonical"]
        assert rb.hash == vector["sha256"]
    else:
        with pytest.raises(rules.RuleError) as e:
            rules.validate(vector["input"])
        assert e.value.code == vector["error"]


def test_registry_file_matches_the_code() -> None:
    file = json.loads((ROOT / "crates" / "rules" / "registry.json").read_text())
    code = [
        {
            "provider": s.provider.value,
            "id": s.id,
            "name": s.name,
            "url": s.url,
            "kind": s.kind.value,
            "measures": [
                {"transform": m.transform.value, "precision": m.precision, "unit": m.unit}
                for m in s.measures
            ],
            "category": s.category,
            "live": s.live,
            "released_after_period": s.released_after_period,
        }
        for s in rules.REGISTRY
    ]
    assert file == code


def test_rfc_8785_ordering_and_escaping() -> None:
    v = {"\u20ac": 1, "\r": 2, "\U0001f600": 3, "1": 4, "\x80": 5, "ö": 6}
    assert rules.canonical(v) == '{"\\r":2,"1":4,"\x80":5,"ö":6,"\u20ac":1,"\U0001f600":3}'
    s = 'a"b\\c\b\t\n\f\r\x01\x1f\x7f/\u2028'
    assert rules.canonical(s) == '"a\\"b\\\\c\\b\\t\\n\\f\\r\\u0001\\u001f\x7f/\u2028"'
    with pytest.raises(rules.RuleError):
        rules.canonical(2**53)
    with pytest.raises(rules.RuleError):
        rules.canonical(1.5)


def test_bool_is_not_an_integer() -> None:
    cpi = next(v["input"] for v in VECTORS if v["name"] == "cpi_valid")
    with pytest.raises(rules.RuleError) as e:
        rules.validate({**cpi, "schema": True})
    assert e.value.code == "malformed"


def test_instants() -> None:
    assert rules.instant("2026-11-07T13:45:30Z") == 1_794_059_130
    assert rules.instant("2023-02-29T00:00:00Z") is None
    assert rules.instant("2026-11-07T13:45:30+00:00") is None
    fullwidth_two = "\uff12"  # a digit to Unicode, not to the format
    assert rules.instant(fullwidth_two + "026-11-07T13:45:30Z") is None


json_values = st.recursive(
    st.none() | st.booleans() | st.integers(-(2**53) + 1, 2**53 - 1) | st.text(),
    lambda inner: st.lists(inner, max_size=5) | st.dictionaries(st.text(), inner, max_size=5),
    max_leaves=30,
)


@given(json_values)
def test_canonical_is_a_fixed_point_and_order_free(v: object) -> None:
    c = rules.canonical(v)
    back = json.loads(c)
    assert back == v
    assert rules.canonical(back) == c
    if isinstance(v, dict):
        assert rules.canonical(dict(reversed(list(v.items())))) == c
