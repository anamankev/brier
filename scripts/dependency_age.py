#!/usr/bin/env python3
"""Supply-chain rule: every locked dependency was public for at least 14 days when chosen.

A fresh release is where a hijacked maintainer account or a typo-squat does its damage, and
most are caught and yanked within days. This script reads every registry package in
`Cargo.lock`, `fuzz/Cargo.lock`, `services/uv.lock`, and `web/package-lock.json`, asks crates.io,
PyPI, and npm when that exact version was published, and fails if any is younger than 14 days. CI runs it whenever a
lockfile changes (`.github/workflows/dependency-age.yml`); `make dependency-age` runs it here.

Standard library only; HTTP goes through `curl` so it uses the system's certificate store.
"""

from __future__ import annotations

import datetime as dt
import json
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MIN_AGE = dt.timedelta(days=14)
USER_AGENT = "brier-dependency-age (github.com/anamankev/brier)"


def fetch(url: str) -> dict[str, object]:
    out = subprocess.run(  # noqa: S603 - fixed program, URL built from lockfile names
        ["curl", "-sfL", "--retry", "3", "-A", USER_AGENT, url],  # noqa: S607
        check=True,
        capture_output=True,
    ).stdout
    result: dict[str, object] = json.loads(out)
    return result


def cargo_packages(lock: Path) -> set[tuple[str, str]]:
    text = lock.read_text()
    pattern = r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = "registry'
    return set(re.findall(pattern, text))


def uv_packages(lock: Path) -> set[tuple[str, str]]:
    text = lock.read_text()
    pattern = r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\nsource = \{ registry'
    return set(re.findall(pattern, text))


def npm_packages(lock: Path) -> set[tuple[str, str]]:
    packages = json.loads(lock.read_text())["packages"]
    out = set()
    for path, meta in packages.items():
        if path and "resolved" in meta and str(meta["resolved"]).startswith("https://registry.npmjs.org/"):
            out.add((path.rsplit("node_modules/", 1)[-1], str(meta["version"])))
    return out


_npm_times: dict[str, dict[str, str]] = {}


def npm_published(name: str, version: str) -> dt.datetime:
    if name not in _npm_times:
        data = fetch(f"https://registry.npmjs.org/{name.replace('/', '%2F')}")
        _npm_times[name] = data["time"]  # type: ignore[assignment]
    return dt.datetime.fromisoformat(_npm_times[name][version].replace("Z", "+00:00"))


def crate_published(name: str, version: str) -> dt.datetime:
    data = fetch(f"https://crates.io/api/v1/crates/{name}/{version}")
    created = data["version"]["created_at"]  # type: ignore[index]
    time.sleep(1)  # crates.io asks for at most one request per second
    return dt.datetime.fromisoformat(str(created))


def pypi_published(name: str, version: str) -> dt.datetime:
    data = fetch(f"https://pypi.org/pypi/{name}/{version}/json")
    uploads = [f["upload_time_iso_8601"] for f in data["urls"]]  # type: ignore[attr-defined]
    return dt.datetime.fromisoformat(min(uploads))


def main() -> int:
    now = dt.datetime.now(dt.UTC)
    checks = [
        ("crates.io", crate_published, cargo_packages(ROOT / "Cargo.lock")
         | cargo_packages(ROOT / "fuzz" / "Cargo.lock")),
        ("PyPI", pypi_published, uv_packages(ROOT / "services" / "uv.lock")),
        ("npm", npm_published, npm_packages(ROOT / "web" / "package-lock.json")),
    ]
    young = []
    for registry, published, packages in checks:
        for name, version in sorted(packages):
            age = now - published(name, version)
            if age < MIN_AGE:
                young.append(f"{registry} {name} {version}: {age.days} days old")
        print(f"{registry}: {len(packages)} packages checked")
    for line in young:
        print("too new:", line)
    return 1 if young else 0


if __name__ == "__main__":
    sys.exit(main())
