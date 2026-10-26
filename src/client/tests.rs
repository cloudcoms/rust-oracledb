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
// Exercises the break and timeout handling of the client against a scripted
// loopback peer that speaks just enough of the TNS framing. No database is
// involved; the database side of these exchanges (ORA-01013 and session
// reuse) is covered by tests/test_3600_break.rs.
//-----------------------------------------------------------------------------

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::Client;
use crate::config::Config;
use crate::constants;
use crate::error::ErrorKind;
use crate::messages::PingMessage;

/// The peer side of the loopback connection.
struct Peer {
    sock: TcpStream,
}

impl Peer {
    /// Reads one packet and returns its type and body (after the 8-byte
    /// header, including the data flags of data packets).
    fn read_packet(&mut self) -> (u8, Vec<u8>) {
        let mut len = [0u8; 2];
        self.sock.read_exact(&mut len).unwrap();
        let len = u16::from_be_bytes(len) as usize;
        let mut rest = vec![0u8; len - 2];
        self.sock.read_exact(&mut rest).unwrap();
        (rest[2], rest[6..].to_vec())
    }

    /// Reads one packet and asserts that it is a marker of the given type.
    fn expect_marker(&mut self, marker_type: u8) {
        let (packet_type, body) = self.read_packet();
        assert_eq!(packet_type, constants::PACKET_TYPE_MARKER);
        assert_eq!(body, [1, 0, marker_type]);
    }

    /// Reads one packet and asserts that it is a data packet (a request).
    fn expect_request(&mut self) {
        let (packet_type, _) = self.read_packet();
        assert_eq!(packet_type, constants::PACKET_TYPE_DATA);
    }

    fn send(&mut self, packet_type: u8, body: &[u8]) {
        let len = u16::try_from(body.len() + 8).unwrap();
        let mut bytes = len.to_be_bytes().to_vec();
        bytes.extend([0, 0, packet_type, 0, 0, 0]);
        bytes.extend(body);
        self.sock.write_all(&bytes).unwrap();
        self.sock.flush().unwrap();
    }

    fn send_marker(&mut self, marker_type: u8) {
        self.send(constants::PACKET_TYPE_MARKER, &[1, 0, marker_type]);
    }

    /// Sends a complete response consisting of a single status message.
    fn send_status(&mut self) {
        let body = [0, 0, constants::TTC_MSG_TYPE_STATUS, 0, 0];
        self.send(constants::PACKET_TYPE_DATA, &body);
    }

    /// Answers an interrupt the way the database does: break marker, wait
    /// for the client's reset, reset marker, then a response.
    fn acknowledge_interrupt(&mut self) {
        self.send_marker(constants::_MARKER_TYPE_BREAK);
        self.expect_marker(constants::MARKER_TYPE_RESET);
        self.send_marker(constants::MARKER_TYPE_RESET);
        self.send_status();
    }
}

/// Returns a client connected to a peer running the given script.
fn connect<F>(script: F) -> (Client, thread::JoinHandle<()>)
where
    F: FnOnce(&mut Peer) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = thread::spawn(move || {
        let (sock, _) = listener.accept().unwrap();
        script(&mut Peer { sock });
    });
    // a connect string gives the default SDU; nothing connects to it
    let config = Config::default()
        .set_connect_string(&format!("{addr}/service"))
        .unwrap();
    let mut client = Client::new(config, String::new());
    client
        .transport
        .connect_stream_for_test(TcpStream::connect(addr).unwrap());
    (client, peer)
}

fn ping(client: &mut Client) -> Result<(), crate::Error> {
    client
        .perform_round_trip(&mut PingMessage::new())
        .map(|_| ())
}

#[test]
fn interrupt_during_a_call_sends_marker_and_resets() {
    let (sent_send, sent_recv) = mpsc::channel::<Instant>();
    let (mut client, peer) = connect(move |peer| {
        peer.expect_request();
        peer.expect_marker(constants::MARKER_TYPE_INTERRUPT);
        sent_send.send(Instant::now()).unwrap();
        peer.acknowledge_interrupt();
        // the session is usable afterwards
        peer.expect_request();
        peer.send_status();
    });
    let handle = client.break_state().handle();
    let requested = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        handle.interrupt();
        Instant::now()
    });
    ping(&mut client).unwrap();
    let requested_at = requested.join().unwrap();
    let delay = sent_recv.recv().unwrap() - requested_at;
    assert!(
        delay < Duration::from_millis(400),
        "interrupt took {delay:?}"
    );
    assert!(!client.break_in_progress);
    assert!(!client.break_state.is_requested());
    ping(&mut client).unwrap();
    peer.join().unwrap();
}

#[test]
fn late_interrupt_answer_is_consumed_before_the_next_call() {
    let (mut client, peer) = connect(|peer| {
        peer.expect_request();
        // the request was pending, so the interrupt follows the call
        peer.expect_marker(constants::MARKER_TYPE_INTERRUPT);
        // the call completes before the database acts on the interrupt
        peer.send_status();
        peer.acknowledge_interrupt();
        peer.expect_request();
        peer.send_status();
    });
    client.break_state().handle().interrupt();
    ping(&mut client).unwrap();
    assert!(!client.break_in_progress);
    // without draining, this call would read the stale break marker
    ping(&mut client).unwrap();
    peer.join().unwrap();
}

#[test]
fn call_timeout_still_interrupts_while_polling() {
    let (sent_send, sent_recv) = mpsc::channel::<Instant>();
    let (mut client, peer) = connect(move |peer| {
        peer.expect_request();
        peer.expect_marker(constants::MARKER_TYPE_INTERRUPT);
        sent_send.send(Instant::now()).unwrap();
        // the timeout recovery sends its reset without waiting
        peer.expect_marker(constants::MARKER_TYPE_RESET);
        peer.send_marker(constants::MARKER_TYPE_RESET);
        peer.send_status();
        peer.expect_request();
        peer.send_status();
    });
    let _handle = client.break_state().handle();
    client
        .set_call_timeout(Some(Duration::from_millis(350)))
        .unwrap();
    let started = Instant::now();
    let err = ping(&mut client).unwrap_err();
    assert_eq!(err.kind(), &ErrorKind::CallTimeoutExceeded);
    let waited = sent_recv.recv().unwrap() - started;
    assert!(
        waited >= Duration::from_millis(350),
        "fired after {waited:?}"
    );
    assert!(
        waited < Duration::from_millis(800),
        "fired after {waited:?}"
    );
    ping(&mut client).unwrap();
    peer.join().unwrap();
}

#[test]
fn unacknowledged_interrupt_closes_after_one_call_timeout() {
    let (done_send, done_recv) = mpsc::channel::<()>();
    let (mut client, peer) = connect(move |peer| {
        peer.expect_request();
        peer.expect_marker(constants::MARKER_TYPE_INTERRUPT);
        // never answer, like a server busy in DBMS_SESSION.SLEEP()
        done_recv.recv().unwrap();
    });
    let handle = client.break_state().handle();
    client
        .set_call_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    handle.interrupt();
    let started = Instant::now();
    let err = ping(&mut client).unwrap_err();
    let elapsed = started.elapsed();
    done_send.send(()).unwrap();
    assert_eq!(err.kind(), &ErrorKind::UnableToRecover);
    assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
    peer.join().unwrap();
}

#[test]
fn polling_does_not_change_the_reported_call_timeout() {
    let (mut client, peer) = connect(|peer| {
        peer.expect_request();
        peer.send_status();
    });
    let timeout = Some(Duration::from_secs(5));
    client.set_call_timeout(timeout).unwrap();
    assert_eq!(client.transport.get_read_timeout().unwrap(), timeout);
    let _handle = client.break_state().handle();
    ping(&mut client).unwrap();
    assert_eq!(client.get_call_timeout().unwrap(), timeout);
    assert_eq!(
        client.transport.get_read_timeout().unwrap(),
        Some(super::BREAK_POLL_INTERVAL)
    );
    peer.join().unwrap();
}

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
