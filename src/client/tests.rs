//-----------------------------------------------------------------------------
// Copyright (c) 2026, contributors to the cloudcoms/rust-oracledb fork.
//
// This file is dual-licensed under the same terms as the rest of this crate:
// the Universal Permissive License (UPL) 1.0 as shown at
// https://oss.oracle.com/licenses/upl and Apache License 2.0 as shown at
// http://www.apache.org/licenses/LICENSE-2.0. You may choose either license.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// tests.rs (client module)
//
// Exercises the connect timeout handling of the client against loopback
// peers. No database is involved.
//-----------------------------------------------------------------------------

use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::Client;
use crate::config::Config;
use crate::error::ErrorKind;

#[test]
fn connect_timeout_bounds_a_silent_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done_send, done_recv) = mpsc::channel::<()>();
    let peer = thread::spawn(move || {
        // accept the TCP connection but never answer the connect packet
        let (_sock, _) = listener.accept().unwrap();
        done_recv.recv().unwrap();
    });
    let config = Config::default()
        .set_connect_string(&format!("127.0.0.1:{port}/service"))
        .unwrap()
        .set_connect_timeout(Some(Duration::from_millis(300)));
    let mut client = Client::new(config, String::new());
    let started = Instant::now();
    let err = client.connect().err().unwrap();
    let elapsed = started.elapsed();
    done_send.send(()).unwrap();
    assert_eq!(err.kind(), &ErrorKind::ConnectTimeoutExceeded);
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    peer.join().unwrap();
}
