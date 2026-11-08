"""The resolver service: propose outcomes for closed markets through the gateway API.

    python -m brier_services.resolve keygen --key-file var/resolver.key
    python -m brier_services.resolve run --gateway http://127.0.0.1:8080 \\
        --key-id 1 --key-file var/resolver.key [--once] [--interval 300]

Setup: an admin gives a user the resolver role (``brier-gateway role <user> resolver <reason>``);
that user logs in and registers the public key ``keygen`` printed (``POST /v1/keys``). The
FRED key comes from the environment (``FRED_API_KEY``), never from an argument.

Each pass, for every closed market without a live resolution: read its rulebook from the
gateway, check it against the rules hash and the journal, resolve it, store the raw response,
and propose. A proposal on a market with automatic review is then verified by a fresh fetch
at least 10 minutes later (docs/resolution.md, "Automatic review"). One JSON line is logged
per market; a failure for one market does not stop the others.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import sys
import time
from collections.abc import Callable, Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Protocol

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, NoEncryption, PrivateFormat

from brier_services.client import ApiError, Ed25519Key, RestClient, clock_ms
from brier_services.resolution.evidence import DEFAULT_DIR, EvidenceStore
from brier_services.resolution.fetch import Fetcher, FetchError, fetch
from brier_services.resolution.resolver import Proposal, SourceError, resolve
from brier_services.resolution.sources import MalformedResponseError
from brier_services.rules import Review, Rulebook, RuleError, instant, validate


class Gateway(Protocol):
    """The gateway calls the resolver makes."""

    def markets(self) -> list[dict[str, Any]]: ...
    def rules(self, market: int) -> dict[str, Any]: ...
    def resolutions(self, market: int) -> list[dict[str, Any]]: ...
    def propose(self, market: int, body: Mapping[str, Any]) -> dict[str, Any]: ...
    def verify(self, resolution: int, body: Mapping[str, Any]) -> dict[str, Any]: ...


@dataclass
class RestGateway:
    """``Gateway`` over the REST API, signing proposals with the resolver's key."""

    client: RestClient

    def markets(self) -> list[dict[str, Any]]:
        return list(self.client.request("GET", "/v1/markets")["markets"])

    def rules(self, market: int) -> dict[str, Any]:
        return dict(self.client.request("GET", f"/v1/markets/{market}/rules"))

    def resolutions(self, market: int) -> list[dict[str, Any]]:
        return list(self.client.request("GET", f"/v1/markets/{market}/resolution")["resolutions"])

    def propose(self, market: int, body: Mapping[str, Any]) -> dict[str, Any]:
        return dict(self.client.request("POST", f"/v1/markets/{market}/resolution", body))

    def verify(self, resolution: int, body: Mapping[str, Any]) -> dict[str, Any]:
        return dict(self.client.request("POST", f"/v1/resolutions/{resolution}/verify", body))


def log(event: str, **fields: Any) -> None:  # noqa: ANN401 - JSON fields
    print(json.dumps({"event": event, **fields}, sort_keys=True), file=sys.stderr)


VERIFY_AFTER = 600 + 60  # the gateway's 10 minutes, and a minute of margin for clock skew


def _verify(
    gateway: Gateway,
    store: EvidenceStore,
    rule: Rulebook,
    proposal: Mapping[str, Any],
    now: int,
    api_key: str | None,
    fetcher: Fetcher,
    say: Callable[..., None],
) -> bool:
    """Verify once it is time; returns whether a verification was sent (agreeing or not)."""
    first = instant(str(proposal.get("evidence", {}).get("retrieved_at", "")))
    if first is None or now < first + VERIFY_AFTER:
        say("verify_wait", market=rule.market, resolution=proposal["resolution"])
        return False
    result = resolve(rule, now, api_key, fetcher)
    if not isinstance(result, Proposal):
        say("verify_wait", market=rule.market, reason=result.reason)
        return False
    store.put(result.body)
    for extra in result.extra:
        store.put(extra)
    try:
        answer = gateway.verify(int(proposal["resolution"]), {"evidence": result.evidence})
        say(
            "verified",
            market=rule.market,
            resolution=proposal["resolution"],
            dispute_until=answer.get("dispute_until"),
        )
    except ApiError as e:
        say("verify_refused", market=rule.market, resolution=proposal["resolution"], error=str(e))
    return True


def run_once(
    gateway: Gateway,
    store: EvidenceStore,
    now: int,
    api_key: str | None,
    fetcher: Fetcher = fetch,
    say: Callable[..., None] = log,
    verified: set[int] | None = None,
) -> list[int]:
    """One pass; returns the markets proposed for. A proposal on a market with automatic
    review is verified by a fresh fetch at least ``VERIFY_AFTER`` seconds after its own, once
    per process (``verified``): a disagreement is the gateway's to record, not to retry."""
    proposed: list[int] = []
    verified = set() if verified is None else verified
    for m in gateway.markets():
        market = int(m["market"])
        if m.get("state") != "closed":
            continue
        try:
            live = [r for r in gateway.resolutions(market) if r["state"] != "withdrawn"]
            if live and not (live[0]["state"] == "proposed" and live[0]["outcome"] != "void"):
                continue
            published = gateway.rules(market)
            rule = validate(published["rulebook"])
            journal = published.get("journal")
            if rule.hash != published["rules_hash"] or not (journal and journal.get("matches")):
                say("rules_mismatch", market=market)
                continue
            if live:
                if (
                    rule.review is Review.AUTOMATIC
                    and market not in verified
                    and _verify(gateway, store, rule, live[0], now, api_key, fetcher, say)
                ):
                    verified.add(market)
                continue
            result = resolve(rule, now, api_key, fetcher)
            if not isinstance(result, Proposal):
                say("wait", market=market, reason=result.reason)
                continue
            store.put(result.body)
            for extra in result.extra:
                store.put(extra)
            what = f"{rule.provider} {rule.series.id} {rule.observation}"
            body: dict[str, Any] = {
                "outcome": result.outcome,
                "evidence": result.evidence,
                "reason": f"automated resolver: {what}",
            }
            if result.void_condition is not None:
                body["void_condition"] = result.void_condition
            answer = gateway.propose(market, body)
            say(
                "proposed",
                market=market,
                outcome=result.outcome,
                resolution=answer.get("resolution"),
                evidence_hash=answer.get("evidence_hash"),
            )
            proposed.append(market)
        except (
            ApiError,
            FetchError,
            SourceError,
            MalformedResponseError,
            RuleError,
            KeyError,
            ValueError,
        ) as e:
            say("error", market=market, error=f"{type(e).__name__}: {e}")
    return proposed


def keygen(path: Path) -> str:
    """Write a new private key (base64 of the 32-byte seed, mode 0600); return the public key."""
    private = Ed25519PrivateKey.generate()
    seed = private.private_bytes(Encoding.Raw, PrivateFormat.Raw, NoEncryption())
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(base64.b64encode(seed).decode() + "\n")
    return Ed25519Key(private).public_key_b64()


def load_key(path: Path) -> Ed25519Key:
    """The key ``keygen`` wrote."""
    seed = base64.b64decode(path.read_text().strip(), validate=True)
    return Ed25519Key(Ed25519PrivateKey.from_private_bytes(seed))


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="python -m brier_services.resolve", description=__doc__.split("\n\n")[0]
    )
    sub = parser.add_subparsers(dest="command", required=True)
    gen = sub.add_parser("keygen", help="create the resolver's signing key")
    gen.add_argument("--key-file", type=Path, required=True)
    run = sub.add_parser("run", help="propose outcomes for due markets")
    run.add_argument("--gateway", required=True)
    run.add_argument("--key-id", type=int, required=True)
    run.add_argument("--key-file", type=Path, required=True)
    run.add_argument("--evidence-dir", type=Path, default=DEFAULT_DIR)
    run.add_argument("--interval", type=float, default=300.0, help="seconds between passes")
    run.add_argument("--once", action="store_true", help="one pass, then exit")
    args = parser.parse_args(list(argv) if argv is not None else None)
    if args.command == "keygen":
        print(keygen(args.key_file))
        return 0
    key = load_key(args.key_file)
    client = RestClient(args.gateway)
    client.use_key(args.key_id, key.sign, clock_ms)
    gateway, store = RestGateway(client), EvidenceStore(args.evidence_dir)
    api_key = os.environ.get("FRED_API_KEY")
    verified: set[int] = set()
    while True:
        try:
            run_once(gateway, store, int(time.time()), api_key, verified=verified)
        except (ApiError, OSError) as e:
            log("pass_failed", error=f"{type(e).__name__}: {e}")
        if args.once:
            return 0
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
