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

/// The newest rulebook schema version this code reads. Schema 1 rulebooks stay valid, with
/// the same canonical form and hash.
pub const SCHEMA: u32 = 3;
/// The oldest schema version this code reads.
pub const MIN_SCHEMA: u32 = 1;
/// Longest provider game identifier accepted.
pub const MAX_GAME_ID_CHARS: usize = 32;
/// The longest event slug, series slug, and tag, in characters.
pub const MAX_SLUG_CHARS: usize = 80;
/// The most tags an event may have.
pub const MAX_TAGS: usize = 8;
/// The longest outcome label, in characters.
pub const MAX_OUTCOME_CHARS: usize = 40;
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
    /// The National Weather Service API (api.weather.gov): daily climate reports.
    Nws,
    /// The U.S. Geological Survey earthquake catalog (`ComCat`).
    Usgs,
    /// NOAA's Space Weather Prediction Center.
    Swpc,
    /// balldontlie: NBA and NFL schedules and final scores (schema 3).
    Balldontlie,
    /// football-data.org: soccer schedules and full-time scores (schema 3).
    #[serde(rename = "football_data")]
    FootballData,
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
    /// Between `threshold` and `upper`, both inclusive (schema 2).
    #[serde(rename = "between")]
    Between,
    /// Exactly equal (schema 3), such as a drawn match's margin of 0.
    #[serde(rename = "==")]
    Equal,
}

/// Who confirms a proposed resolution (schema 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Review {
    /// An admin other than the proposer, as in schema 1.
    Manual,
    /// The gateway, when an independent re-fetch computes the same outcome; for registry
    /// series marked machine-checkable only.
    Automatic,
}

/// The event a market belongs to (schema 2): markets sharing a slug are shown together, as
/// the outcomes of one question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// URL-safe identifier shared by the event's markets.
    pub slug: String,
    /// The event's question, such as "Highest temperature in New York on November 12?".
    pub title: String,
    /// Search and grouping tags: sorted, without repeats, each a slug.
    pub tags: Vec<String>,
    /// The recurring series the event belongs to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub series: Option<String>,
    /// Exactly one of the event's markets resolves YES.
    pub exclusive: bool,
    /// This market's label inside the event, such as "54-55°F".
    pub outcome: String,
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
    /// The one game a per-game series' market is about (schema 3): required for those series
    /// and absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<Game>,
}

/// A scheduled game (schema 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Game {
    /// The provider's game identifier: 1 to 32 ASCII letters, digits, `-` or `_`.
    pub id: String,
    /// The scheduled start, a UTC instant. Trading closes by then, the observation is its UTC
    /// date, and the result is released after it.
    pub starts: String,
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
    /// The inclusive upper end, for `between` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper: Option<String>,
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

/// A market's rulebook entry. In schema 1 every field below is required except `event` and
/// `review`, which must be absent; in schema 2 those two are required too. Either way a
/// rulebook has exactly one JSON form.
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
    /// The event (schema 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Event>,
    /// Who confirms the resolution (schema 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<Review>,
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
#[allow(clippy::struct_excessive_bools)] // independent yes/no facts about a source
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
    /// The resolver computes the outcome from the source without judgement, so a second fetch
    /// can confirm it and `review = "automatic"` is allowed.
    pub machine_checkable: bool,
    /// Each market is about one scheduled game named in `source.game` (schema 3).
    pub per_game: bool,
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

const FAHRENHEIT: Measure = Measure {
    transform: Transform::Level,
    precision: 0,
    unit: "°F",
};

const POINTS: Measure = Measure {
    transform: Transform::Level,
    precision: 0,
    unit: "points",
};
const GOALS: Measure = Measure {
    transform: Transform::Level,
    precision: 0,
    unit: "goals",
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
        machine_checkable: true,
        per_game: false,
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
        machine_checkable: true,
        per_game: false,
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
        machine_checkable: true,
        per_game: false,
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
        machine_checkable: true,
        per_game: false,
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
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/NYC",
        name: "Daily maximum temperature, New York Central Park (NWS daily climate report CLINYC)",
        url: "https://api.weather.gov/products/types/CLI/locations/NYC",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/ORD",
        name: "Daily maximum temperature, Chicago O'Hare (NWS daily climate report CLIORD)",
        url: "https://api.weather.gov/products/types/CLI/locations/ORD",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/LAX",
        name: "Daily maximum temperature, Los Angeles International Airport (NWS daily climate report CLILAX)",
        url: "https://api.weather.gov/products/types/CLI/locations/LAX",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/MIA",
        name: "Daily maximum temperature, Miami (NWS daily climate report CLIMIA)",
        url: "https://api.weather.gov/products/types/CLI/locations/MIA",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/BOS",
        name: "Daily maximum temperature, Boston (NWS daily climate report CLIBOS)",
        url: "https://api.weather.gov/products/types/CLI/locations/BOS",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/SEA",
        name: "Daily maximum temperature, Seattle-Tacoma Airport (NWS daily climate report CLISEA)",
        url: "https://api.weather.gov/products/types/CLI/locations/SEA",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/DEN",
        name: "Daily maximum temperature, Denver (NWS daily climate report CLIDEN)",
        url: "https://api.weather.gov/products/types/CLI/locations/DEN",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/ATL",
        name: "Daily maximum temperature, Atlanta (NWS daily climate report CLIATL)",
        url: "https://api.weather.gov/products/types/CLI/locations/ATL",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/PHX",
        name: "Daily maximum temperature, Phoenix (NWS daily climate report CLIPHX)",
        url: "https://api.weather.gov/products/types/CLI/locations/PHX",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/DCA",
        name: "Daily maximum temperature, Washington National (NWS daily climate report CLIDCA)",
        url: "https://api.weather.gov/products/types/CLI/locations/DCA",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/AUS",
        name: "Daily maximum temperature, Austin-Bergstrom (NWS daily climate report CLIAUS)",
        url: "https://api.weather.gov/products/types/CLI/locations/AUS",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Nws,
        id: "CLI/DFW",
        name: "Daily maximum temperature, Dallas/Fort Worth (NWS daily climate report CLIDFW)",
        url: "https://api.weather.gov/products/types/CLI/locations/DFW",
        kind: PeriodKind::Day,
        measures: &[FAHRENHEIT],
        category: "weather",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Usgs,
        id: "WORLD/M5",
        name: "Earthquakes of magnitude 5.0 or more worldwide, by UTC day (USGS ComCat)",
        url: "https://earthquake.usgs.gov/earthquakes/map/",
        kind: PeriodKind::Day,
        measures: &[Measure {
            transform: Transform::Level,
            precision: 0,
            unit: "earthquakes",
        }],
        category: "science",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Usgs,
        id: "WORLD/M6",
        name: "Earthquakes of magnitude 6.0 or more worldwide, by UTC day (USGS ComCat)",
        url: "https://earthquake.usgs.gov/earthquakes/map/",
        kind: PeriodKind::Day,
        measures: &[Measure {
            transform: Transform::Level,
            precision: 0,
            unit: "earthquakes",
        }],
        category: "science",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Swpc,
        id: "KP/MAX",
        name: "Daily maximum planetary K-index, by UTC day (NOAA SWPC estimated 3-hour values)",
        url: "https://www.swpc.noaa.gov/products/planetary-k-index",
        kind: PeriodKind::Day,
        measures: &[Measure {
            transform: Transform::Level,
            precision: 2,
            unit: "Kp",
        }],
        category: "science",
        live: true,
        released_after_period: true,
        machine_checkable: true,
        per_game: false,
    },
    Series {
        provider: Provider::Balldontlie,
        id: "NBA/MARGIN",
        name: "NBA game final margin: home points minus away points, including overtime",
        url: "https://www.balldontlie.io/",
        kind: PeriodKind::Day,
        measures: &[POINTS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::Balldontlie,
        id: "NFL/MARGIN",
        name: "NFL game final margin: home points minus away points, including overtime",
        url: "https://www.balldontlie.io/",
        kind: PeriodKind::Day,
        measures: &[POINTS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::FootballData,
        id: "PL/MARGIN",
        name: "Premier League match full-time margin: home goals minus away goals",
        url: "https://www.football-data.org/",
        kind: PeriodKind::Day,
        measures: &[GOALS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::FootballData,
        id: "PD/MARGIN",
        name: "La Liga match full-time margin: home goals minus away goals",
        url: "https://www.football-data.org/",
        kind: PeriodKind::Day,
        measures: &[GOALS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::FootballData,
        id: "SA/MARGIN",
        name: "Serie A match full-time margin: home goals minus away goals",
        url: "https://www.football-data.org/",
        kind: PeriodKind::Day,
        measures: &[GOALS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::FootballData,
        id: "BL1/MARGIN",
        name: "Bundesliga match full-time margin: home goals minus away goals",
        url: "https://www.football-data.org/",
        kind: PeriodKind::Day,
        measures: &[GOALS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
    Series {
        provider: Provider::FootballData,
        id: "FL1/MARGIN",
        name: "Ligue 1 match full-time margin: home goals minus away goals",
        url: "https://www.football-data.org/",
        kind: PeriodKind::Day,
        measures: &[GOALS],
        category: "sports",
        live: false,
        released_after_period: false,
        machine_checkable: false,
        per_game: true,
    },
];

/// Why a rulebook was refused. [`RuleError::code`] is stable and shared with the Python twin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleError {
    /// Not valid JSON for the schema: a missing, unknown, or mistyped field.
    Malformed(String),
    /// A schema version this code does not read.
    UnsupportedSchema,
    /// A schema 1 rulebook with schema 2 fields, or a schema 2 rulebook without them.
    SchemaFields,
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
    /// `between` without an `upper` above its threshold, or `upper` with another comparison.
    Range,
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
    /// The event's slug, title, tags, series, or outcome label is not well formed.
    Event,
    /// Automatic review for a series that is not machine-checkable.
    Review,
    /// `source.game` missing for a per-game series or present for another; a malformed game
    /// ID or start; an observation that is not the start's UTC date; trading that closes after
    /// the start, or a release before it.
    Game,
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
            Self::SchemaFields => "schema_fields",
            Self::MarketId => "market_id",
            Self::Title => "title",
            Self::Question => "question",
            Self::UnknownSeries => "unknown_series",
            Self::SourceUrl => "source_url",
            Self::Transform => "transform",
            Self::Threshold => "threshold",
            Self::Range => "range",
            Self::Observation => "observation",
            Self::Time => "time",
            Self::ScheduleOrder => "schedule_order",
            Self::CloseAfterObservable => "close_after_observable",
            Self::ReleaseBeforePeriodEnd => "release_before_period_end",
            Self::DeadlineTooFar => "deadline_too_far",
            Self::VoidConditions => "void_conditions",
            Self::Event => "event",
            Self::Review => "review",
            Self::Game => "game",
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

/// The game rules (schema 3): a per-game series names exactly one well-formed game, observed
/// on its UTC start date, traded only before it starts and released after.
fn game_ok(series: Series, game: Option<&Game>, observation: &str, times: &Times) -> bool {
    match (series.per_game, game) {
        (false, None) => true,
        (true, Some(g)) => {
            let id_ok = (1..=MAX_GAME_ID_CHARS).contains(&g.id.len())
                && g.id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
            let Some(starts) = time::instant(&g.starts) else {
                return false;
            };
            let date = time::format_instant(starts.div_euclid(DAY).saturating_mul(DAY));
            id_ok
                && date.is_some_and(|d| d.get(..10) == Some(observation))
                && times.close <= starts
                && times.release > starts
        }
        _ => false,
    }
}

/// A slug: lowercase ASCII letters, digits, and single hyphens inside, at most
/// [`MAX_SLUG_CHARS`].
fn slug_ok(s: &str) -> bool {
    (1..=MAX_SLUG_CHARS).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.contains("--")
}

fn event_ok(e: &Event) -> bool {
    slug_ok(&e.slug)
        && text_ok(&e.title, MAX_TITLE_CHARS)
        && text_ok(&e.outcome, MAX_OUTCOME_CHARS)
        && e.tags.len() <= MAX_TAGS
        && e.tags.iter().all(|t| slug_ok(t))
        && e.tags.windows(2).all(|w| matches!(w, [a, b] if a < b))
        && e.series.as_deref().is_none_or(slug_ok)
}

/// Whether canonical decimal `a` is less than canonical decimal `b` (both pass
/// [`threshold_ok`]), compared exactly as digit strings.
fn decimal_lt(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> (bool, String, String) {
        let (neg, body) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        (neg, int.to_owned(), frac.to_owned())
    }
    let ((na, ia, fa), (nb, ib, fb)) = (parts(a), parts(b));
    let width = fa.len().max(fb.len());
    let key = |int: &str, frac: &str| (int.len(), format!("{int}{frac:0<width$}"));
    let (ka, kb) = (key(&ia, &fa), key(&ib, &fb));
    match (na, nb) {
        (false, false) => ka < kb,
        (true, true) => kb < ka,
        (true, false) => true,
        (false, true) => false,
    }
}

/// A decimal string with at most `precision` decimal places, as an integer count of
/// `10^-precision` steps. `None` if it does not parse or needs more places.
#[must_use]
pub fn scaled(s: &str, precision: usize) -> Option<i128> {
    let (neg, body) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let (int, frac) = body.split_once('.').unwrap_or((body, ""));
    if int.is_empty()
        || frac.len() > precision
        || !body.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return None;
    }
    let digits = format!("{int}{frac:0<precision$}");
    let n: i128 = digits.parse().ok()?;
    Some(if neg { n.checked_neg()? } else { n })
}

impl Validated {
    /// The YES set as an inclusive range of resolved values, in steps of the series'
    /// precision (the resolver rounds to that precision first). `None` is unbounded.
    #[must_use]
    pub fn yes_range(&self) -> (Option<i128>, Option<i128>) {
        let c = &self.rulebook.condition;
        let p = self.measure.precision;
        let t = scaled(&c.threshold, p);
        match c.op {
            Op::Above => (t.and_then(|t| t.checked_add(1)), None),
            Op::AtLeast => (t, None),
            Op::Below => (None, t.and_then(|t| t.checked_sub(1))),
            Op::AtMost => (None, t),
            Op::Between => (t, c.upper.as_deref().and_then(|u| scaled(u, p))),
            Op::Equal => (t, t),
        }
    }
}

/// Whether two inclusive ranges (`None` unbounded) share a value.
#[must_use]
pub fn ranges_overlap(a: (Option<i128>, Option<i128>), b: (Option<i128>, Option<i128>)) -> bool {
    let lo = |x: Option<i128>| x.unwrap_or(i128::MIN);
    let hi = |x: Option<i128>| x.unwrap_or(i128::MAX);
    lo(a.0).max(lo(b.0)) <= hi(a.1).min(hi(b.1))
}

/// Whether the ranges cover every value exactly once: unbounded at both ends and contiguous.
#[must_use]
pub fn ranges_partition(ranges: &[(Option<i128>, Option<i128>)]) -> bool {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.0.unwrap_or(i128::MIN));
    let (Some(first), Some(last)) = (sorted.first(), sorted.last()) else {
        return false;
    };
    first.0.is_none()
        && last.1.is_none()
        && sorted
            .windows(2)
            .all(|w| matches!(w, [a, b] if a.1.and_then(|h| h.checked_add(1)).is_some_and(|n| Some(n) == b.0)))
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
        if !(MIN_SCHEMA..=SCHEMA).contains(&self.schema) {
            return Err(RuleError::UnsupportedSchema);
        }
        let v2_fields = [self.event.is_some(), self.review.is_some()];
        let fields_ok = if self.schema == 1 {
            v2_fields == [false, false] && self.condition.op != Op::Between
        } else {
            v2_fields == [true, true]
        } && (self.schema >= 3
            || (self.source.game.is_none() && self.condition.op != Op::Equal));
        if !fields_ok {
            return Err(RuleError::SchemaFields);
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
        match (&self.condition.op, &self.condition.upper) {
            (Op::Between, Some(upper))
                if threshold_ok(upper, measure.precision)
                    && decimal_lt(&self.condition.threshold, upper) => {}
            (Op::Between, _) | (_, Some(_)) => return Err(RuleError::Range),
            _ => {}
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
        if let Some(event) = &self.event
            && !event_ok(event)
        {
            return Err(RuleError::Event);
        }
        if self.review == Some(Review::Automatic) && !series.machine_checkable {
            return Err(RuleError::Review);
        }
        if !game_ok(series, self.source.game.as_ref(), &self.observation, &times) {
            return Err(RuleError::Game);
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
            Op::Between => "between",
            Op::Equal => "exactly",
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
            r.observation,
            r.condition.upper.as_ref().map_or_else(
                || r.condition.threshold.clone(),
                |u| format!("{} and {u} (inclusive),", r.condition.threshold)
            ),
            self.measure.unit,
            r.schedule.deadline
        )
    }
}
