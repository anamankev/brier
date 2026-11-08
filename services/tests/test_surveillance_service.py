"""The live surveillance service against a fake gateway."""

from __future__ import annotations

import json
from typing import Any

from brier_services.sim import surveillance as sim
from brier_services.surveillance import service


class FakeApi:
    def __init__(self, links: list[list[int]]) -> None:
        self.links = links
        self.cases: dict[tuple[str, int, int], dict[str, Any]] = {}

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401 - JSON
        if (method, path) == ("GET", "/v1/surveillance/links"):
            return {"links": [{"accounts": a, "shared_addresses": 1} for a in self.links]}
        assert (method, path) == ("POST", "/v1/surveillance/cases")
        assert body is not None
        assert len(json.dumps(body)) <= 8 * 1024, "fits the gateway's body limit"
        key = (body["typology"], body["market"], body["account"])
        opened = key not in self.cases
        self.cases.setdefault(key, body)
        return {"case": list(self.cases).index(key) + 1, "opened": opened}


def test_a_pass_opens_one_case_per_alert_and_not_again() -> None:
    s = next(sim.run(seed) for seed in range(1, 50) if len(sim.run(seed).truth) == 5)
    commands = [c for c, _ in s.records]
    api = FakeApi([list(sim.WASH), list(sim.LINKED_NOISE)])
    logs: list[str] = []
    opened = service.run_once(api, commands, logs.append)
    found = {(o["typology"], o["account"]) for o in opened}
    # Marking the close catches about 70% (docs/notes/surveillance.md §4); the others are
    # caught in this market.
    assert {t for t in s.truth if t[0] != "close"} <= found
    case = api.cases[("wash", 1, sim.WASH[0])]
    assert case["evidence"]["timeline"], "the evidence has the account's activity"
    assert {"cash", "position", "value", "valued_at"} <= set(case["evidence"]["pnl"])
    assert service.run_once(api, commands, logs.append) == [], "a second pass opens nothing new"
