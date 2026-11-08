# Pricing the recurring catalogue: are the market maker's models any good? (G9)

Status: REGISTERED. Written before any model in this note was fitted or tested. The registering commit is the first that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

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
Not yet run.
