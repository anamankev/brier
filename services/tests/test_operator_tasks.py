"""The operator can run a subset of its tasks (bots on one server, surveillance beside the
journal on the other); bad combinations stop it before it touches the exchange."""

from __future__ import annotations

from pathlib import Path

import pytest

from brier_services import exchange_operator


@pytest.mark.parametrize(
    "tasks",
    ["bogus", "list,bogus", "", "surveil"],  # surveillance needs --core and --data
)
def test_bad_task_choices_stop_at_once(tasks: str, tmp_path: Path) -> None:
    with pytest.raises(SystemExit):
        exchange_operator.main(["--state", str(tmp_path), "--tasks", tasks])


def test_without_saved_accounts_the_first_start_needs_the_gateway_binary(tmp_path: Path) -> None:
    with pytest.raises(SystemExit, match="first start needs --gateway-binary"):
        exchange_operator.accounts("http://127.0.0.1:1", None, tmp_path)
