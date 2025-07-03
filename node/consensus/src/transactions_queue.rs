// Copyright (c) 2019-2025 Provable Inc.
// This file is part of the snarkOS library.

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at:

// http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, HashMap, hash_map::Entry},
    num::NonZeroUsize,
};

use anyhow::{Result, bail};
use lru::LruCache;
use snarkvm::{ledger::Transaction, prelude::*};

use crate::{CAPACITY_FOR_DEPLOYMENTS, CAPACITY_FOR_EXECUTIONS};

pub struct TransactionsQueue<N: Network> {
    pub deployments: TransactionsQueueInner<N>,
    pub executions: TransactionsQueueInner<N>,
}

impl<N: Network> Default for TransactionsQueue<N> {
    fn default() -> Self {
        Self {
            deployments: TransactionsQueueInner::new(CAPACITY_FOR_DEPLOYMENTS),
            executions: TransactionsQueueInner::new(CAPACITY_FOR_EXECUTIONS),
        }
    }
}

impl<N: Network> TransactionsQueue<N> {
    pub fn contains(&self, transaction_id: &N::TransactionID) -> bool {
        self.executions.contains(transaction_id) || self.deployments.contains(transaction_id)
    }

    pub fn insert(&mut self, transaction_id: N::TransactionID, transaction: Transaction<N>) -> Result<()> {
        if transaction.is_execute() {
            self.executions.insert(transaction_id, transaction)
        } else {
            self.deployments.insert(transaction_id, transaction)
        }
    }

    pub fn transactions(&self) -> impl Iterator<Item = (N::TransactionID, Transaction<N>)> {
        self.deployments
            .priority_queue
            .transactions
            .clone()
            .into_iter()
            .chain(self.deployments.queue.clone())
            .chain(self.executions.priority_queue.transactions.clone())
            .chain(self.executions.queue.clone())
    }
}

pub struct TransactionsQueueInner<N: Network> {
    capacity: usize,
    queue: LruCache<N::TransactionID, Transaction<N>>,
    priority_queue: PriorityQueue<N>,
}

impl<N: Network> TransactionsQueueInner<N> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            queue: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
            priority_queue: Default::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len().saturating_add(self.priority_queue.len())
    }

    fn contains(&self, transaction_id: &N::TransactionID) -> bool {
        self.queue.contains(transaction_id) || self.priority_queue.transactions.contains_key(transaction_id)
    }

    fn insert(&mut self, transaction_id: N::TransactionID, transaction: Transaction<N>) -> Result<()> {
        let priority_fee = transaction.priority_fee_amount()?;

        // If the queue is not full, insert in the appropriate queue.
        if self.len() < self.capacity {
            if priority_fee.is_zero() {
                self.queue.get_or_insert(transaction_id, || transaction);
            } else {
                self.priority_queue.insert(transaction_id, transaction, priority_fee);
            }

            return Ok(());
        }

        match (self.priority_queue.len() < self.capacity, *priority_fee) {
            // Invariant: if the queue is at capacity but the priority queue
            // isn't equal to the capacity, the low-priority queue must be non-empty.
            (true, 0) => {
                let _ = self.queue.get_or_insert(transaction_id, || transaction);
            }
            (true, _fee) => {
                // Remove an entry from the low-priority queue to make room for the high-priority transaction.
                self.queue.pop_lru();
                self.priority_queue.insert(transaction_id, transaction, priority_fee)
            }

            // Invariant: if the queue is at capacity but the priority queue is
            // equal to the capacity, the low-priority queue must be empty.
            (false, 0) => bail!("The memory pool is full"),
            (false, _fee) => self.priority_queue.compare_insert(transaction_id, transaction, priority_fee),
        }

        Ok(())
    }

    pub fn pop(&mut self) -> Option<(N::TransactionID, Transaction<N>)> {
        self.priority_queue.pop().or_else(|| self.queue.pop_lru())
    }
}

struct PriorityQueue<N: Network> {
    /// A counter to ensure fifo ordering for transmissions with the same fee.
    counter: u64,
    /// A map of transmissions ordered by fee and by fifo sequence.
    transaction_ids: BTreeMap<(Reverse<U64<N>>, u64), N::TransactionID>,
    /// A map of transmission IDs to transmissions.
    transactions: HashMap<N::TransactionID, Transaction<N>>,
}

impl<N: Network> Default for PriorityQueue<N> {
    /// Initializes a new instance of the priority queue.
    fn default() -> Self {
        Self { counter: Default::default(), transaction_ids: Default::default(), transactions: Default::default() }
    }
}

impl<N: Network> PriorityQueue<N> {
    fn len(&self) -> usize {
        self.transactions.len()
    }

    fn insert(&mut self, transaction_id: N::TransactionID, transaction: Transaction<N>, fee: U64<N>) {
        if let Entry::Vacant(entry) = self.transactions.entry(transaction_id) {
            // Insert the transaction into the map.
            entry.insert(transaction);
            // Sort by fee (highest first) and counter (fifo).
            self.transaction_ids.insert((Reverse(fee), self.counter), transaction_id);
            // Increment the counter.
            self.counter += 1;
        }
    }

    fn compare_insert(&mut self, transaction_id: N::TransactionID, transaction: Transaction<N>, fee: U64<N>) {
        // Make sure the collection isn't empty.
        if self.transaction_ids.is_empty() {
            return;
        }

        // If the lowest fee in the collection is higher than the new fee, no-op.
        //
        // SAFETY: the empty check guarantees an item will be returned
        let ((Reverse(lowest_fee), _), _) = self.transaction_ids.last_key_value().expect("item must be present");
        if lowest_fee > &fee {
            return;
        }

        // Otherwise, remove the current value and insert the new.
        //
        // SAFETY: the empty check guarantees an item will be returned
        let (_, id) = self.transaction_ids.pop_last().expect("item must be present");
        self.transactions.remove(&id);
        self.insert(transaction_id, transaction, fee);
    }

    fn pop(&mut self) -> Option<(N::TransactionID, Transaction<N>)> {
        let (_, transaction_id) = self.transaction_ids.pop_first()?;
        self.transactions.remove(&transaction_id).map(|transaction| (transaction_id, transaction))
    }
}
