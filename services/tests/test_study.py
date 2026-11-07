"""The resolver study's own parsing (docs/notes/resolution.md), offline."""

from __future__ import annotations

import re
from fractions import Fraction

import pytest

from brier_services.resolution import study


@pytest.mark.parametrize(
    ("sentence", "upper"),
    [
        (
            "the Committee decided to maintain the target range for the federal funds rate at "
            "2-1/4 to 2-1/2 percent.",
            Fraction(5, 2),
        ),
        (
            "the Federal Open Market Committee decided today to lower the target range for the "
            "federal funds rate by 1/2 percentage point, to 1 to 1\u20111/4 percent.",
            Fraction(5, 4),
        ),
        (
            "the Committee decided to lower the target range for the federal funds rate by 1/4 "
            "percentage point to 4 to 4\u20111/4 percent.",
            Fraction(17, 4),
        ),
        (
            "the Committee decided to raise the target range for the federal funds rate to "
            "1\u20111/2 to 1-3/4 percent and anticipates",
            Fraction(7, 4),
        ),
        (
            "the Committee decided to keep the target range for the federal funds rate at 0 to "
            "1/4 percent.",
            Fraction(1, 4),
        ),
    ],
)
def test_fomc_decisions_are_read_in_every_phrasing(sentence: str, upper: Fraction) -> None:
    text = re.sub(r"\s+", " ", sentence.translate(study._HYPHENS))
    m = study._DECISION.search(text)
    assert m is not None, sentence
    assert study._mixed(m.group(2)) == upper


def test_operations_statements_are_not_decisions() -> None:
    operations = (
        "to undertake open market operations as necessary to maintain the federal funds rate in "
        "a target range of 1-3/4 to 2 percent."
    )
    dissent = (
        "who preferred to lower the target range for the federal funds rate to 2 to 2-1/4 percent"
    )
    assert study._DECISION.search(operations) is None
    assert study._DECISION.search(dissent) is None


def test_every_study_instance_has_a_valid_rulebook() -> None:
    for kind in study.KINDS:
        for period in kind.periods[:3] + kind.periods[-3:]:
            rule = study.instance_rule(kind.provider, kind.series, period, kind.transform, "0")
            assert rule.observation == period
    assert study.MONTHS[0] == "2021-10"
    assert study.MONTHS[-1] == "2026-09"
    assert len(study.MONTHS) == 60
    assert len(study.DAYS_2025) == 365
