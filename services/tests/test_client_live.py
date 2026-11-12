"""The reference client against a real gateway process (signing, WebSocket, book tracking).

Needs the Rust toolchain and PostgreSQL: ``make up``, then
``BRIER_TEST_DATABASE_URL=$(make db-url) uv run pytest -m integration``. Skipped otherwise.
"""

from __future__ import annotations

import asyncio
import json
import os
import shutil
import signal
import socket
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest

from brier_services.client import ApiError, Ed25519Key, RestClient, WsClient, clock_ms, totp_code

ROOT = Path(__file__).resolve().parents[2]
RULEBOOK = ROOT / "crates" / "gateway" / "tests" / "fixtures" / "long_window_rulebook.json"
PASSWORD = "a long enough password"  # noqa: S105 - a throwaway local test account


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port: int = s.getsockname()[1]
        return port


@pytest.fixture(scope="module")
def gateway(tmp_path_factory: pytest.TempPathFactory) -> Iterator[tuple[str, dict[str, str]]]:
    url = os.environ.get("BRIER_TEST_DATABASE_URL")
    if not url:
        pytest.skip("set BRIER_TEST_DATABASE_URL (make db-url) for live client tests")
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    if not Path(cargo).exists():
        pytest.skip("cargo not found: live client tests need the Rust toolchain")
    subprocess.run(  # noqa: S603 - fixed arguments
        [cargo, "build", "--release", "--locked", "-p", "brier-gateway"], cwd=ROOT, check=True
    )
    port = _free_port()
    env = os.environ | {
        "BRIER_DATABASE_URL": url,
        "BRIER_DATABASE_SCHEMA": f"pytest_client_{time.time_ns()}",
        "BRIER_DATA_DIR": str(tmp_path_factory.mktemp("journal")),
        "BRIER_LISTEN": f"127.0.0.1:{port}",
        "BRIER_TOTP_KEY": "42" * 32,
    }
    binary = str(ROOT / "target" / "release" / "brier-gateway")
    process = subprocess.Popen([binary, "serve"], env=env, stderr=subprocess.PIPE)  # noqa: S603
    base = f"http://127.0.0.1:{port}"
    deadline = time.monotonic() + 30
    while True:
        try:
            RestClient(base, timeout=1).request("GET", "/v1/health")
            break
        except OSError:
            if time.monotonic() > deadline or process.poll() is not None:
                process.kill()
                raise
            time.sleep(0.1)
    yield base, env
    process.send_signal(signal.SIGTERM)
    assert process.wait(timeout=30) == 0, "the gateway stops cleanly on SIGTERM"


async def _scenario(base: str, key: Ed25519Key, key_id: int, alice_token: str) -> None:
    ws_url = base.replace("http://", "ws://") + "/v1/ws"
    async with WsClient(ws_url) as bob, WsClient(ws_url) as alice:
        assert (await bob.request("auth", **key.ws_auth(key_id)))["user_id"] > 0
        await alice.request("auth", token=alice_token)
        book = await bob.subscribe_book(1)
        account = await bob.subscribe("account")
        for price in (55, 56, 57):
            placed = await bob.request(
                "place", market=1, side="sell", price=price, qty=10, client_order_id=price
            )
            assert placed["status"] == "resting"
        again = await bob.request(
            "place", market=1, side="sell", price=55, qty=10, client_order_id=55
        )
        assert again["status"] == "duplicate"
        taken = await alice.request("place", market=1, side="buy", price=56, qty=15, tif="ioc")
        assert taken["fills"] == [{"price": 55, "qty": 10}, {"price": 56, "qty": 5}]
        durable = RestClient(base).request("GET", "/v1/health")["durable_seq"]
        for _ in range(200):
            if book.journal_seq >= durable and len(account.messages) >= 8:
                break
            await asyncio.sleep(0.01)
        rest = RestClient(base).request("GET", "/v1/markets/1/book")
        assert book.synced
        assert sorted(book.asks.items()) == [tuple(level) for level in rest["asks"]]
        assert book.asks == {56: 5, 57: 10}
        kinds = [m.get("event", m["type"]) for m in account.messages]
        assert kinds.count("fill") == 2, kinds
        assert book.gaps == 0
        assert bob.refetches == 0


@pytest.mark.integration
def test_the_reference_client_trades_against_a_real_gateway(
    gateway: tuple[str, dict[str, str]],
) -> None:
    base, env = gateway
    rest = RestClient(base)
    for name in ("alice", "bob"):
        rest.request(
            "POST",
            "/v1/register",
            {"username": name, "password": PASSWORD, "adult": True, "terms": "2026-11-12"},
        )
    binary = str(ROOT / "target" / "release" / "brier-gateway")
    subprocess.run(  # noqa: S603 - fixed arguments
        [binary, "promote", "alice", "live client test administrator"], env=env, check=True
    )
    alice = RestClient(base)
    alice.login("alice", PASSWORD)
    secret, step = alice.enable_totp()
    alice.login("alice", PASSWORD, totp_code(secret, step + 1))
    rulebook = json.loads(RULEBOOK.read_text())
    alice.request("POST", "/v1/admin/markets", {"rulebook": rulebook, "reason": "live client test"})
    # The rulebook's open time has passed, so the gateway's scheduler opens the market within a
    # second; an explicit open that loses that race is refused as an invalid transition.
    try:
        alice.request("POST", "/v1/admin/markets/1/state", {"state": "open", "reason": "open it"})
    except ApiError as e:
        if e.code != "invalid_transition":
            raise

    bob = RestClient(base)
    bob.login("bob", PASSWORD)
    key = Ed25519Key()
    key_id = bob.request("POST", "/v1/keys", {"public_key": key.public_key_b64()})["key_id"]
    signed = RestClient(base)
    signed.use_key(key_id, key.sign, clock_ms)
    assert signed.request("GET", "/v1/portfolio")["cash"] > 0

    frozen = clock_ms()
    replayed = RestClient(base)
    replayed.use_key(key_id, key.sign, lambda: frozen)
    replayed.request("GET", "/v1/portfolio")
    with pytest.raises(ApiError) as refused:
        replayed.request("GET", "/v1/portfolio")  # same bytes, same signature: a replay
    assert (refused.value.status, refused.value.code) == (401, "unauthorized")

    assert alice.token is not None
    asyncio.run(_scenario(base, key, key_id, alice.token))
