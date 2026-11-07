//! Per-account windows of client order IDs, for idempotent order entry (SPEC §6;
//! docs/protocol.md §3).
//!
//! When an order carrying a client order ID is accepted, the ID is recorded against the
//! account. A later order from the same account with the same ID, while it is still in the
//! window, is answered with `duplicate` and the original order ID instead of trading again.
//! A client that retries after a timeout therefore cannot double its order. The window keeps
//! the most recent [`CLIENT_ID_WINDOW`] accepted IDs per account, evicting the oldest first.
//! This is engine state: deterministic, in snapshots, and rebuilt by replay.

use std::collections::{HashMap, VecDeque};

use brier_types::OrderId;

/// Accepted client order IDs remembered per account.
pub const CLIENT_ID_WINDOW: usize = 10_000;

/// One account's window: lookups by ID (a hash map, never iterated) and eviction order.
#[derive(Debug, Clone, Default)]
pub(crate) struct ClientIds {
    by_id: HashMap<u64, OrderId>,
    order: VecDeque<(u64, OrderId)>,
}

impl ClientIds {
    pub(crate) fn get(&self, client_id: u64) -> Option<OrderId> {
        self.by_id.get(&client_id).copied()
    }

    /// Record an accepted order's client ID, evicting the oldest beyond the window.
    pub(crate) fn record(&mut self, client_id: u64, order: OrderId) {
        self.by_id.insert(client_id, order);
        self.order.push_back((client_id, order));
        if self.order.len() > CLIENT_ID_WINDOW
            && let Some((old, _)) = self.order.pop_front()
        {
            self.by_id.remove(&old);
        }
    }

    /// Entries oldest first: the canonical order for hashing and snapshots.
    pub(crate) fn entries(&self) -> impl Iterator<Item = (u64, OrderId)> + '_ {
        self.order.iter().copied()
    }

    pub(crate) fn len(&self) -> usize {
        self.order.len()
    }

    /// The map and the eviction queue agree, and the window is within its bound.
    pub(crate) fn consistent(&self) -> bool {
        self.order.len() <= CLIENT_ID_WINDOW
            && self.order.len() == self.by_id.len()
            && self
                .order
                .iter()
                .all(|(id, order)| self.by_id.get(id) == Some(order))
    }
}
