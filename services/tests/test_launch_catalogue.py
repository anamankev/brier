"""The G5 exit test: every launch-catalogue market resolves end to end.

A real gateway process (built with the `test-clock` feature, in its own target directory) runs
on a clock read from a file, so the test moves through two months of schedule in seconds. For
each market in `markets/`: list it from its rulebook, trade it, let the scheduler open and close
it, resolve it with the resolver service from a response in the source's format, confirm it,
dispute one and reject the dispute, and let the settler settle it. Checked: payouts, credits
conserved (so every escrow is back to zero), the journal's rules and evidence hashes, offline
recomputation of every decision from the stored response, and the read models.

The responses: the November values cannot be recorded before they exist, so each is the
recorded response for that series (`tests/fixtures/resolution/`) with the reference period's
value added, in exactly the source's format; FRED's are written by hand (its terms).

Needs PostgreSQL (`BRIER_TEST_DATABASE_URL`) and the Rust toolchain; marked slow.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import signal
import socket
import subprocess
import time
import urllib.parse
from collections.abc import Callable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import pytest

from brier_services import resolve as service
from brier_services.client import ApiError, Ed25519Key, RestClient, clock_ms
from brier_services.resolution.evidence import EvidenceStore, recompute
from brier_services.resolution.fetch import Request, Response, check_url, redact
from brier_services.rules import Rulebook, canonical, instant, load_toml

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
PASSWORD = "a long enough password"  # noqa: S105 - a throwaway local test account
WINDOW = 3_600
GRANT = 1_000_000

pytestmark = pytest.mark.slow


def at(s: str) -> int:
    t = instant(s)
    assert t is not None
    return t


def iso(t: int) -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(t))


def bls_with(path: str, year: str, period: str, value: str) -> bytes:
    """A recorded BLS response with one more month, written the way BLS writes it."""
    doc = json.loads((FIXTURES / path).read_text())
    data = doc["Results"]["series"][0]["data"]
    for row in data:
        row.pop("latest", None)
    data.insert(
        0,
        {
            "year": year,
            "period": period,
            "periodName": "November",
            "latest": "true",
            "value": value,
            "footnotes": [{}],
        },
    )
    return json.dumps(doc, separators=(",", ":")).encode()


def fred_with(date: str, realtime_start: str, value: str) -> bytes:
    return json.dumps({
        "realtime_start": realtime_start, "realtime_end": "9999-12-31", "observation_start": date,
        "observation_end": date, "units": "lin", "output_type": 4, "file_type": "json",
        "order_by": "observation_date", "sort_order": "asc", "count": 1, "offset": 0,
        "limit": 100000,
        "observations": [{"realtime_start": realtime_start, "realtime_end": "9999-12-31",
                          "date": date, "value": value}],
    }).encode()  # fmt: skip


# What each source answers once the value exists, and the outcome that value gives.
RESPONSES: dict[int, tuple[bytes, str]] = {
    # 336.200 / 324.122 (November 2025, recorded) - 1 = 3.73%, rounded 3.7 > 3.5.
    101: (bls_with("bls_cpi_2025_2026.json", "2026", "M11", "336.200"), "yes"),
    # 4.1 is not >= 4.2.
    102: (bls_with("bls_unemployment_2025_2026.json", "2026", "M11", "4.1"), "no"),
    # A 25-basis-point cut: 3.75 < 4.00.
    103: (fred_with("2026-12-10", "2026-12-10", "3.75"), "yes"),
    # 4.4 °C is 39.92 °F, which rounds to 40 >= 40.
    105: (
        json.dumps([{"DATE": "2026-12-25", "STATION": "USW00094728", "TMAX": "4.4"}]).encode(),
        "yes",
    ),
}
# FRED has no value for 31 December 2026 (published as "."), even after the deadline: void.
MISSING_DGS10 = fred_with("2026-12-31", "2027-01-04", ".")


@dataclass
class Exchange:
    base: str
    env: dict[str, str]
    clock_file: Path
    binary: str

    def set_clock(self, t: int) -> None:
        tmp = self.clock_file.with_suffix(".tmp")
        tmp.write_text(str(t))
        tmp.replace(self.clock_file)

    def now(self) -> int:
        return int(self.clock_file.read_text())


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port: int = s.getsockname()[1]
        return port


@pytest.fixture(scope="module")
def exchange(tmp_path_factory: pytest.TempPathFactory) -> Iterator[Exchange]:
    url = os.environ.get("BRIER_TEST_DATABASE_URL")
    if not url:
        pytest.skip("set BRIER_TEST_DATABASE_URL (make db-url) for the exit test")
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    if not Path(cargo).exists():
        pytest.skip("cargo not found: the exit test needs the Rust toolchain")
    target = ROOT / "target" / "test-clock"
    subprocess.run(  # noqa: S603 - fixed arguments
        [cargo, "build", "--release", "--locked", "-p", "brier-gateway", "--features", "test-clock",
         "--target-dir", str(target)],
        cwd=ROOT, check=True,
    )  # fmt: skip
    work = tmp_path_factory.mktemp("exit")
    clock_file = work / "clock"
    port = _free_port()
    env = os.environ | {
        "BRIER_DATABASE_URL": url,
        "BRIER_DATABASE_SCHEMA": f"pytest_exit_{time.time_ns()}",
        "BRIER_DATA_DIR": str(work / "journal"),
        "BRIER_LISTEN": f"127.0.0.1:{port}",
        "BRIER_TEST_CLOCK_FILE": str(clock_file),
        "BRIER_DISPUTE_WINDOW_SECONDS": str(WINDOW),
    }
    ex = Exchange(
        f"http://127.0.0.1:{port}", env, clock_file, str(target / "release" / "brier-gateway")
    )
    ex.set_clock(at("2026-11-08T00:00:00Z"))
    process = subprocess.Popen([ex.binary, "serve"], env=env, stderr=subprocess.DEVNULL)  # noqa: S603
    deadline = time.monotonic() + 30
    while True:
        try:
            RestClient(ex.base, timeout=1).request("GET", "/v1/health")
            break
        except OSError:
            if time.monotonic() > deadline or process.poll() is not None:
                process.kill()
                raise
            time.sleep(0.1)
    yield ex
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=30) == 0, "the gateway stops cleanly on SIGTERM"


def wait_for(what: str, check: Callable[[], bool], seconds: float = 10) -> None:
    deadline = time.monotonic() + seconds
    while not check():
        if time.monotonic() > deadline:
            pytest.fail(f"timed out waiting for {what}")
        time.sleep(0.1)


def user(ex: Exchange, name: str, role: str | None = None) -> RestClient:
    RestClient(ex.base).request("POST", "/v1/register", {"username": name, "password": PASSWORD})
    if role is not None:
        subprocess.run(  # noqa: S603 - fixed arguments
            [ex.binary, "role", name, role, "exit test role"], env=ex.env, check=True,
            stderr=subprocess.DEVNULL,
        )  # fmt: skip
    c = RestClient(ex.base)
    c.login(name, PASSWORD)
    return c


def test_every_launch_market_resolves_end_to_end(exchange: Exchange, tmp_path: Path) -> None:
    ex = exchange
    public = RestClient(ex.base)
    rules: dict[int, Rulebook] = {
        r.market: r for r in map(load_toml, sorted((ROOT / "markets").glob("*.toml")))
    }
    assert sorted(rules) == [101, 102, 103, 104, 105]
    ada, cy = user(ex, "ada", "admin"), user(ex, "cyd", "admin")
    tom, uma = user(ex, "tom"), user(ex, "uma")
    resolver_session = user(ex, "res", "resolver")
    key = Ed25519Key()
    key_id = resolver_session.request("POST", "/v1/keys", {"public_key": key.public_key_b64()})[
        "key_id"
    ]
    signed = RestClient(ex.base)
    signed.use_key(key_id, key.sign, clock_ms)
    gateway = service.RestGateway(signed)

    # List every market; the gateway's rules hash is Python's, and the journal records it.
    for r in rules.values():
        listed = ada.request(
            "POST", "/v1/admin/markets", {"rulebook": r.raw, "reason": "launch catalogue"}
        )
        assert listed["rules_hash"] == r.hash

    def journaled() -> bool:
        return all(
            (public.request("GET", f"/v1/markets/{m}/rules")["journal"] or {}).get("matches")
            for m in rules
        )

    wait_for("the listings in the read models", journaled)
    with pytest.raises(ApiError) as refused:
        signed.request("POST", "/v1/orders", {"market": 101, "side": "buy", "price": 50, "qty": 1})
    assert refused.value.code == "forbidden", "a resolver cannot trade"

    def states() -> dict[int, str]:
        return {m["market"]: m["state"] for m in public.request("GET", "/v1/markets")["markets"]}

    # The scheduler opens every market at its open time; Tom buys 10 YES at 50 from Uma in each.
    ex.set_clock(at("2026-11-09T14:30:00Z"))
    wait_for("the markets to open", lambda: set(states().values()) == {"open"})
    for m in rules:
        uma.request("POST", "/v1/orders", {"market": m, "side": "sell", "price": 50, "qty": 10})
        filled = tom.request(
            "POST", "/v1/orders", {"market": m, "side": "buy", "price": 50, "qty": 10}
        )
        assert filled["filled"] == 10
    # Each closes at its own close time: unemployment, the Fed, CPI, then Christmas, the yield.
    ex.set_clock(at("2026-12-10T13:00:00Z"))
    wait_for(
        "the first three to close",
        lambda: [states()[m] for m in (101, 102, 103)] == ["closed", "closed", "closed"],
    )
    assert states()[104] == states()[105] == "open"
    ex.set_clock(at("2026-12-31T00:00:00Z"))
    wait_for("the rest to close", lambda: set(states().values()) == {"closed"})

    # After every release time, one resolver pass proposes four outcomes; DGS10 has no value yet.
    ex.set_clock(at("2027-01-04T21:00:01Z"))
    store = EvidenceStore(tmp_path / "evidence")

    def fetcher(request: Request) -> Response:
        check_url(request.url)
        q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(request.url).query))
        series = q.get("series_id") or json.loads(request.body or b"{}").get("seriesid", ["?"])[0]
        market = {"CUUR0000SA0": 101, "LNS14000000": 102, "DFEDTARU": 103, "DGS10": 104}.get(
            series, 105
        )
        body = MISSING_DGS10 if market == 104 else RESPONSES[market][0]
        sent = None if request.body is None else request.body.decode()
        return Response(redact(request.url), 200, body, iso(ex.now()), sent)

    said: list[tuple[str, dict[str, Any]]] = []
    proposed = service.run_once(
        gateway, store, ex.now(), "test-key", fetcher, lambda e, **f: said.append((e, f))
    )
    assert proposed == [101, 102, 103, 105], said
    assert ("wait", {"market": 104, "reason": "2026-12-31 published as missing"}) in said

    def live(m: int) -> dict[str, Any]:
        resolution: dict[str, Any] = public.request("GET", f"/v1/markets/{m}/resolution")[
            "resolutions"
        ][0]
        return resolution

    for m in proposed:
        assert live(m)["outcome"] == RESPONSES[m][1]
        ada.request(
            "POST",
            f"/v1/admin/resolutions/{live(m)['resolution']}/confirm",
            {"reason": "checked the release"},
        )
    dispute = tom.request(
        "POST", f"/v1/resolutions/{live(102)['resolution']}/disputes", {"reason": "I expected 4.2"}
    )
    cy.request(
        "POST",
        f"/v1/admin/disputes/{dispute['dispute']}/rule",
        {"decision": "reject", "reason": "BLS published 4.1"},
    )
    assert (
        service.run_once(gateway, store, ex.now(), "test-key", fetcher, lambda *_a, **_k: None)
        == []
    ), "live resolutions are left alone"
    ex.set_clock(ex.now() + WINDOW)
    wait_for("four settlements", lambda: [live(m)["state"] for m in proposed] == ["settled"] * 4)

    # DGS10 is still missing at the deadline: void.
    ex.set_clock(at("2027-02-04T21:00:00Z"))
    assert service.run_once(
        gateway, store, ex.now(), "test-key", fetcher, lambda *_a, **_k: None
    ) == [104]
    assert (live(104)["outcome"], live(104)["void_condition"]) == ("void", "no_value_by_deadline")
    cy.request(
        "POST",
        f"/v1/admin/resolutions/{live(104)['resolution']}/confirm",
        {"reason": "no value by the deadline"},
    )
    ex.set_clock(ex.now() + WINDOW)
    wait_for("the void settlement", lambda: live(104)["state"] == "settled")

    # Payouts: Tom held YES and Uma NO in all five (cost 500 each); YES pays 1,000, void 500 each.
    assert states() == {
        101: "resolved",
        102: "resolved",
        103: "resolved",
        104: "voided",
        105: "resolved",
    }
    tom_cash = tom.request("GET", "/v1/portfolio")["cash"]
    uma_cash = uma.request("GET", "/v1/portfolio")["cash"]
    assert tom_cash == GRANT - 5 * 500 + 3 * 1_000 + 500
    assert uma_cash == GRANT - 5 * 500 + 1_000 + 500
    assert tom_cash + uma_cash == 2 * GRANT, "every credit came back out of escrow"

    # The journal pins the rules and the evidence, and the stored response decides again.
    for m, r in rules.items():
        view = public.request("GET", f"/v1/markets/{m}/resolution")
        settled = view["resolutions"][0]
        path = f"/v1/markets/{m}/resolution"

        def projected(path: str = path) -> bool:
            return public.request("GET", path)["journal"] is not None

        wait_for(f"market {m}'s settlement in the read models", projected)
        journal = public.request("GET", f"/v1/markets/{m}/resolution")["journal"]
        evidence = settled["evidence"]
        assert (
            journal["evidence_hash"]
            == settled["evidence_hash"]
            == hashlib.sha256(canonical(evidence).encode()).hexdigest()
        )
        assert journal["outcome"] == settled["outcome"]
        assert journal["settled_seq"] == settled["settled_seq"]
        assert public.request("GET", f"/v1/markets/{m}/rules")["journal"]["rules_hash"] == r.hash
        assert recompute(evidence, r, store) == (settled["outcome"], settled["void_condition"])
        assert isinstance(evidence["body_sha256"], str)
        assert store.get(evidence["body_sha256"])
    assert [f["market"] for e, f in said if e == "proposed"] == [101, 102, 103, 105]

    # The read models carry the trades.
    fills = tom.request("GET", "/v1/fills")
    assert sorted(f["market"] for f in fills["fills"]) == [101, 102, 103, 104, 105], fills
