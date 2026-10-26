//-----------------------------------------------------------------------------
// Copyright (c) 2026, contributors to the cloudcoms/rust-oracledb fork.
//
// This file is dual-licensed under the same terms as the rest of this crate:
// the Universal Permissive License (UPL) 1.0 as shown at
// https://oss.oracle.com/licenses/upl and Apache License 2.0 as shown at
// http://www.apache.org/licenses/LICENSE-2.0. You may choose either license.
//-----------------------------------------------------------------------------

//-----------------------------------------------------------------------------
// test_3600_break.rs
//
// Tests for interrupting calls with Connection::break_handle() against a real
// database. Elapsed times are printed so a run doubles as latency evidence.
//-----------------------------------------------------------------------------

mod common;

use std::thread;
use std::time::{Duration, Instant};

use rstest::*;

use common::conn;

/// A query that keeps the server busy (and checking for interrupts) for far
/// longer than any of these tests run.
const LONG_QUERY: &str =
    "select count(*) from all_objects a, all_objects b, all_objects c";

/// Returns the error of a call that is expected to fail.
fn expect_err<T>(result: Result<T, oracledb::Error>) -> oracledb::Error {
    match result {
        Ok(_) => panic!("the call was expected to fail"),
        Err(err) => err,
    }
}

fn db_error_code(err: &oracledb::Error) -> Option<usize> {
    match err.kind() {
        oracledb::ErrorKind::DbError(db_error) => Some(db_error.code()),
        _ => None,
    }
}

fn interrupt_after(handle: oracledb::BreakHandle, delay: Duration) {
    thread::spawn(move || {
        thread::sleep(delay);
        handle.interrupt();
    });
}

fn assert_usable(conn: &oracledb::Connection) {
    let row = conn.query_row("select 42 from dual", &[]).unwrap();
    let value: i64 = row.get(0).unwrap();
    assert_eq!(value, 42);
}

/// Returns the status of the sessions tagged with the client identifier, as
/// seen by the administrative user.
fn session_status(client_identifier: &str) -> Vec<String> {
    let admin_config = oracledb::get_test_config().admin_config.clone();
    let admin = oracledb::connect(admin_config).unwrap();
    let rows = admin
        .query(
            "select status from v$session where client_identifier = :1",
            &[&client_identifier],
        )
        .unwrap();
    rows.map(|row| row.unwrap().get::<String>(0).unwrap())
        .collect()
}

#[rstest]
fn test_3600_interrupt_running_query(conn: oracledb::Connection) {
    let tag = format!("rso-break-{}", std::process::id());
    conn.set_client_identifier(&tag).unwrap();
    let handle = conn.break_handle().unwrap();
    // bounds the test if the interrupt does not reach the server
    conn.set_call_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    interrupt_after(handle, Duration::from_secs(1));
    let started = Instant::now();
    let err = expect_err(conn.query_row(LONG_QUERY, &[]));
    let elapsed = started.elapsed();
    println!("interrupted running query after {elapsed:?}: {err}");
    assert_eq!(db_error_code(&err), Some(1013), "{err}");
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
    // the server stopped working on the call and the session is reusable
    assert_eq!(session_status(&tag), ["INACTIVE"]);
    assert_usable(&conn);
}

#[rstest]
fn test_3601_interrupt_before_quick_call(conn: oracledb::Connection) {
    let handle = conn.break_handle().unwrap();
    for _ in 0..5 {
        handle.interrupt();
        // the call may complete before the server acts on the interrupt;
        // either way the session must be clean for the next call
        match conn.query_row("select 1 from dual", &[]) {
            Ok(_) => println!("quick call completed before the interrupt"),
            Err(err) => {
                println!("quick call interrupted: {err}");
                assert_eq!(db_error_code(&err), Some(1013), "{err}");
            }
        }
        assert_usable(&conn);
    }
}

#[rstest]
fn test_3602_call_timeout_with_break_handle(conn: oracledb::Connection) {
    let _handle = conn.break_handle().unwrap();
    conn.set_call_timeout(Some(Duration::from_secs(1))).unwrap();
    let started = Instant::now();
    let err = expect_err(conn.query_row(LONG_QUERY, &[]));
    let elapsed = started.elapsed();
    println!("call timeout fired after {elapsed:?}: {err}");
    assert_eq!(err.kind(), &oracledb::ErrorKind::CallTimeoutExceeded);
    assert!(elapsed >= Duration::from_secs(1), "took {elapsed:?}");
    assert!(elapsed < Duration::from_secs(4), "took {elapsed:?}");
    assert_eq!(conn.call_timeout().unwrap(), Some(Duration::from_secs(1)));
    assert_usable(&conn);
}

#[rstest]
fn test_3603_interrupt_ignored_by_sleep_is_bounded(
    conn: oracledb::Connection,
) {
    // DBMS_SESSION.SLEEP() acknowledges an interrupt only when it ends; with
    // a call timeout the client gives up after one more call timeout.
    let handle = conn.break_handle().unwrap();
    conn.set_call_timeout(Some(Duration::from_secs(2))).unwrap();
    interrupt_after(handle, Duration::from_millis(500));
    let started = Instant::now();
    let result = conn.execute("begin dbms_session.sleep(20); end;", &[]);
    let elapsed = started.elapsed();
    let err = expect_err(result);
    println!("sleep with interrupt ended after {elapsed:?}: {err}");
    assert_eq!(err.kind(), &oracledb::ErrorKind::UnableToRecover);
    assert!(elapsed < Duration::from_secs(6), "took {elapsed:?}");
}

#[rstest]
fn test_3604_break_handle_without_interrupt_is_transparent(
    conn: oracledb::Connection,
) {
    let _handle = conn.break_handle().unwrap();
    let rows = conn
        .query("select level from dual connect by level <= 5000", &[])
        .unwrap();
    assert_eq!(rows.count(), 5000);
    assert_usable(&conn);
}
