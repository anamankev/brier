"""A short crash run in every test cycle; the registered 1,000-kill run is `make crash`."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest

from brier_services.crash import run

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(scope="session")
def core_binary() -> Path:
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    if not Path(cargo).exists():
        pytest.skip("cargo not found: crash tests need the Rust toolchain")
    subprocess.run(  # noqa: S603 - fixed arguments
        [cargo, "build", "--release", "--locked", "-p", "brier-core"], cwd=ROOT, check=True
    )
    return ROOT / "target" / "release" / "brier-core"


@pytest.mark.differential
def test_killed_core_never_loses_an_acknowledged_command(core_binary: Path, tmp_path: Path) -> None:
    summary = run(core_binary, tmp_path / "data", cycles=10, seed=7)
    assert summary.failures == []
    assert summary.cycles == 10
    assert summary.acknowledged > 0, "load reached the core"
    assert summary.commands_journaled >= summary.acknowledged
