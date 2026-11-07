//! `brier-bench`: the registered P1/P2 engine benchmark (docs/notes/performance.md).
//!
//! Workload, generated before any timing: 10 open markets, 100 accounts.
//! - 60% passive GTC orders: buys at 40-49 and sells at 51-60, i.e. within ±10 ticks of 50
//!   and never crossing on arrival; quantity 1-100.
//! - 35% cancels of a uniformly chosen live order.
//! - 5% aggressive IOC orders that cross the spread: buys at 60 or sells at 40; quantity 1-100.
//!
//! To pick cancels of orders that are really live, generation runs its own engine and tracks
//! the live set from the events; the timed runs then replay the finished command list on fresh
//! engines. Each seed gets an untimed warm-up of 1,000,000 commands, then 10,000,000 measured
//! ones: once without instrumentation (throughput), once timing each command (latency).
//!
//! This is a measurement tool, not part of the order path, so floating-point reporting and
//! plain arithmetic on counters are allowed here.
#![allow(clippy::float_arithmetic, clippy::arithmetic_side_effects)]
#![allow(clippy::cast_precision_loss)] // counts and nanoseconds converted for reporting only

use std::collections::HashMap;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

use brier_engine::{
    Command, Engine, EngineError, Event, MarketState, NewOrder, Sequenced, TimeInForce,
};
use brier_types::{AccountId, MarketId, OrderId, Seq, Side};
use hdrhistogram::Histogram;

const MARKETS: u32 = 10;
const ACCOUNTS: u64 = 100;
const WARMUP: usize = 1_000_000;
const MEASURED: usize = 10_000_000;
const SEEDS: [u64; 5] = [1, 2, 3, 4, 5];
const TIMER_SAMPLES: usize = 1_000_000;

/// `SplitMix64` (Steele, Lea, Flood 2014): a small, fast, well-mixed generator. Enough for a
/// benchmark workload; modulo bias at these ranges is far below anything measurable here.
struct SplitMix64(u64);

impl SplitMix64 {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    const fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    #[allow(clippy::cast_possible_truncation)] // results are < 256 by construction
    const fn range_u8(&mut self, lo: u8, hi: u8) -> u8 {
        lo + self.below((hi - lo) as u64 + 1) as u8
    }

    #[allow(clippy::cast_possible_truncation)] // results are <= 100 by construction
    const fn qty(&mut self) -> u32 {
        1 + self.below(100) as u32
    }
}

/// Builds the workload, tracking which orders are live so cancels target real orders.
struct Workload {
    rng: SplitMix64,
    /// Live order IDs, for uniform choice.
    live: Vec<OrderId>,
    /// Per live order: owner, remaining quantity, index in `live`.
    info: HashMap<OrderId, (AccountId, u32, usize)>,
}

impl Workload {
    fn next_command(&mut self) -> Result<Command, EngineError> {
        let rng = &mut self.rng;
        let roll = rng.below(100);
        #[allow(clippy::cast_possible_truncation)] // below(MARKETS) < 10
        let market = MarketId::new(1 + rng.below(u64::from(MARKETS)) as u32);
        let account = AccountId::new(1 + rng.below(ACCOUNTS));
        if roll < 60 {
            let side = if rng.below(2) == 0 {
                Side::Buy
            } else {
                Side::Sell
            };
            let price = match side {
                Side::Buy => rng.range_u8(40, 49),
                Side::Sell => rng.range_u8(51, 60),
            };
            let qty = rng.qty();
            return Ok(Command::Place(NewOrder {
                account,
                market,
                side,
                price,
                qty,
                tif: TimeInForce::Gtc,
                post_only: false,
            }));
        }
        if roll < 95 && !self.live.is_empty() {
            #[allow(clippy::cast_possible_truncation)] // index < live.len()
            let pick = rng.below(self.live.len() as u64) as usize;
            let order = *self
                .live
                .get(pick)
                .ok_or(EngineError::Corrupt("bench: live index"))?;
            let owner = self
                .info
                .get(&order)
                .ok_or(EngineError::Corrupt("bench: live info"))?
                .0;
            return Ok(Command::Cancel {
                account: owner,
                order,
            });
        }
        let (side, price) = if rng.below(2) == 0 {
            (Side::Buy, 60)
        } else {
            (Side::Sell, 40)
        };
        let qty = rng.qty();
        Ok(Command::Place(NewOrder {
            account,
            market,
            side,
            price,
            qty,
            tif: TimeInForce::Ioc,
            post_only: false,
        }))
    }

    /// Update the live set from one command's events.
    fn observe(&mut self, command: Command, events: &[Event]) -> Result<(), EngineError> {
        for e in events {
            match *e {
                Event::Rested { order, remaining } => {
                    let Command::Place(o) = command else {
                        return Err(EngineError::Corrupt("bench: rested without a place"));
                    };
                    self.info
                        .insert(order, (o.account, remaining.contracts(), self.live.len()));
                    self.live.push(order);
                }
                Event::Trade(t) => {
                    let entry = self
                        .info
                        .get_mut(&t.maker)
                        .ok_or(EngineError::Corrupt("bench: unknown maker"))?;
                    entry.1 -= t.qty.contracts();
                    if entry.1 == 0 {
                        self.remove(t.maker)?;
                    }
                }
                Event::Cancelled { order, .. } if self.info.contains_key(&order) => {
                    self.remove(order)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn remove(&mut self, order: OrderId) -> Result<(), EngineError> {
        let (_, _, index) = self
            .info
            .remove(&order)
            .ok_or(EngineError::Corrupt("bench: removing unknown order"))?;
        self.live.swap_remove(index);
        if let Some(&moved) = self.live.get(index) {
            self.info
                .get_mut(&moved)
                .ok_or(EngineError::Corrupt("bench: moved order"))?
                .2 = index;
        }
        Ok(())
    }
}

/// The setup commands (list and open every market) followed by random workload commands,
/// `total` in all.
fn generate(seed: u64, total: usize) -> Result<Vec<Sequenced>, EngineError> {
    let mut workload = Workload {
        rng: SplitMix64(seed),
        live: Vec::new(),
        info: HashMap::new(),
    };
    let mut engine = Engine::new();
    let mut events = Vec::new();
    let mut commands = Vec::with_capacity(total);
    let mut seq = Seq::FIRST;
    let mut setup = Vec::new();
    for m in 1..=MARKETS {
        let market = MarketId::new(m);
        setup.push(Command::CreateMarket { market });
        setup.push(Command::SetState {
            market,
            state: MarketState::Open,
        });
    }
    let mut setup = setup.into_iter();
    while commands.len() < total {
        let command = match setup.next() {
            Some(c) => c,
            None => workload.next_command()?,
        };
        let cmd = Sequenced { seq, command };
        events.clear();
        engine.apply(&cmd, &mut events)?;
        workload.observe(command, &events)?;
        commands.push(cmd);
        seq = seq.next()?;
    }
    Ok(commands)
}

fn warm(
    engine: &mut Engine,
    commands: &[Sequenced],
    events: &mut Vec<Event>,
) -> Result<(), EngineError> {
    for c in commands {
        events.clear();
        engine.apply(c, events)?;
    }
    Ok(())
}

fn throughput(commands: &[Sequenced]) -> Result<f64, EngineError> {
    let (warmup, measured) = commands.split_at(WARMUP);
    let mut engine = Engine::new();
    let mut events = Vec::with_capacity(64);
    warm(&mut engine, warmup, &mut events)?;
    let start = Instant::now();
    for c in measured {
        events.clear();
        engine.apply(c, &mut events)?;
    }
    let seconds = start.elapsed().as_secs_f64();
    black_box(&events);
    black_box(engine.state_hash()?);
    Ok(measured.len() as f64 / seconds)
}

fn latency(commands: &[Sequenced], histogram: &mut Histogram<u64>) -> Result<(), EngineError> {
    let (warmup, measured) = commands.split_at(WARMUP);
    let mut engine = Engine::new();
    let mut events = Vec::with_capacity(64);
    warm(&mut engine, warmup, &mut events)?;
    for c in measured {
        events.clear();
        let start = Instant::now();
        engine.apply(c, &mut events)?;
        let ns = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        histogram.saturating_record(ns.max(1));
    }
    black_box(&events);
    Ok(())
}

fn timer_overhead() -> Result<Histogram<u64>, hdrhistogram::CreationError> {
    let mut h = new_histogram()?;
    for _ in 0..TIMER_SAMPLES {
        let start = Instant::now();
        let ns = u64::try_from(black_box(start).elapsed().as_nanos()).unwrap_or(u64::MAX);
        h.saturating_record(ns.max(1));
    }
    Ok(h)
}

fn new_histogram() -> Result<Histogram<u64>, hdrhistogram::CreationError> {
    // 1 ns to 10 s at 3 significant digits.
    Histogram::new_with_bounds(1, 10_000_000_000, 3)
}

fn percentiles(h: &Histogram<u64>) -> String {
    format!(
        "{{\"p50_ns\": {}, \"p90_ns\": {}, \"p99_ns\": {}, \"p99_9_ns\": {}, \"max_ns\": {}, \"count\": {}}}",
        h.value_at_quantile(0.50),
        h.value_at_quantile(0.90),
        h.value_at_quantile(0.99),
        h.value_at_quantile(0.999),
        h.max(),
        h.len()
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("brier-bench: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let overhead = timer_overhead()?;
    let mut pooled = new_histogram()?;
    let mut rates = Vec::new();
    for seed in SEEDS {
        let commands = generate(seed, WARMUP + MEASURED)?;
        let rate = throughput(&commands)?;
        latency(&commands, &mut pooled)?;
        eprintln!("seed {seed}: {rate:.0} commands/s");
        rates.push(rate);
    }
    let mut sorted = rates.clone();
    sorted.sort_by(f64::total_cmp);
    let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
    let rates_json: Vec<String> = rates.iter().map(|r| format!("{r:.0}")).collect();
    println!(
        "{{\"throughput_per_seed\": [{}], \"throughput_median\": {median:.0}, \"latency_pooled\": {}, \"timer_overhead\": {}}}",
        rates_json.join(", "),
        percentiles(&pooled),
        percentiles(&overhead)
    );
    Ok(())
}
