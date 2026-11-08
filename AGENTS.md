# Fork instructions

Read `FORK.md` for branch policy and verification boundaries.

- All cloudcoms CI, including bootstrap, uses the existing organization
  self-hosted runners with explicit group/architecture/role labels. Never use
  GitHub-hosted runners or silently fall back to them.
- Normal CI is database-free. Manual live checks reuse cloudkit's existing
  Oracle at `172.30.1.50:1521/FREEPDB1` with an approved existing test account.
  Do not install Oracle locally, provision disposable database containers, or
  recreate schemas on the shared instance.
- Keep credentials outside the repository and logs. `main` mirrors upstream;
  fork changes belong on the appropriate `dbwork/**` branch.
