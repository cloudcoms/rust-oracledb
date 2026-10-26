# cloudcoms/rust-oracledb (fork)

This fork exists for one consumer, [cloudcoms/dbwork], until upstream
[oracle/rust-oracledb] ships an equivalent feature. It is not a general
distribution of the driver.

## What it adds

On top of an upstream release tag, as small commits that each stand alone:

1. The upstream transport connect timeout (cherry-pick of upstream e9cfda6).
2. A fix for a partially received packet being dropped after a read timeout.
3. `Config::set_connect_timeout()`: one deadline for TCP, TLS, Oracle Net and
   authentication (`ErrorKind::ConnectTimeoutExceeded`).
4. `Connection::break_handle()`: interrupts a call from another thread. The
   handle only records the request; the thread waiting for the database sends
   the interrupt itself (TCP and TLS alike), see `src/break_handle.rs`.
5. `.github/workflows/break-live.yml`: unit tests plus the live tests
   (`tests/test_3600_break.rs`, and the upstream suite on 23.26) against
   throwaway Oracle Database 23.26 Free and 21c XE containers on a
   GitHub-hosted runner.
6. Out-of-band breaks (TCP urgent data) when the server offers them, on
   non-Windows clients over plain TCP, as python-oracledb thin does;
   otherwise the in-band interrupt marker.
7. A fix for the reset after an interrupt: the database answers an in-band
   interrupt with a reset marker straight away, and that marker must count,
   or the reset discards the response (ORA-01013) and waits in vain.

Design discussion: oracle/rust-oracledb#24.

## Branches and alignment with upstream

- `main` mirrors upstream `main` and is never committed to.
- `dbwork/break-<upstream version>` holds the patches on top of the upstream
  tag of that version, e.g. `dbwork/break-beta4` on `v26.0.0-beta.4`. The
  crate version is unchanged, so `[patch.crates-io]` replaces exactly the
  version the consumer pins.
- Consumers pin a full commit SHA, and every pinned SHA is also tagged
  (`dbwork-<upstream version>-break.<n>`) so it stays reachable.
- When upstream releases a new version: create a new branch from the new tag,
  cherry-pick the patches (dropping any that upstream now covers), let the CI
  pass, tag it, then update the consumer's pin in a reviewed PR. Old branches
  and tags are kept for rollback.
- When upstream ships an equivalent break/connect-timeout API, the consumer
  returns to the crates.io release and this fork is archived.

Contributions upstream require the Oracle Contributor Agreement; nothing from
this fork is proposed upstream as code until that is settled.

[cloudcoms/dbwork]: https://github.com/cloudcoms/dbwork
[oracle/rust-oracledb]: https://github.com/oracle/rust-oracledb
