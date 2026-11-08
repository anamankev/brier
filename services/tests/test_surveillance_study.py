"""The evaluation's counting, threshold choice, and intervals, on made-up scores."""

from __future__ import annotations

import pytest

from brier_services.surveillance import study
from brier_services.surveillance.study import Data, Row


def data() -> Data:
    rows = [
        Row(1, "wash", 40, 0.9, "wash"),
        Row(1, "wash", 30, 0.5, "linked_noise"),
        Row(2, "wash", 40, 0.5, "wash"),
        Row(2, "wash", 31, 0.2, "linked_noise"),
    ]
    return Data(rows, {(1, "wash", 40), (2, "wash", 40), (2, "wash", 41)}, {})


def test_counts_per_seed() -> None:
    assert study.per_seed(data(), "wash", 0.5, range(1, 3)) == {1: (1, 1, 0), 2: (1, 0, 1)}
    assert study.total([(1, 1, 0), (1, 0, 1)]) == (2, 1, 1)


def test_rates_are_undefined_without_a_denominator() -> None:
    assert study.rates(0, 0, 0) == {"precision": None, "recall": None, "f1": None}
    assert study.rates(2, 1, 1) == {
        "precision": pytest.approx(2 / 3),
        "recall": pytest.approx(2 / 3),
        "f1": pytest.approx(2 / 3),
    }


def test_the_threshold_maximizes_f1_and_ties_go_higher(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr(study, "TUNING", range(1, 3))
    # At 0.5: tp 2, fp 1, fn 1 -> F1 2/3. At 0.9: tp 1, fp 0, fn 2 -> 1/2. At 0.2: F1 4/7.
    assert study.tune(data(), "wash") == 0.5
    tie = Data([Row(1, "close", 1, 1.0, "x"), Row(1, "close", 1, 2.0, "x")], {(1, "close", 1)}, {})
    assert study.tune(tie, "close") == 2.0


def test_percentile_interval() -> None:
    assert study.interval([float(v) for v in range(1001)]) == [25.0, 975.0]
    assert study.interval([]) is None
