# Pricing daily lows: does the forecast model beat climatology? (G10 U5)

Status: REGISTERED. Registered before any model in this note was fitted or tested; the registering commit is the first that contains this file. Nothing below changes after results are seen.

## 1. Why
Daily-low markets ("Lowest temperature in Denver on Nov 14?") add 24 events a day across the 12 cities that already have daily highs. The reference market maker needs a fair value. The daily-high model passed both of its tests (`docs/notes/catalog-models.md`, T1 and T2); this note tests the same model on lows, which behave differently: they come overnight, near dawn, and depend on cloud cover and wind more than on sun.

## 2. Hypotheses
- **L1.** On the test period, the forecast model's mean multi-outcome Brier score for the daily low is lower than 30-year climatology's in at least 10 of the 12 cities.
- **L2.** The model's probabilities are calibrated. Pooled over cities and outcomes on the test period, in every probability bin of width 0.1 holding at least 100 forecasts, the observed frequency is within 0.05 of the bin's mean forecast.

A hypothesis is supported if its inequality holds on the point estimates. Differences are reported with 95% intervals from 1,000 bootstrap resamples of test days (resampling seed 20261112).

## 3. Design
Everything is as in `catalog-models.md` §3 for highs, with the minimum in place of the maximum:
- **Forecasts:** day-ahead hourly 2 m temperature forecasts from the Open-Meteo previous-runs API at each official station. The forecast low is the minimum over the local calendar day.
- **Outcomes:** GHCN-Daily `TMIN` at each station, converted from tenths of °C to whole °F as the NOAA resolver does.
- **Climatology:** the same station's daily minima for 1996-2025, ±7 days of the calendar date, with one pseudo-count per outcome.
- **Periods:** fit 1 November 2025 to 30 June 2026; test 1 July to 31 October 2026.
- **Model:** Normal(forecast + mean error, sd of error) per city. Outcomes are the seven ranges the template uses, centred on the rounded forecast, and each range is widened by half a degree at each end.
- **Score:** multi-outcome Brier score, averaged over test days.

## 4. What the result decides
- **If L1 is supported:** daily-low markets are listed and quoted around the model.
- **If it is not supported:** they are not listed. A market the maker cannot price better than climatology is not worth opening.
- **L2 is reported either way.**
