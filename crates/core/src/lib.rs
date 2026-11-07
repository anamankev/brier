//! The exchange core as a library (SPEC §5, §12; ADR-0004; docs/gates/G4-plan.md).
//!
//! [`start`] recovers the engine (newest valid snapshot, then the journal after it) and runs
//! the group-commit loop on one dedicated thread, the only writer of the engine and journal.
//! Callers talk to it through a [`CoreHandle`]:
//! - [`CoreHandle::submit`] sends a command and resolves, with its sequence number and events,
//!   once the batch it joined is durable;
//! - [`CoreHandle::query`] reads engine state between batches, so it never sees a command that
//!   is not yet durable;
//! - [`CoreHandle::subscribe`] receives every durable batch, in order;
//! - [`CoreHandle::durable`] watches the highest durable sequence number (the journal follower
//!   reads only up to it).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use brier_engine::{
    AccountView, Command, Engine, EngineError, Event, MarketState, RestingOrder, Sequenced,
};
use brier_journal::{
    JournalError, JournalWriter, SEGMENT_BYTES, Truncation, latest_snapshot, recover_after,
    write_snapshot,
};
use brier_types::{AccountId, MarketId, Price, Seq, Side};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

/// Most commands in one group-commit batch. 8,192 records of at most 80 bytes stay under the
/// journal's `MAX_BATCH_BYTES` (1 MiB), so a full batch can never be refused.
pub const MAX_BATCH_COMMANDS: usize = 8_192;
/// Requests waiting for the core thread before senders are made to wait (backpressure).
pub const REQUEST_QUEUE: usize = 65_536;
/// Durable batches buffered for each subscriber; a subscriber further behind than this lags
/// and must resynchronize from a snapshot.
pub const BROADCAST_BUFFER: usize = 4_096;
/// Default commands between snapshots.
pub const DEFAULT_SNAPSHOT_EVERY: u64 = 100_000;

/// How to run the core.
#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Directory holding the journal and snapshots.
    pub data: PathBuf,
    /// Commands between snapshots.
    pub snapshot_every: u64,
    /// Journal segment size.
    pub segment_bytes: u64,
}

impl CoreConfig {
    /// Defaults for `data`.
    #[must_use]
    pub fn new(data: &Path) -> Self {
        Self {
            data: data.to_path_buf(),
            snapshot_every: DEFAULT_SNAPSHOT_EVERY,
            segment_bytes: SEGMENT_BYTES,
        }
    }
}

/// Why the core failed or refused a request.
#[derive(Debug)]
pub enum CoreError {
    /// The journal failed; the core stops, and nothing unsynced was acknowledged.
    Journal(JournalError),
    /// The engine halted, or recovery found an inconsistent state.
    Engine(String),
    /// The core thread has stopped; no further requests are possible.
    Stopped,
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Journal(e) => write!(f, "{e}"),
            Self::Engine(e) => write!(f, "engine: {e}"),
            Self::Stopped => f.write_str("the core has stopped"),
        }
    }
}

impl std::error::Error for CoreError {}

impl From<JournalError> for CoreError {
    fn from(e: JournalError) -> Self {
        Self::Journal(e)
    }
}

fn engine_error(e: impl std::fmt::Display) -> CoreError {
    CoreError::Engine(e.to_string())
}

/// One durable command: its sequence number, the command, and its events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    /// Sequence number assigned by the core.
    pub seq: Seq,
    /// The command as journaled.
    pub command: Command,
    /// The events it produced.
    pub events: Vec<Event>,
}

/// A batch of durable commands, in sequence order, as broadcast to subscribers.
pub type Batch = Arc<Vec<Committed>>;

/// What [`start`] returns: the handle, what recovery found, and the core thread (which ends
/// with the error that stopped it, or `Ok` once every handle is dropped).
pub type Started = (CoreHandle, Recovery, JoinHandle<Result<(), CoreError>>);

/// A read of engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// Every market and its state.
    Markets,
    /// One market's aggregated book.
    Book(MarketId),
    /// One account's balances, positions, and open orders.
    Account(AccountId),
}

/// One side of an aggregated book: (price, total resting quantity), best price first.
pub type Levels = Vec<(Price, u64)>;

/// The answer to a [`Query`], as of the last durable command (`as_of`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Every market and its state, ascending.
    Markets {
        /// The last durable sequence number this answer reflects.
        as_of: u64,
        /// Market IDs and states.
        markets: Vec<(MarketId, MarketState)>,
    },
    /// One market's aggregated book.
    Book {
        /// The last durable sequence number this answer reflects.
        as_of: u64,
        /// The market asked about.
        market: MarketId,
        /// Its state, or `None` if it does not exist.
        state: Option<MarketState>,
        /// Bid levels, best first.
        bids: Levels,
        /// Ask levels, best first.
        asks: Levels,
    },
    /// One account.
    Account {
        /// The last durable sequence number this answer reflects.
        as_of: u64,
        /// Balances and positions, or `None` if the account does not exist.
        view: Option<AccountView>,
        /// Open orders, ascending by order ID.
        open_orders: Vec<(MarketId, Side, RestingOrder)>,
    },
}

/// What recovery found when the core started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// The sequence number the next command will get.
    pub next_seq: Seq,
    /// The engine's state hash after recovery.
    pub state_hash: [u8; 32],
    /// The snapshot recovery started from, if any.
    pub snapshot: Option<Seq>,
    /// Snapshot files skipped as damaged, with reasons.
    pub skipped_snapshots: Vec<(PathBuf, &'static str)>,
    /// Journal commands replayed after the snapshot.
    pub replayed: u64,
    /// A torn tail removed from the journal, if any.
    pub truncation: Option<Truncation>,
}

enum Request {
    Command {
        command: Command,
        reply: oneshot::Sender<Result<Committed, CoreError>>,
    },
    Query {
        query: Query,
        reply: oneshot::Sender<Answer>,
    },
}

/// A cloneable handle to the running core.
#[derive(Clone)]
pub struct CoreHandle {
    requests: mpsc::Sender<Request>,
    batches: broadcast::Sender<Batch>,
    durable: watch::Receiver<u64>,
}

impl CoreHandle {
    /// Submit a command; resolves once its batch is durable.
    ///
    /// # Errors
    /// [`CoreError::Stopped`] if the core has stopped, or the error that stopped it while the
    /// command's batch was pending (the command was then not acknowledged).
    pub async fn submit(&self, command: Command) -> Result<Committed, CoreError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Command { command, reply })
            .await
            .map_err(|_| CoreError::Stopped)?;
        answer.await.map_err(|_| CoreError::Stopped)?
    }

    /// Submit a command without waiting, from a non-async thread; the returned receiver
    /// resolves once the command is durable. Used by the pipelined `serve` adapter.
    ///
    /// # Errors
    /// [`CoreError::Stopped`] if the core has stopped.
    pub fn submit_nowait(
        &self,
        command: Command,
    ) -> Result<oneshot::Receiver<Result<Committed, CoreError>>, CoreError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .blocking_send(Request::Command { command, reply })
            .map_err(|_| CoreError::Stopped)?;
        Ok(answer)
    }

    /// Read engine state as of the last durable command.
    ///
    /// # Errors
    /// [`CoreError::Stopped`] if the core has stopped.
    pub async fn query(&self, query: Query) -> Result<Answer, CoreError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(Request::Query { query, reply })
            .await
            .map_err(|_| CoreError::Stopped)?;
        answer.await.map_err(|_| CoreError::Stopped)
    }

    /// Every durable batch from now on, in order.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Batch> {
        self.batches.subscribe()
    }

    /// The highest durable sequence number (0 before any command).
    #[must_use]
    pub fn durable(&self) -> watch::Receiver<u64> {
        self.durable.clone()
    }
}

/// Rebuild the engine from the newest valid snapshot plus the journal after it.
///
/// # Errors
/// [`CoreError`] if the journal is corrupt or the recovered state is inconsistent.
pub fn recover_engine(data: &Path) -> Result<(Engine, Recovery), CoreError> {
    std::fs::create_dir_all(data).map_err(JournalError::from)?;
    let latest = latest_snapshot(data)?;
    let (mut engine, snapshot_seq) = match &latest.snapshot {
        Some(s) => {
            let engine = Engine::restore(&s.body).map_err(engine_error)?;
            if engine.state_hash().map_err(engine_error)? != s.state_hash {
                return Err(CoreError::Engine(format!(
                    "snapshot {} restores to a different state hash than it recorded",
                    s.seq.get()
                )));
            }
            (engine, s.seq.get())
        }
        None => (Engine::new(), 0),
    };
    let mut events = Vec::new();
    let mut replayed: u64 = 0;
    let recovered =
        recover_after::<CoreError>(data, Seq::from_raw(snapshot_seq), |seq, command| {
            if seq.get() <= snapshot_seq {
                return Ok(());
            }
            events.clear();
            engine
                .apply(&Sequenced { seq, command }, &mut events)
                .map_err(engine_error)?;
            replayed = replayed.saturating_add(1);
            Ok(())
        })?;
    let last = recovered.last_seq.map_or(0, Seq::get);
    if last < snapshot_seq {
        return Err(CoreError::Engine(format!(
            "snapshot at {snapshot_seq} is ahead of the journal, which ends at {last}"
        )));
    }
    let recovery = Recovery {
        next_seq: Seq::from_raw(last.saturating_add(1)),
        state_hash: engine.state_hash().map_err(engine_error)?,
        snapshot: (snapshot_seq > 0).then_some(Seq::from_raw(snapshot_seq)),
        skipped_snapshots: latest.skipped,
        replayed,
        truncation: recovered.truncation,
    };
    Ok((engine, recovery))
}

/// Recover, then start the core thread.
///
/// # Errors
/// Any [`CoreError`] from recovery or from opening the journal.
pub fn start(config: CoreConfig) -> Result<Started, CoreError> {
    let (engine, recovery) = recover_engine(&config.data)?;
    let journal = JournalWriter::open_with_segment_limit(
        &config.data,
        recovery.next_seq,
        config.segment_bytes,
    )?;
    let (requests, inbox) = mpsc::channel(REQUEST_QUEUE);
    let (batches, _) = broadcast::channel(BROADCAST_BUFFER);
    let durable_now = recovery.next_seq.get().saturating_sub(1);
    let (durable_tx, durable) = watch::channel(durable_now);
    let publisher = batches.clone();
    let thread = thread::Builder::new()
        .name("brier-core".to_owned())
        .spawn(move || run(engine, journal, &config, inbox, &publisher, &durable_tx))
        .map_err(|e| CoreError::Journal(JournalError::Io(e)))?;
    Ok((
        CoreHandle {
            requests,
            batches,
            durable,
        },
        recovery,
        thread,
    ))
}

fn answer(engine: &Engine, as_of: u64, query: Query) -> Result<Answer, EngineError> {
    Ok(match query {
        Query::Markets => Answer::Markets {
            as_of,
            markets: engine
                .markets()
                .filter_map(|m| engine.market_state(m).map(|s| (m, s)))
                .collect(),
        },
        Query::Book(market) => {
            let levels = |side| -> Result<Levels, EngineError> {
                let mut out: Levels = Vec::new();
                for o in engine.resting(market, side)? {
                    let qty = u64::from(o.remaining.contracts());
                    match out.last_mut() {
                        Some((price, total)) if *price == o.price => {
                            *total = total.saturating_add(qty);
                        }
                        _ => out.push((o.price, qty)),
                    }
                }
                Ok(out)
            };
            Answer::Book {
                as_of,
                market,
                state: engine.market_state(market),
                bids: levels(Side::Buy)?,
                asks: levels(Side::Sell)?,
            }
        }
        Query::Account(account) => Answer::Account {
            as_of,
            view: engine.accounts().into_iter().find(|v| v.account == account),
            open_orders: engine.orders_of(account)?,
        },
    })
}

/// The core thread: batch, journal, apply, sync, then reply and publish.
fn run(
    mut engine: Engine,
    mut journal: JournalWriter,
    config: &CoreConfig,
    mut inbox: mpsc::Receiver<Request>,
    publisher: &broadcast::Sender<Batch>,
    durable: &watch::Sender<u64>,
) -> Result<(), CoreError> {
    let mut since_snapshot: u64 = 0;
    while let Some(first) = inbox.blocking_recv() {
        let mut requests = vec![first];
        while requests.len() < MAX_BATCH_COMMANDS {
            match inbox.try_recv() {
                Ok(r) => requests.push(r),
                Err(_) => break,
            }
        }
        let mut committed: Vec<(Committed, oneshot::Sender<Result<Committed, CoreError>>)> =
            Vec::new();
        let mut queries = Vec::new();
        let mut failure = None;
        for request in requests {
            match request {
                Request::Query { query, reply } => queries.push((query, reply)),
                Request::Command { command, reply } => {
                    if failure.is_some() {
                        drop(reply); // the batch will not be acknowledged
                        continue;
                    }
                    let seq = journal.next_seq();
                    let mut events = Vec::new();
                    let step = journal
                        .append(seq, &command)
                        .map_err(CoreError::from)
                        .and_then(|()| {
                            engine
                                .apply(&Sequenced { seq, command }, &mut events)
                                .map_err(engine_error)
                        });
                    match step {
                        Ok(()) => committed.push((
                            Committed {
                                seq,
                                command,
                                events,
                            },
                            reply,
                        )),
                        Err(e) => failure = Some(e),
                    }
                }
            }
        }
        if let Some(e) = failure {
            // A halt or journal failure drops the whole unsynced batch: nothing in it is
            // acknowledged, and recovery will not find it (it was never synced).
            return Err(e);
        }
        journal.commit()?;
        let last = journal.next_seq().get().saturating_sub(1);
        durable.send_replace(last);
        let batch: Vec<Committed> = committed.iter().map(|(c, _)| c.clone()).collect();
        since_snapshot = since_snapshot.saturating_add(batch.len() as u64);
        for (c, reply) in committed {
            let _ = reply.send(Ok(c)); // the requester may have given up; that is its choice
        }
        if !batch.is_empty() {
            let _ = publisher.send(Arc::new(batch)); // no subscribers is fine
        }
        for (query, reply) in queries {
            let _ = reply.send(answer(&engine, last, query).map_err(engine_error)?);
        }
        if last > 0 && since_snapshot >= config.snapshot_every {
            since_snapshot = 0;
            let body = engine.snapshot().map_err(engine_error)?;
            write_snapshot(
                &config.data,
                Seq::from_raw(last),
                &engine.state_hash().map_err(engine_error)?,
                &body,
            )?;
        }
    }
    journal.commit()?;
    Ok(())
}
