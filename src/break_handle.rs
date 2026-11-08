//-----------------------------------------------------------------------------
// Copyright (c) 2026, contributors to the cloudcoms/rust-oracledb fork.
//
// This file is dual-licensed under the same terms as the rest of this crate:
// This software is dual-licensed to you under the Universal Permissive License
// (UPL) 1.0 as shown at https://oss.oracle.com/licenses/upl and Apache License
// 2.0 as shown at http://www.apache.org/licenses/LICENSE-2.0. You may choose
// either license.
//
// If you elect to accept the software under the Apache License, Version 2.0,
// the following applies:
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// break_handle.rs
//
// Defines the handle used to interrupt a call in progress from another
// thread. The handle never touches the socket: it only records the request.
// The thread performing the call notices it (see Client::receive_data_packet)
// and sends the in-band interrupt itself, so no second writer is needed for
// either TCP or TLS transports.
//-----------------------------------------------------------------------------

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const ENABLED: u64 = 1;
const REQUESTED: u64 = 2;
const FLAG_MASK: u64 = ENABLED | REQUESTED;
const GENERATION_STEP: u64 = FLAG_MASK + 1;

/// State shared between a connection and the handles created for it.
/// The generation and flags live in one atomic word, so an old handle can
/// never overwrite an interrupt requested by a newer connection user.
#[derive(Debug)]
pub(crate) struct BreakState {
    /// Upper bits hold the generation; the two low bits track whether the
    /// current user has created a handle and requested an interrupt.
    state: AtomicU64,
}

impl BreakState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU64::new(GENERATION_STEP),
        })
    }

    /// Clears a pending request without changing the handle generation.
    pub(crate) fn clear(&self) {
        self.state.fetch_and(!REQUESTED, Ordering::SeqCst);
    }

    /// Creates a handle bound atomically to the current generation.
    pub(crate) fn handle(self: &Arc<Self>) -> BreakHandle {
        let previous = self.state.fetch_or(ENABLED, Ordering::SeqCst);
        BreakHandle {
            state: Arc::clone(self),
            generation: previous & !FLAG_MASK,
        }
    }

    /// Returns whether any handle exists for the current generation.
    pub(crate) fn is_enabled(&self) -> bool {
        self.state.load(Ordering::SeqCst) & ENABLED != 0
    }

    /// Returns whether the current generation requested an interrupt.
    pub(crate) fn is_requested(&self) -> bool {
        self.state.load(Ordering::SeqCst) & REQUESTED != 0
    }

    /// Invalidates old handles and clears both flags atomically.
    pub(crate) fn new_generation(&self) {
        let mut current = self.state.load(Ordering::SeqCst);
        loop {
            let next = (current & !FLAG_MASK).wrapping_add(GENERATION_STEP);
            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

/// Interrupts a database call running on a [Connection](crate::Connection)
/// from another thread. Created by
/// [Connection::break_handle()](crate::Connection::break_handle()).
///
/// The handle is cheap to clone, is `Send + Sync` and never blocks: calling
/// [interrupt()](BreakHandle::interrupt) only records the request. The thread
/// that is waiting for the database notices it within about 100 ms, sends an
/// in-band interrupt and then waits for the database to acknowledge it. The
/// interrupted call normally fails with `ORA-01013: user requested cancel of
/// current operation` and the connection remains usable.
///
/// Notes:
///
/// - Only calls started after the first handle was created are covered.
/// - If no call is in progress, the next call on the connection is
///   interrupted as soon as it starts waiting for the database.
/// - The database decides when it acts on an interrupt. Some operations (for
///   example `DBMS_SESSION.SLEEP()`) only acknowledge it once they finish.
///   With a call timeout set, the client waits at most one call timeout for
///   the acknowledgement and then closes the connection.
/// - Handles created before the connection was returned to a pool have no
///   effect on later users of the pooled connection.
#[derive(Clone, Debug)]
pub struct BreakHandle {
    state: Arc<BreakState>,
    generation: u64,
}

impl BreakHandle {
    /// Requests that the call in progress on the connection be interrupted.
    pub fn interrupt(&self) {
        let mut current = self.state.state.load(Ordering::SeqCst);
        while (current & !FLAG_MASK) == self.generation {
            match self.state.state.compare_exchange_weak(
                current,
                current | REQUESTED,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupt_is_pending_until_cleared() {
        let state = BreakState::new();
        assert!(!state.is_enabled());
        let handle = state.handle();
        assert!(state.is_enabled());
        assert!(!state.is_requested());
        handle.clone().interrupt();
        assert!(state.is_requested());
        state.clear();
        assert!(!state.is_requested());
    }

    #[test]
    fn handles_from_a_previous_generation_have_no_effect() {
        let state = BreakState::new();
        let stale = state.handle();
        state.new_generation();
        assert!(!state.is_enabled());
        stale.interrupt();
        assert!(!state.is_requested());
        let current = state.handle();
        current.interrupt();
        assert!(state.is_requested());
        // A stale user must not erase a pending request from the new user.
        stale.interrupt();
        assert!(state.is_requested());
        state.clear();
        stale.interrupt();
        assert!(!state.is_requested());
        assert!(state.is_enabled());
        // a request racing with the end of the request is discarded
        state.new_generation();
        assert!(!state.is_requested());
    }
}
