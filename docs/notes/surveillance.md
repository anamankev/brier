# Surveillance: how often are the detectors right? (G8)

Status: COMPLETE. V1 and V4 supported; V2 and V3 not supported (§4). Registered before any detector code existed. The registering commit is the first commit that contains this file. Nothing below changes after results are seen; a change would be a new, dated section.

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

## 3a. Amendment, 7 November 2026, before any detector or simulation code existed
Writing the simulation exposed details §3 left open, and one conflict. They are settled here before anything has run. Nothing in §2 changes.

### The conflict
In the market-maker study the maker pulls its quotes 15 steps before the close. With that, the book would be empty during the last 10 steps, which is exactly where §3's closer acts. **In these markets the maker quotes until the close.** Everything else about the maker is as in the market-maker study.

### The ordinary traders
- **Noise traders.** The market-maker study's noise flow is shared among the 10 noise accounts: each step, with probability 0.5, one account chosen uniformly at random arrives. So each account arrives with probability 0.05 a step. Every other account below that "trades like a noise trader" also arrives with probability 0.05 a step.
- **Outcome.** The hidden truth's path is drawn before trading starts (it does not depend on trading), and the outcome is drawn from its end, as in the market-maker study. The insider is told the outcome.

### The manipulators
- **Wash pair.** In an episode, one account rests a GTC order at the last trade price (50 if nothing has traded yet), and the other takes all of it at once with an immediate-or-cancel order.
- **Spoofer.**
  - The large order is a GTC order, 5-10 times 25 contracts (25 is the maker's quote size, the usual resting size).
  - The opposite-side trade is an immediate-or-cancel order at the best opposite price, one step after the large order.
  - The large order is cancelled one step after that.
- **Closer.** In each of the last 10 steps, with probability 0.6, it sends an immediate-or-cancel buy of 10-30 contracts priced 2-5 ticks above the best ask.
- **Insider.**
  - Its account opens at a step uniform in 400-449.
  - In each of the last 30 steps, with probability 0.5, it sends an immediate-or-cancel order for 20-60 contracts on the winning side, priced 3 ticks through the touch.
  - It stops once it has 100-300 contracts (the target is drawn at random).

### The innocent look-alikes
- **Linked noise pair.** Half their arrivals rest a GTC order at their own side's best price, and cancel it after 5 steps if it is unfilled. The other half take, like other noise traders.
- **Large passive trader.**
  - Each step, with probability 0.1, it rests a GTC order of 4-8 times 25 contracts, one or two ticks behind the touch, on a random side.
  - It cancels the order after 1-3 steps if unfilled.
  - It never trades on the other side.
- **Late informed trader.** It is active all market at the informed rate (0.15 a step) and at 0.6 a step in the final 30 steps.
- **Late noise.** In the final 10 steps, the noise arrival probability is 1.0, double the usual 0.5.

### Detector parameters, fixed now
The detectors measure time in market commands: the commands that concern a market (orders in it, cancels and amends of its orders, its own state changes). The simulation makes about six per step.

| Parameter | Value |
|---|---|
| Spoofing: size multiple k | 4 × the median size of the market's GTC orders |
| Spoofing: distance from the touch | 2 ticks or fewer |
| Spoofing: window w | 30 market commands |
| Spoofing: weight of an order with no opposite-side trade within w of its cancel | 0.25 |
| Marking the close: window n | the last 60 market commands before the close |
| Insider-like: window n | the last 180 market commands before the close |

## 3b. Second amendment, 7 November 2026, before any simulation ran
Writing the agents settled four more details. Nothing in §2 or §3a's detector parameters changes.

1. **Where the wash order rests.** §3a put it at the last trade price. In these markets, that price is almost always the maker's own bid or ask, so the other account's order would trade with the maker first, and the episode would not be a wash trade. The resting order goes instead at the price strictly inside the spread nearest the last trade price, which is within a tick or two of it, as §3 intended. A step with no price inside the spread has no episode, and the episode moves to the next step.
2. **The informed flow.** As with noise, the three informed accounts share the market-maker study's flow: each step, with probability 0.15, one of them, chosen uniformly at random, arrives.
3. **The late informed trader** is a fourth informed account, separate from the three, with its own rates (§3a).
4. **The closer's early position.** At a step uniform in 20-100, it sends one immediate-or-cancel buy of 20-60 contracts priced 2 ticks above the best ask.

## 4. Results
Run on 7 November 2026, with the code at commit `bc10435`, the first commit with the simulation and the evaluation runner:

    python -m brier_services.surveillance.study

The run took 4.2 seconds for 300 markets. The test seeds are 101-300; the thresholds were chosen on seeds 1-100.

| | Typology | Threshold | TP | FP | FN | Precision (95% CI) | Recall (95% CI) | Verdict |
|---|---|---|---|---|---|---|---|---|
| V1 | Wash trading | 0.299 | 190 | 0 | 4 | 1.000 (1.000-1.000) | 0.979 (0.951-1.000) | **Supported** |
| V2 | Spoofing and layering | 1.25 | 84 | 200 | 6 | 0.296 (0.262-0.331) | 0.933 (0.880-0.979) | **Not supported**: precision far below 0.70 |
| V3 | Marking the close | 2.489 | 68 | 59 | 29 | 0.535 (0.455-0.609) | 0.701 (0.606-0.793) | **Not supported**: precision below 0.70. Recall meets 0.70 only just, and 0.70 is inside its interval |
| V4 | Insider-like trading | 194 | 102 | 0 | 1 | 1.000 (1.000-1.000) | 0.990 (0.964-1.000) | **Supported** |

### Where the false positives came from (test seeds)
- **Spoofing:** all 200 were the large passive trader, one in every test market. It rests large orders near the touch and pulls them unfilled, so under the registered score its 0.25-weighted cancels add up past the spoofer's paired ones. The registered score counts the cancels when it should be asking what the cancels were for.
- **Marking the close:** 30 from the late informed trader, 18 from the insider, 11 from noise traders. A trader who is right late and trades aggressively moves the last price as much as a manipulator does, and this score cannot tell the two apart.
- **Wash trading and insider-like trading:** none. No chance meeting of the linked noise pair came close to the threshold.

### What the passes do and do not show
- **Wash trading** is detected well because the simulated wash pair is linked. Wash traders who use unlinked accounts (different networks) are invisible to this detector, by construction.
- **The insider result leans on the new-account weight.** The simulated insider always opens its account late. §5 measures the detector without that weight.

## 5. Exploratory follow-ups (not registered)
These were chosen after §4's results were seen, so they test no hypothesis. To keep them honest they are measured on fresh seeds 301-500, which nothing had used. Thresholds are tuned on seeds 1-100 as before.

    python -m brier_services.surveillance.explore

| Variant | Threshold | TP | FP | FN | Precision | Recall |
|---|---|---|---|---|---|---|
| Spoofing, registered | 1.25 | 100 | 200 | 6 | 0.333 | 0.943 |
| Spoofing, unpaired cancels weigh 0 | 2.0 | 93 | 149 | 13 | 0.384 | 0.877 |
| Spoofing, only orders paired with an opposite trade **the account initiated** | 1.0 | 105 | 0 | 1 | 1.000 | 0.991 |
| Insider-like, registered | 194 | 100 | 0 | 1 | 1.000 | 0.990 |
| Insider-like, no weight for a new account | 108 | 99 | 5 | 2 | 0.952 | 0.980 |

### Spoofing
Dropping the unpaired cancels alone does little. The large passive trader's resting orders on the other side sometimes fill near one of its cancels, and that counts as "trading the other way". The spoofer is different: it trades the other way as the taker. Counting only opposite trades the account initiated separates the two almost perfectly.

Two caveats:
- The simulated look-alike never takes liquidity, so this rule fits the simulation's construction. A real market maker who pulls a large quote and then hits the other side would be flagged.
- The rule is new, and the gateway runs it as such: live surveillance uses this variant with the threshold tuned here (1.0), and the registered score is kept only in the evaluation.

### Insider-like trading
Without the new-account weight, precision falls to 0.95. The detector mainly measures concentration on the winning side late in the market, and the weight adds a little.

### Marking the close
Not explored. The likely fix is to use what happens after the push: a manipulator buys the close up whatever the outcome, while an informed late trader is right. That needs a new registration, not a variant of this one.

## 6. What this measures, and what it does not
- **This is an evaluation in simulation.** The manipulators were written by the same person as the detectors, and to a specification published in advance. Real manipulators adapt.
- **The numbers rank the detectors against these specific look-alikes.** They are not field accuracy.
- **What carries over to the live exchange** is the code: the detectors read the same command stream from a live journal (`brier-core export`) as from the simulation.
