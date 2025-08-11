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
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant},
};

#[cfg(feature = "locktick")]
use locktick::parking_lot::RwLock;

#[cfg(not(feature = "locktick"))]
use parking_lot::RwLock;

#[derive(Default)]
pub struct BlockSyncMetrics {
    /// The number of block requests completed since the last update.
    completed_request_counter: AtomicU32,

    /// The current sync speed
    sync_speed: RwLock<f64>,
}

impl BlockSyncMetrics {
    const METRIC_UPDATE_WINDOW: Duration = Duration::from_secs(60);

    pub async fn update_loop(&self) {
        let mut last_update = Instant::now();

        loop {
            tokio::time::sleep(Self::METRIC_UPDATE_WINDOW).await;

            let now = Instant::now();
            let elapsed = now - last_update;

            let count = self.completed_request_counter.swap(0, Ordering::Relaxed);
            let speed = (count as f64) / elapsed.as_secs_f64();

            *self.sync_speed.write() = speed;
            last_update = now;
        }
    }

    #[inline]
    pub fn get_sync_speed(&self) -> f64 {
        *self.sync_speed.read()
    }

    #[inline]
    pub fn count_request_completed(&self) {
        self.completed_request_counter.fetch_add(1, Ordering::Relaxed);
    }
}
