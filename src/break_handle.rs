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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// State shared between a connection and the handles created for it.
#[derive(Debug)]
pub(crate) struct BreakState {
    /// Changes whenever the connection ends a request (for example when it is
    /// returned to a pool) so that handles created earlier have no effect.
    generation: AtomicU64,
    /// Generation for which an interrupt was requested, or 0 for none.
    requested: AtomicU64,
    /// Set once a handle exists. While set, the client waits for the database
    /// in short slices so that it can notice a request.
    enabled: AtomicBool,
}

impl BreakState {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            generation: AtomicU64::new(1),
            requested: AtomicU64::new(0),
            enabled: AtomicBool::new(false),
        })
    }

    /// Clears a pending request.
    pub(crate) fn clear(&self) {
        self.requested.store(0, Ordering::SeqCst);
    }

    /// Creates a handle bound to the current generation.
    pub(crate) fn handle(self: &Arc<Self>) -> BreakHandle {
        self.enabled.store(true, Ordering::SeqCst);
        BreakHandle {
            state: Arc::clone(self),
            generation: self.generation.load(Ordering::SeqCst),
        }
    }

    /// Returns whether any handle exists for the current generation.
    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Returns whether an interrupt was requested by a current handle.
    pub(crate) fn is_requested(&self) -> bool {
        let requested = self.requested.load(Ordering::SeqCst);
        requested != 0 && requested == self.generation.load(Ordering::SeqCst)
    }

    /// Invalidates all existing handles.
    pub(crate) fn new_generation(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.enabled.store(false, Ordering::SeqCst);
        self.clear();
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
        self.state
            .requested
            .store(self.generation, Ordering::SeqCst);
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
        // a request racing with the end of the request is discarded
        state.new_generation();
        assert!(!state.is_requested());
    }
}
