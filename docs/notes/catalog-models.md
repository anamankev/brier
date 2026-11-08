# Pricing the recurring catalogue: are the market maker's models any good? (G9)

Status: COMPLETE. T1 and T2 supported; Q1 and Q2 not supported (§4). Registered before any model in this note was fitted or tested. The registering commit is the first that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

## 1. Why
The reference market maker quotes around a fair value. In the recurring catalogue (`docs/catalog.md`), a bad fair value would quote people into losing trades, or would make the maker the loser. G7 left this unmeasured for its models ("no historical backtest"). This note measures the new models on history they were not fitted on.

## 2. Hypotheses
**Temperature** (12 cities, 7-outcome events built as the lister builds them):
- **T1.** On the test period, the forecast model's mean multi-outcome Brier score is lower than 30-year climatology's in at least 10 of the 12 cities.
- **T2.** The model's probabilities are calibrated. Pooled over cities and outcomes on the test period, in every probability bin of width 0.1 holding at least 100 forecasts, the observed frequency is within 0.05 of the bin's mean forecast.

**Earthquakes** (daily worldwide magnitude 5+ counts, 4 outcomes: 2 or fewer, 3-4, 5-6, 7 or more):
- **Q1.** On the test period, a negative binomial model (fitted by the method of moments, so it allows the clustering of aftershocks) has a lower mean Brier score than a Poisson model with the same mean.
- **Q2.** The negative binomial model's mean Brier score is lower than a uniform 1/4 per outcome.

A hypothesis is supported if its inequality holds on the point estimates. Differences are reported with 95% intervals from 1,000 bootstrap resamples of test days (resampling seed 20261108), and a supported hypothesis whose interval crosses zero is said to be weak.

## 3. Design
### Data
- **Forecasts:** day-ahead hourly 2 m temperature forecasts at each official station's coordinates, from the Open-Meteo previous-runs API (`temperature_2m_previous_day1`, °F, local time). The daily forecast high is the maximum over the local calendar day.
  - Brier quotes from the NWS forecast, which has no public archive. Open-Meteo's archived forecasts stand in for it, so these results measure a comparable forecast, not the exact one.
- **Outcomes:** the official daily maximum from NOAA GHCN-Daily (`TMAX`) at each city's station, converted from tenths of °C to whole °F as the NOAA resolver does.
  - **Check:** for every day both exist, the GHCN maximum is compared with the NWS daily climate report the markets settle on, and the agreement rate is reported.
- **Climatology:** the same station's daily maxima for the 30 years 1996-2025, with every year's days within ±7 days of the calendar date.
- **Earthquakes:** daily counts of magnitude 5.0+ earthquakes worldwide (UTC days) from the USGS catalog.

### Periods
- **Temperature:** fit 1 November 2025 to 30 June 2026; test 1 July to 31 October 2026.
- **Earthquakes:** fit 1 November 2024 to 31 October 2025; test 1 November 2025 to 31 October 2026.

### Models
- **Temperature.**
  - Per city, the forecast error is the actual maximum minus the forecast maximum.
  - The model is Normal(forecast + mean error, sd of error), with the mean and sd fitted on the fit period.
  - The probability of an outcome is the normal mass over its range widened by half a degree at each end, because readings are whole degrees.
  - Each test day's event is centred on the rounded forecast, exactly as `catalog/templates.temperature` centres it.
- **Climatology:** the empirical frequency of each outcome among the ±7-day window's 30-year values, with one pseudo-count per outcome.
- **Earthquakes:**
  - The negative binomial has the fit period's mean and variance. If the variance does not exceed the mean, the model is the Poisson.
  - The Poisson has the same mean.

### Scores
- **Multi-outcome Brier score:** the sum over an event's outcomes of (probability − outcome)², averaged over test days.
- **Lower is better.**

## 4. Results
Run on 8 November 2026 with the code at commit `b329ff6`, the first commit with the runner:

    python -m brier_services.catalog.backtest

### Temperature
Fit on 242 days per city, and tested on about 123.

| City | Bias (°F) | Error sd (°F) | Test days | Model Brier | Climatology Brier | Difference (95% CI) |
|---|---|---|---|---|---|---|
| atlanta | +2.58 | 3.42 | 123 | 0.902 | 1.089 | -0.187 (-0.237 to -0.136) |
| austin | +2.47 | 3.16 | 123 | 0.759 | 0.983 | -0.224 (-0.271 to -0.174) |
| boston | +1.76 | 2.90 | 123 | 0.822 | 1.106 | -0.284 (-0.358 to -0.211) |
| chicago | +1.16 | 3.06 | 123 | 0.825 | 1.062 | -0.237 (-0.319 to -0.152) |
| dallas | +2.16 | 3.26 | 123 | 0.797 | 1.091 | -0.294 (-0.356 to -0.233) |
| denver | -0.06 | 3.50 | 123 | 0.789 | 1.116 | -0.328 (-0.379 to -0.277) |
| los-angeles | +1.37 | 2.90 | 123 | 0.788 | 1.094 | -0.306 (-0.381 to -0.231) |
| miami | +1.63 | 2.12 | 122 | 0.778 | 0.788 | -0.010 (-0.092 to +0.077) |
| nyc | +1.22 | 3.53 | 123 | 0.874 | 1.009 | -0.136 (-0.206 to -0.059) |
| phoenix | +0.28 | 2.56 | 123 | 0.748 | 1.076 | -0.328 (-0.387 to -0.266) |
| seattle | +1.60 | 2.93 | 123 | 0.816 | 1.022 | -0.206 (-0.273 to -0.143) |
| washington-dc | +0.13 | 3.52 | 123 | 0.818 | 1.075 | -0.257 (-0.321 to -0.191) |

- **T1: supported.** The model beat climatology in 12 of 12 cities.
  - Miami's interval crosses zero, so there the support is weak.
  - Miami's daily highs vary little, which leaves the forecast little room to beat climatology.
- **The forecast runs cold.** The archived forecasts underestimate the high by 1 to 2.6 °F in most cities. The fitted bias corrects for it.

**Calibration (T2),** pooled over cities and outcomes on the test period:

| Forecast bin | Forecasts | Mean forecast | Observed | Within 0.05 |
|---|---|---|---|---|
| 0.0-0.1 | 3,809 | 0.042 | 0.053 | yes |
| 0.1-0.2 | 2,829 | 0.151 | 0.138 | yes |
| 0.2-0.3 | 3,565 | 0.236 | 0.237 | yes |
| 0.3-0.4 | 122 | 0.362 | 0.328 | yes |

**T2: supported.** All 4 bins with at least 100 forecasts are within 0.05. No forecast in the test period exceeded 0.4, because every event spreads its chance over seven ranges.

**Settlement source check:** on 53 days where both exist, the NWS daily climate report's maximum equals GHCN-Daily's (53 of 53).

### Earthquakes
- **Fit year:** mean 5.70 magnitude 5+ earthquakes a day, variance 70.5.
- **Test year:** mean 5.56, variance 15.8.

| Model | Mean Brier (test) |
|---|---|
| Negative binomial (registered) | 0.919 |
| Poisson | 0.758 |
| Uniform | 0.750 |

- **Q1: not supported.** The negative binomial scored worse than the Poisson, by 0.118 to 0.206.
- **Q2: not supported.** It also scored worse than uniform.
- **The cause** is in the fit year. One great aftershock sequence (a variance of 71 against a mean under 6) pushed the method-of-moments fit to put half its probability on "2 or fewer". That outcome happened on far fewer days.
- **The Poisson** did no better than uniform.

## 5. Exploratory follow-ups (not registered)
These were chosen after §4 and scored on the same test year, so they test nothing.

    python -m brier_services.catalog.backtest --explore

| Model | Mean Brier (test) |
|---|---|
| Frequency of each outcome in the fit year | 0.739 |
| Negative binomial without the fit year's top 1% of days (above 28) | 0.753 |
| Poisson | 0.758 |
| Uniform | 0.750 |

- **Best model:** the plain outcome frequencies, which beat uniform by 0.003 to 0.018. That margin is real but small.
- **The honest conclusion:** a day's count of magnitude 5+ earthquakes is barely predictable from history. A market on it should trade near its base rates, and the maker should not pretend to know more.
- **Magnitude 6+:** at least one occurred on 28.2% of fit-year days and 30.4% of test-year days. The fit-year rate as a forecast scores 0.212.

## 6. What the market maker uses
- **Temperature: the model of §3.**
  - Its input is the Open-Meteo day-ahead forecast at the station, the same kind of forecast it was fitted on.
  - It uses each city's bias and error spread from §4.
  - Brier's lister centres the ranges on the NWS forecast, which only decides where the outcome boundaries fall.
- **Magnitude 5+ counts:** the outcome frequencies over the most recent 365 days (§5). The model was chosen after the results, so this is exploratory.
- **Magnitude 6+:** the share of the most recent 365 days with at least one.
- **Geomagnetic storms:** NOAA SWPC's own 3-day forecast probabilities, used as published.
  - No free archive of those forecasts was found, so this one is not backtested.
  - That is the weakest of the four models.
