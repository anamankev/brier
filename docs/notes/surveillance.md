# Surveillance: how often are the detectors right? (G8)

Status: REGISTERED. Written before any detector code exists. The registering commit is the first commit that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

## 1. Questions
For each typology in SPEC §9 (wash trading, spoofing and layering, marking the close, insider-like trading): at a threshold chosen in advance on separate data, what share of the detector's alerts are real manipulation (precision), and what share of real manipulation does it catch (recall)?

## 2. Hypotheses
At the threshold chosen on the tuning seeds, measured on the test seeds:

| | Typology | Precision at least | Recall at least |
|---|---|---|---|
| V1 | Wash trading | 0.80 | 0.80 |
| V2 | Spoofing and layering | 0.70 | 0.70 |
| V3 | Marking the close | 0.70 | 0.70 |
| V4 | Insider-like trading | 0.60 | 0.60 |

A hypothesis is supported if both point estimates meet their targets. The 95% intervals are reported too, and a target inside an interval is said plainly, so a pass is never overstated.

## 3. Design
### Unit of evaluation
An (account, market) pair for each typology.
- **Label:** a pair is positive for a typology when that account was injected as that typology's manipulator in that market.
- **True positive:** an alert of typology T on a positive pair for T. Every other alert of T is a false positive. A positive pair with no alert of T is a false negative.

### Markets
- 480 steps each, run on the Python reference engine as in the market-maker study.
- **Tuning set:** seeds 1-100, used to choose thresholds.
- **Test set:** seeds 101-300, used to report.
- The hidden truth and the noise, informed, and market-maker agents are those of `docs/notes/market-maker.md` §3, with:
  - 10 noise accounts;
  - 3 informed accounts at signal sd 1.0;
  - the reference market maker.

### Injected manipulators
Each type is present in a market with probability 0.5, independently of the others, so a market has between none and four.
- **Wash pair:** two accounts, linked in the metadata. Three to eight times during the market (steps uniform at random), one rests an order within one tick of the last trade price, and the other takes it at once; 20-100 contracts each time. Between episodes, each trades like a noise trader.
- **Spoofer:** three to six episodes. In each, it rests an order 5-10 times the median resting size, one or two ticks from the touch. Within the next two steps it trades 5-20 contracts on the opposite side, then cancels the large order unfilled.
- **Closer:** buys 20-60 contracts early in the market. In the last 10 steps before the close, it sends aggressive buy orders that sweep 2-5 ticks above the best ask.
- **Insider:** an account created at step 400 or later (metadata), which in the last 30 steps takes 100-300 contracts on the side that will win, in immediate-or-cancel orders.

### Innocent look-alikes (present in every market)
- **Linked noise pair:** two noise traders linked in the metadata (a shared household). They trade independently, so they sometimes meet by chance.
- **Large passive trader:** rests large orders near the touch and cancels many of them unfilled, but without trading on the other side around the cancels.
- **Late informed trader:** one of the informed accounts at signal sd 0.25, active in the final 30 steps (correct far more often than not), with an account created at step 0.
- **Late noise:** noise arrivals in the final 10 steps at double the usual rate.

### Threshold choice and reporting
- For each typology, the threshold is the score that maximizes F1 on the tuning seeds. Ties go to the higher threshold, which is more conservative.
- Detector parameters (k, w, n) are fixed in the code before tuning. Only the alert threshold is tuned.
- **Test reporting:** precision, recall, F1, and counts on the test seeds, with 95% intervals from 1,000 bootstrap resamples of test seeds (resampling seed fixed at 20261108).
- **Also reported:** the false positives that come from the innocent look-alikes, by kind.

## 4. Results
Not yet run.
