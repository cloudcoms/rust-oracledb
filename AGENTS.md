# Fork instructions

Read `FORK.md` for branch policy and verification boundaries.

- All cloudcoms CI, including bootstrap, uses the existing organization
  self-hosted runners with explicit group/architecture/role labels. Never use
  GitHub-hosted runners or silently fall back to them.
- Draft preflight is database-free. Ready-only trusted same-repository PR
  CI may run limited, no-DDL live checks against the existing approved cloudkit
  Oracle through a protected `oracle-live` Environment and restricted account.
  No live tests on forks or Draft PRs; missing credentials fail the Ready gate.
  Do not install Oracle locally, provision disposable database containers, or
  recreate schemas on the shared instance.
- Keep credentials outside the repository and logs. `main` mirrors upstream;
  fork changes belong on the appropriate `dbwork/**` branch.
