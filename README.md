# metal-nagi

> Autonomous GitHub Engineering Bot in Rust.

**metal-nagi** is a serverless AI engineering agent built in pure Rust, running on AWS Lambda with Neon Postgres for transactional state and concurrency control. It autonomously analyzes issues and pull requests, writes code, reviews implementations, and opens surgical PRs.

---

## Trigger

Tag `@metal-nagi` or `@metal-nagi[bot]` on any GitHub Issue or Pull Request:

```markdown
@metal-nagi fix the connection pool leak in the lambda handler
```

---

## Architecture & Pipelines

metal-nagi operates via two dedicated execution pipelines:

### Pipeline A: Issues &rarr; PR
```
Issue Comment ──> Context Audit ──> Parallel Coders ──> Multi-Lens Review ──> Automated PR
```
Autonomous end-to-end delivery: explores codebase context, generates candidate solutions across parallel coder personas, validates via multi-lens review, and opens a clean PR.

### Pipeline B: PRs &rarr; Fixes
```
PR Comment ──> Targeted Audit ──> Surgical Fix ──> Review & CI Convergence
```
Iterative review and repair: inspects PR diffs, executes targeted fixes, and iterates until checks pass and review standards converge.

---

## Workspace Structure

- [`crates/gh-core`](crates/gh-core): GitHub App authentication (RS256 JWT, short-lived installation access tokens, HMAC-SHA256 webhook signature verification).
- [`crates/nagi-lambda`](crates/nagi-lambda): Serverless entry point on AWS Lambda, Neon Postgres partial unique index concurrency lock, ephemeral shallow clone, and phase router.
- [`infra/aws`](infra/aws): Terraform infrastructure configurations (ARM64 Lambda, Git Layer, API Gateway HTTP endpoints, Secrets Manager).

---

## Status

| Stage | Focus | Status |
| :--- | :--- | :--- |
| **Stage 0** | GitHub App Auth & Database Schema | **Deployed & Live** |
| **Stage 1** | Webhook Ingestion & Concurrency Locking | **Deployed & Live** |
| **Stage 2** | Codebase Ingestion & Context Audit | *In Development* |
| **Stage 3** | Parallel Coder Agents & Multi-Lens Review | *Planned* |
