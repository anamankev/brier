//! Market rulebooks (SPEC §7, G5 plan): the schema, the validator, and the rules hash.
//!
//! A rulebook says exactly how a market resolves. It is written as a TOML file under
//! `markets/`, sent to the gateway as JSON, and validated here. Its rules hash is the SHA-256
//! of its canonical JSON (RFC 8785, [`canonical`]); the hash goes into the market's
//! `CreateMarket` command, so the journal pins the rules a market was listed under.
//!
//! The YES condition is structured (`value op threshold`), never prose, so resolvers decide it
//! mechanically; the prose shown to people is generated from it ([`Rulebook::describe`]).
//! Every rule is checked here and, identically, by the Python twin
//! (`brier_services.rules`); shared vectors in `tests/vectors.json` hold the two together.

pub mod canonical;
pub mod time;

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::time::{DAY, PeriodKind, Unix};

/// The rulebook schema version this code reads.
pub const SCHEMA: u32 = 1;
/// The longest title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;
/// The longest question, in characters.
pub const MAX_QUESTION_CHARS: usize = 500;
/// The longest a resolution may wait after the release time: three months covers a late or
/// delayed publication without leaving a market unresolved indefinitely.
pub const MAX_DEADLINE_AFTER_RELEASE: Unix = 92 * DAY;
/// The most digits before the decimal point in a threshold.
pub const MAX_INTEGER_DIGITS: usize = 6;

/// A data provider (`docs/data-licenses.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// The Bureau of Labor Statistics public API.
    Bls,
    /// The Federal Reserve Bank of St. Louis (FRED and ALFRED).
    Fred,
    /// NOAA's National Centers for Environmental Information.
    Noaa,
}

/// How the resolved value is derived from the series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    /// The observation as published.
    Level,
    /// The 12-month percent change, (`v_t` / `v_{t−12}` − 1) × 100, rounded half-up to the
    /// series' precision, as BLS rounds its published changes.
    #[serde(rename = "pct_change_12m")]
    PctChange12m,
}

/// The comparison in the YES condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    /// Strictly above.
    #[serde(rename = ">")]
    Above,
    /// At or above.
    #[serde(rename = ">=")]
    AtLeast,
    /// Strictly below.
    #[serde(rename = "<")]
    Below,
    /// At or below.
    #[serde(rename = "<=")]
    AtMost,
}

/// The revision policy. Only one exists: the first published value counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Revision {
    /// The value in the first successful fetch after the release time; later revisions are
    /// ignored.
    FirstRelease,
}

/// A condition under which the market is voided (SPEC §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoidCondition {
    /// The source publishes no value for the period by the resolution deadline. Required.
    NoValueByDeadline,
    /// The source discontinues or redefines the series before publishing the value.
    SourceDiscontinued,
}

/// Where the value comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// The provider.
    pub provider: Provider,
    /// The provider's series identifier (for NOAA, `station/element`).
    pub series: String,
    /// The public page for the series; must equal the registry's.
    pub url: String,
}

/// The YES condition: `value op threshold`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// How the value is derived.
    pub transform: Transform,
    /// The comparison.
    pub op: Op,
    /// A decimal string, with at most the series' precision in decimal places.
    pub threshold: String,
}

/// The market's times, as UTC instants (`YYYY-MM-DDTHH:MM:SSZ`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    /// Trading opens.
    pub open: String,
    /// Trading closes. Strictly before `release`.
    pub close: String,
    /// The source's scheduled publication time.
    pub release: String,
    /// The market is voided if no value has been published by then.
    pub deadline: String,
}

/// A market's rulebook entry (schema version 1). Every field is required, so a rulebook has
/// exactly one JSON form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rulebook {
    /// Schema version; must be [`SCHEMA`].
    pub schema: u32,
    /// The market ID this rulebook lists.
    pub market: u32,
    /// A short title.
    pub title: String,
    /// The question, as a trader reads it.
    pub question: String,
    /// The source.
    pub source: Source,
    /// The period observed: `YYYY-MM` or `YYYY-MM-DD`, per the series.
    pub observation: String,
    /// The YES condition.
    pub condition: Condition,
    /// The times.
    pub schedule: Schedule,
    /// The revision policy.
    pub revision: Revision,
    /// Void conditions, sorted, without repeats, including `no_value_by_deadline`.
    pub void: Vec<VoidCondition>,
}

/// One transform a series allows, with the precision and unit of the resulting value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measure {
    /// The transform.
    pub transform: Transform,
    /// Decimal places of the resolved value.
    pub precision: usize,
    /// The unit, as shown to people.
    pub unit: &'static str,
}

/// A series that markets may resolve on. Only registry series are accepted: each one has had
/// its terms read (`docs/data-licenses.md`) and has a resolver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Series {
    /// The provider.
    pub provider: Provider,
    /// The series identifier.
    pub id: &'static str,
    /// The series' name.
    pub name: &'static str,
    /// The public page.
    pub url: &'static str,
    /// The period kind.
    pub kind: PeriodKind,
    /// The transforms allowed.
    pub measures: &'static [Measure],
    /// The market category.
    pub category: &'static str,
    /// The value can be watched while the period runs (market yields, temperatures), so
    /// trading must close before the period starts.
    pub live: bool,
    /// The source publishes only after the period ends, so a release time inside the period
    /// is a mistake. False for the FOMC target, announced before its effective date.
    pub released_after_period: bool,
}

const PERCENT_1DP: Measure = Measure {
    transform: Transform::Level,
    precision: 1,
    unit: "percent",
};
const PERCENT_2DP: Measure = Measure {
    transform: Transform::Level,
    precision: 2,
    unit: "percent",
};

/// Every series markets may resolve on.
pub const REGISTRY: &[Series] = &[
    Series {
        provider: Provider::Bls,
        id: "CUUR0000SA0",
        name: "CPI-U, U.S. city average, all items, not seasonally adjusted",
        url: "https://data.bls.gov/timeseries/CUUR0000SA0",
        kind: PeriodKind::Month,
        measures: &[
            Measure {
                transform: Transform::PctChange12m,
                precision: 1,
                unit: "percent",
            },
            Measure {
                transform: Transform::Level,
                precision: 3,
                unit: "index points (1982-84 = 100)",
            },
        ],
        category: "economics",
        live: false,
        released_after_period: true,
    },
    Series {
        provider: Provider::Bls,
        id: "LNS14000000",
        name: "Unemployment rate, seasonally adjusted",
        url: "https://data.bls.gov/timeseries/LNS14000000",
        kind: PeriodKind::Month,
        measures: &[PERCENT_1DP],
        category: "economics",
        live: false,
        released_after_period: true,
    },
    Series {
        provider: Provider::Fred,
        id: "DFEDTARU",
        name: "Federal funds target range, upper limit",
        url: "https://fred.stlouisfed.org/series/DFEDTARU",
        kind: PeriodKind::Day,
        measures: &[PERCENT_2DP],
        category: "rates",
        live: false,
        released_after_period: false,
    },
    Series {
        provider: Provider::Fred,
        id: "DGS10",
        name: "10-year Treasury constant maturity yield",
        url: "https://fred.stlouisfed.org/series/DGS10",
        kind: PeriodKind::Day,
        measures: &[PERCENT_2DP],
        category: "rates",
        live: true,
        released_after_period: true,
    },
    Series {
        provider: Provider::Noaa,
        id: "USW00094728/TMAX",
        name: "Daily maximum temperature, New York Central Park (GHCN-Daily USW00094728)",
        url: "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries&stations=USW00094728&dataTypes=TMAX",
        kind: PeriodKind::Day,
        measures: &[Measure {
            transform: Transform::Level,
            precision: 0,
            unit: "°F",
        }],
        category: "weather",
        live: true,
        released_after_period: true,
    },
];

/// Why a rulebook was refused. [`RuleError::code`] is stable and shared with the Python twin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// Not valid JSON for the schema: a missing, unknown, or mistyped field.
    Malformed(String),
    /// A schema version this code does not read.
    UnsupportedSchema,
    /// Market ID zero.
    MarketId,
    /// Title empty, too long, padded with spaces, or containing control characters.
    Title,
    /// Question empty, too long, padded with spaces, or containing control characters.
    Question,
    /// The provider and series are not in the registry.
    UnknownSeries,
    /// The URL is not the registry's page for the series.
    SourceUrl,
    /// The transform is not allowed for the series.
    Transform,
    /// The threshold is not a canonical decimal within the series' precision.
    Threshold,
    /// The observation is not a valid period of the series' kind.
    Observation,
    /// A time is not of the form `YYYY-MM-DDTHH:MM:SSZ` within 2000-2100.
    Time,
    /// The times are not in the order open < close < release < deadline.
    ScheduleOrder,
    /// A live series' market closes after its period starts.
    CloseAfterObservable,
    /// The release time is before the period ends, for a series published after its period.
    ReleaseBeforePeriodEnd,
    /// The deadline is more than [`MAX_DEADLINE_AFTER_RELEASE`] after the release.
    DeadlineTooFar,
    /// The void conditions are unsorted, repeated, or lack `no_value_by_deadline`.
    VoidConditions,
    /// The rulebook has no canonical JSON form (cannot happen for a parsed rulebook).
    Canonical,
}

impl RuleError {
    /// The stable code, as the gateway reports it and the shared vectors name it.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Malformed(_) => "malformed",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::MarketId => "market_id",
            Self::Title => "title",
            Self::Question => "question",
            Self::UnknownSeries => "unknown_series",
            Self::SourceUrl => "source_url",
            Self::Transform => "transform",
            Self::Threshold => "threshold",
            Self::Observation => "observation",
            Self::Time => "time",
            Self::ScheduleOrder => "schedule_order",
            Self::CloseAfterObservable => "close_after_observable",
            Self::ReleaseBeforePeriodEnd => "release_before_period_end",
            Self::DeadlineTooFar => "deadline_too_far",
            Self::VoidConditions => "void_conditions",
            Self::Canonical => "canonical",
        }
    }
}

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(why) => write!(f, "rulebook is malformed: {why}"),
            other => write!(f, "rulebook refused: {}", other.code()),
        }
    }
}

impl std::error::Error for RuleError {}

/// The times of a validated rulebook, as Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Times {
    /// Trading opens.
    pub open: Unix,
    /// Trading closes.
    pub close: Unix,
    /// Scheduled publication.
    pub release: Unix,
    /// Resolution deadline.
    pub deadline: Unix,
}

/// A rulebook that passed every check, with its canonical form and hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Validated {
    /// The rulebook.
    pub rulebook: Rulebook,
    /// Its registry series.
    pub series: Series,
    /// The measure its transform selects.
    pub measure: Measure,
    /// Its times.
    pub times: Times,
    /// Its canonical JSON.
    pub canonical: String,
    /// SHA-256 of the canonical JSON: the rules hash.
    pub hash: [u8; 32],
}

impl Validated {
    /// The rules hash as lowercase hex.
    #[must_use]
    pub fn hash_hex(&self) -> String {
        hex(&self.hash)
    }
}

/// Lowercase hex of bytes.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn text_ok(s: &str, max_chars: usize) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= max_chars
        && !s.chars().any(char::is_control)
}

/// A canonical decimal: optional `-`, no leading zeros, at most `precision` decimal places,
/// no trailing `.`, and not `-0`.
fn threshold_ok(s: &str, precision: usize) -> bool {
    let body = s.strip_prefix('-').unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let int_ok =
        digits(int) && int.len() <= MAX_INTEGER_DIGITS && (int == "0" || !int.starts_with('0'));
    let frac_ok = frac.is_none_or(|f| digits(f) && f.len() <= precision);
    let zero = int.bytes().all(|b| b == b'0') && frac.is_none_or(|f| f.bytes().all(|b| b == b'0'));
    int_ok && frac_ok && !(s.starts_with('-') && zero)
}

/// Find a series in the registry.
#[must_use]
pub fn series(provider: Provider, id: &str) -> Option<Series> {
    REGISTRY
        .iter()
        .find(|s| s.provider == provider && s.id == id)
        .copied()
}

/// Parse and check a rulebook given as JSON.
///
/// # Errors
/// The first rule the rulebook breaks, in the order the checks are listed in [`RuleError`].
pub fn validate(value: &Value) -> Result<Validated, RuleError> {
    let rulebook: Rulebook =
        serde_json::from_value(value.clone()).map_err(|e| RuleError::Malformed(e.to_string()))?;
    rulebook.check()
}

impl Rulebook {
    /// Check every rule, and compute the canonical form and hash.
    ///
    /// # Errors
    /// The first rule broken, in the order the checks are listed in [`RuleError`].
    pub fn check(self) -> Result<Validated, RuleError> {
        if self.schema != SCHEMA {
            return Err(RuleError::UnsupportedSchema);
        }
        if self.market == 0 {
            return Err(RuleError::MarketId);
        }
        if !text_ok(&self.title, MAX_TITLE_CHARS) {
            return Err(RuleError::Title);
        }
        if !text_ok(&self.question, MAX_QUESTION_CHARS) {
            return Err(RuleError::Question);
        }
        let series =
            series(self.source.provider, &self.source.series).ok_or(RuleError::UnknownSeries)?;
        if self.source.url != series.url {
            return Err(RuleError::SourceUrl);
        }
        let measure = series
            .measures
            .iter()
            .find(|m| m.transform == self.condition.transform)
            .copied()
            .ok_or(RuleError::Transform)?;
        if !threshold_ok(&self.condition.threshold, measure.precision) {
            return Err(RuleError::Threshold);
        }
        let (period_start, period_end) =
            time::period(series.kind, &self.observation).ok_or(RuleError::Observation)?;
        let t = |s: &str| time::instant(s).ok_or(RuleError::Time);
        let times = Times {
            open: t(&self.schedule.open)?,
            close: t(&self.schedule.close)?,
            release: t(&self.schedule.release)?,
            deadline: t(&self.schedule.deadline)?,
        };
        if !(times.open < times.close
            && times.close < times.release
            && times.release < times.deadline)
        {
            return Err(RuleError::ScheduleOrder);
        }
        if series.live && times.close > period_start {
            return Err(RuleError::CloseAfterObservable);
        }
        if series.released_after_period && times.release < period_end {
            return Err(RuleError::ReleaseBeforePeriodEnd);
        }
        if times
            .deadline
            .checked_sub(times.release)
            .is_none_or(|gap| gap > MAX_DEADLINE_AFTER_RELEASE)
        {
            return Err(RuleError::DeadlineTooFar);
        }
        let sorted_unique = self.void.windows(2).all(|w| matches!(w, [a, b] if a < b));
        if !sorted_unique || !self.void.contains(&VoidCondition::NoValueByDeadline) {
            return Err(RuleError::VoidConditions);
        }
        let value = serde_json::to_value(&self).map_err(|_| RuleError::Canonical)?;
        let canonical = canonical::canonical(&value).map_err(|_| RuleError::Canonical)?;
        let hash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        Ok(Validated {
            rulebook: self,
            series,
            measure,
            times,
            canonical,
            hash,
        })
    }
}

impl Validated {
    /// The YES condition in words, generated from the structured fields so the prose can
    /// never disagree with what the resolver checks.
    #[must_use]
    pub fn describe(&self) -> String {
        let r = &self.rulebook;
        let what = match r.condition.transform {
            Transform::Level => format!("the value of {}", self.series.name),
            Transform::PctChange12m => format!(
                "the 12-month percent change in {}, rounded to {} decimal place{}",
                self.series.name,
                self.measure.precision,
                if self.measure.precision == 1 { "" } else { "s" }
            ),
        };
        let op = match r.condition.op {
            Op::Above => "above",
            Op::AtLeast => "at or above",
            Op::Below => "below",
            Op::AtMost => "at or below",
        };
        let period = match self.series.kind {
            PeriodKind::Month => "the month",
            PeriodKind::Day => "the day",
        };
        let discontinued = if r.void.contains(&VoidCondition::SourceDiscontinued) {
            ", or if the source discontinues or redefines the series before publishing it"
        } else {
            ""
        };
        format!(
            "Resolves YES if {what}, for {period} {}, as first published by the source, is {op} {} {}. \
             Resolves NO if it is published and is not. Voided if no value is published by {}{discontinued}.",
            r.observation, r.condition.threshold, self.measure.unit, r.schedule.deadline
        )
    }
}
