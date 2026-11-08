# cloudcoms/rust-oracledb (fork)

This fork exists for one consumer, [cloudcoms/dbwork], until upstream
[oracle/rust-oracledb] ships an equivalent feature. It is not a general
distribution of the driver.

## What it adds

On top of an upstream release tag, as small commits that each stand alone:

1. The upstream transport connect timeout (included in beta.5; no fork cherry-pick).
2. A fix for a partially received packet being dropped after a read timeout.
3. `Config::set_connect_timeout()`: one deadline for TCP, TLS, Oracle Net and
   authentication (`ErrorKind::ConnectTimeoutExceeded`). Connect-time reads
   and writes are bounded, including TCPS handshake; descriptor-specific
   transport timeouts still take precedence. Retry delay is capped by the
   remaining overall deadline and never runs after the final attempt.
4. `Connection::break_handle()`: interrupts a call from another thread. The
   handle only records the request; the thread waiting for the database sends
   the interrupt itself (TCP and TLS alike), see `src/break_handle.rs`.
5. Draft database-free preflight and Ready-only approved Oracle integration
   tests on the organization's existing self-hosted Linux x64 CI runners.
   Tests reuse the existing cloudkit Oracle; CI does not create an Oracle
   database, schema, or container.
6. A fix for the reset after an interrupt: the database answers an in-band
   interrupt with a reset marker straight away, and that marker must count,
   or the reset discards the response (ORA-01013) and waits in vain.
7. Fetched NCHAR/NVARCHAR2 values (and NCLOB fetched as LONG) from a
   database whose national character set is `UTF8` (id 871, CESU-8) rather
   than `AL16UTF16`. The database sends this data in its national character
   set, and upstream always decoded it as UTF-16, which turned Korean and
   other non-ASCII text into unrelated characters. Reading NCLOB locators
   (`src/lob.rs`) and NCHAR binds are unchanged and not verified with `UTF8`.

Breaks are always sent in band, as the upstream driver and python-oracledb
on Windows do. python-oracledb thin on other platforms sends them out of
band (TCP urgent data) when the server offers it; neither tested database
(23.26 Free, 21c XE) offered it, so that path is not carried here.

Design discussion: oracle/rust-oracledb#24.

## Branches and alignment with upstream

The fork tracks `Cargo.lock` for reproducible DevRelay checks. Run
`cargo fetch --locked` during dependency preparation, then
`cargo clippy --locked --offline --all-targets -- -D warnings` and
`cargo test --locked --offline --lib` without database credentials.
The optional Arrow feature is checked with
`cargo check --locked --offline --all-targets --features arrow`.
`.github/workflows/preflight.yml` runs the same checks on same-repository pull
requests using group `ci` with explicit Linux x64/build labels. GitHub-hosted
runners are not used, including for bootstrap. It reuses Rust 1.99.1 and removes
its per-job Cargo registry (512 MiB cap) and build outputs (2 GiB cap) on exit;
the job owns cleanup and does not upload artifacts or persist caches.

Ready-only CI (trusted same-repository PRs, no forks) uses a protected
`oracle-live` GitHub Environment containing `RSO_TEST_CONNECT_STRING`,
`RSO_TEST_MAIN_USER` and `RSO_TEST_MAIN_PASSWORD` as secrets. The existing
approved limited account must access the already-running cloudkit Oracle;
the connection string is kept out of Git. Configure the Environment's
required reviewers and branch restrictions before placing credentials there.
Missing credentials fail the Ready CI gate closed. No live tests run on Draft PRs.
Do not install Oracle locally, start database containers, or invoke
`create_test_schema`/`drop_test_schema` against the shared database. The full
upstream integration suite still requires a separately approved test schema.

The mandatory limited-account Ready tests `test_3601..3604` use a
test-local fixture without upstream's eager administrator connection. They
run SELECT queries, call timeout, and DBMS_SESSION.SLEEP without DDL.
The optional `test_3600` additionally requires `RSO_TEST_ADMIN_USER` and
`RSO_TEST_ADMIN_PASSWORD` secrets for an approved account with `v$session`
read access. This test is explicitly reported skipped if they are absent.
CI success without it does not establish `v$session` parity. Confirm national
charset and TCPS coverage separately; never infer CESU-8 validation from
break tests.

- `main` mirrors upstream `main` and is never committed to.
- `dbwork/break-<upstream version>` holds the patches on top of the upstream
  tag of that version, e.g. `dbwork/break-beta5` on `v26.0.0-beta.5`. The
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

## Beta.5 sync (2026-11-08)

- Upstream base: `v26.0.0-beta.5` (`3aaa64dd3e44683ca831338991a497bb85fe0e49`).
- Preserves read-timeout packet buffering, overall connect deadline, cross-thread
  interrupt + reset recovery, and CESU-8 NCHAR decoding.
- Removes the upstream transport timeout cherry-pick already shipped in beta.5.
- Fork preflight is database-free on an existing organization self-hosted runner.
- Ready CI runs approved limited-account break tests on the existing Oracle;
  `test_3600` requires separately approved `v$session` access. Charset/TCPS
  coverage is separately recorded, never inferred from this gate.
