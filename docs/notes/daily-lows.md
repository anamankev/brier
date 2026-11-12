# Pricing daily lows: does the forecast model beat climatology? (G10 U5)

Status: COMPLETE. L1 supported (11 of 12 cities); L2 not supported (§5). Registered before any model in this note was fitted or tested; the registering commit is the first that contains this file. Nothing below changes after results are seen.

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

## 5. Results
Run on 12 November 2026 with the code at commit `d634f4d`, the first commit with the runner. The registration was committed earlier, at `d180d1b`:

    python -m brier_services.catalog.backtest --lows

| City | Bias (°F) | Error sd (°F) | Test days | Model Brier | Climatology Brier | Difference (95% CI) |
|---|---|---|---|---|---|---|
| atlanta | +1.71 | 2.10 | 123 | 0.746 | 0.962 | -0.216 (-0.275 to -0.157) |
| austin | -2.49 | 4.24 | 123 | 0.830 | 1.003 | -0.173 (-0.230 to -0.116) |
| boston | +2.88 | 3.49 | 123 | 0.785 | 0.910 | -0.125 (-0.188 to -0.066) |
| chicago | +1.22 | 3.06 | 123 | 0.761 | 1.101 | -0.340 (-0.400 to -0.284) |
| dallas | +0.93 | 2.31 | 123 | 0.696 | 1.066 | -0.370 (-0.426 to -0.320) |
| denver | -2.41 | 3.72 | 123 | 0.848 | 1.136 | -0.288 (-0.353 to -0.229) |
| los-angeles | +2.57 | 2.08 | 123 | 0.760 | 0.887 | -0.128 (-0.201 to -0.055) |
| miami | +2.93 | 2.34 | 122 | 0.748 | 0.801 | -0.053 (-0.097 to -0.007) |
| nyc | +4.38 | 4.13 | 123 | 0.813 | 0.777 | +0.036 (-0.019 to +0.088) |
| phoenix | +4.97 | 2.18 | 123 | 0.627 | 0.785 | -0.158 (-0.235 to -0.083) |
| seattle | +0.38 | 2.90 | 123 | 0.769 | 0.839 | -0.070 (-0.116 to -0.017) |
| washington-dc | +2.58 | 4.06 | 123 | 0.845 | 0.985 | -0.140 (-0.187 to -0.093) |

| Bin | Forecasts | Mean forecast | Observed | Within 0.05 |
|---|---|---|---|---|
| 0.0-0.1 | 4425 | 0.033 | 0.027 | yes |
| 0.1-0.2 | 2951 | 0.157 | 0.190 | yes |
| 0.2-0.3 | 1843 | 0.243 | 0.240 | yes |
| 0.3-0.4 | 860 | 0.330 | 0.284 | yes |
| 0.4-0.5 | 123 | 0.489 | 0.309 | no |
| 0.5-0.6 | 123 | 0.586 | 0.569 | yes |

- **L1 is supported:** the model beats climatology in 11 of 12 cities. In 10 of them the interval is clear of zero; in Miami it is barely clear. **New York is the exception:** worse than climatology by 0.036, with an interval that crosses zero. The Open-Meteo grid point for Central Park runs about 4 °F warm overnight, and the error spread (4.1 °F) is the largest of the twelve.
- **L2 is not supported:** the 0.4-0.5 bin (123 forecasts) observed 0.31 against a mean forecast of 0.49. The other five counted bins are within 0.05.
- **Under §4:** daily-low markets are listed in all 12 cities and quoted around the model, with each city's fitted bias and spread (`marketmaker/catalog_values.TEMPERATURE_LOW_FIT`).
- **New York's quotes** are wider in effect, because of the larger spread. A city-specific change would be a new registration.
