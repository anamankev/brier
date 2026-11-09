# Pricing sports markets: does a simple rating model beat the base rate? (G10 U6)

Status: REGISTERED. Registered before any model in this note was fitted or tested; the registering commit is the first that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

## 1. Why
Sports markets (G10 U6) need a fair value for the reference market maker to quote around. A model that is worse than knowing only "home teams win about 55% of the time" would quote people into losing trades. This note tests a standard rating model on seasons it was not tuned on.

## 2. Hypotheses
- **S1 (NBA).** On the test season, the Elo model's mean Brier score for "the home team wins" is lower than the base-rate model's.
- **S2 (NFL).** The same, for the NFL.
- **S3 (soccer).** Pooled over the five leagues, the Elo model's mean three-outcome Brier score (home win, draw, away win) is lower than the base-rate model's.
- **S4 (calibration).** Pooled over NBA and NFL test games, in every probability bin of width 0.1 holding at least 100 forecasts, the observed home-win frequency is within 0.05 of the bin's mean forecast.

A hypothesis is supported if its inequality holds on the point estimates. Differences are reported with 95% intervals from 1,000 bootstrap resamples of test games (resampling seed 20261109); a supported hypothesis whose interval crosses zero is called weak.

## 3. Design
### Data
- **NBA and NFL:** every completed game (regular season and playoffs) from balldontlie, with the final score including overtime, which is what the markets settle on.
- **Soccer:** every completed match in the Premier League, La Liga, Serie A, Bundesliga and Ligue 1 from football-data.org, using the full-time score (league matches have no extra time).
- Games without a final score (postponed, cancelled) are dropped.

### Periods
- **NBA:** ratings warm up on the 2023-24 and 2024-25 seasons (the fit period); test on 2025-26.
- **NFL:** warm up on the 2023 and 2024 seasons; test on 2025.
- **Soccer:** warm up on 2024-25; test on 2025-26.

### Models
- **Elo**, with the parameters fixed here, not fitted:
  - Every team starts at 1500. After each game, each rating moves by K × (result − expected), where the result is 1 for a win, 0 for a loss and 0.5 for a draw.
  - The expected home score is E = 1 / (1 + 10^(−(home − away + H) / 400)).
  - **K and the home advantage H:** NBA K = 20, H = 100; NFL K = 20, H = 48; soccer K = 20, H = 60.
  - **Between seasons,** every rating moves a third of the way back to 1500.
  - Ratings update through the test season too, game by game, as they would live; each game is forecast before its own result is used.
  - **NBA and NFL:** P(home wins) = E. An NFL tie counts as "no" for the market.
  - **Soccer:** P(draw) = d, P(home) = (1 − d) × E, P(away) = (1 − d) × (1 − E), where d is the league's draw rate in the fit period.
- **Base rate:** the fit period's frequencies. For NBA and NFL, the home-win rate as a constant. For soccer, each league's home, draw and away frequencies.

### Scores
- **Brier score:** (p − outcome)² for a binary market; summed over the three outcomes for soccer. Averaged over test games. Lower is better.

## 4. What the result decides
- **If a hypothesis is supported:** that sport's markets are quoted around the Elo model.
- **If it is not supported:** that sport is quoted around the base rate instead, and the result is reported here.
- **Either way:** sports markets settle through the manual review lane (`docs/data-licenses.md`); the model only sets quotes.
