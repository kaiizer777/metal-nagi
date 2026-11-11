# metal-nagi

<div align="center">

![Rust](https://img.shields.io/badge/Rust-1.85%2B-black?style=for-the-badge&logo=rust&logoColor=white)
![AWS Lambda](https://img.shields.io/badge/AWS%20Lambda-Serverless-orange?style=for-the-badge&logo=awslambda&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/Neon%20Postgres-Serverless-00e599?style=for-the-badge&logo=postgresql&logoColor=white)
![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)

**Autonomous GitHub Engineering Bot in Rust on AWS Lambda + Neon.**  
Part of the **metal-crew** fleet (Nagi [Coder] · Reviewer · Manager).

</div>

---

## ⚡ Quick Start / Bot Trigger

Mention `@metal-nagi` in an Issue or Pull Request comment:

```markdown
@metal-nagi run
@metal-nagi implement HMAC SHA-256 webhook verification in crates/gh-core
```

- **Pipeline A (Issues)**: Triggered when `issue.pull_request == null`. Gathers context, plans, executes parallel coders, reviews diffs, and opens a PR.
- **Pipeline B (Pull Requests)**: Triggered on active PRs. Pulls PR diff + comments, runs targeted coders, applies review fixes, and pushes directly to the PR branch.
- **Access Control**: Senders are validated against `repo_configs` allowlist. Unauthorized mentions are dropped with HTTP 200.

---

## 🏗️ Architecture in Brief

Event-driven, serverless pipeline with zero idle infrastructure cost:

```
Webhook (GitHub) 
  ──> Gateway (HMAC-SHA256 Verification)
  ──> Neon Postgres (Unique Concurrency Lock: idx_jobs_active_lock)
  ──> Ephemeral Clone (/tmp/repos/{job_id} via Git Lambda Layer)
  ──> Async Agent Loops (InvocationType::Event chaining across Lambdas)
```

1. **HMAC Verification**: Constant-time signature check (`X-Hub-Signature-256`) against App webhook secret.
2. **Concurrency Lock**: Partial unique index `idx_jobs_active_lock` on `jobs(repo, issue_or_pr_number) WHERE status = 'active'` drops duplicate triggers and replies with running status.
3. **Ephemeral Sandbox**: Authenticates via short-lived installation token and performs shallow clone (`--depth 1`) in `/tmp`.
4. **Decoupled Chaining**: Phase transitions write intermediate outputs to Postgres and asynchronously trigger the next Lambda without Step Functions overhead.

---

## 🔄 Dual Pipeline Breakdown

### Pipeline A: Issue ➔ Pull Request
1. **Context Gatherer**: Two-tier inspection (repo tree/symbol index ➔ surgical file read).
2. **Context Confirmer**: Validates sufficiency (max 2 feedback loops); slices task into 1–5 coder tasks.
3. **Parallel Coders & Fan-In**: Up to 5 concurrent worker Lambdas; diffs aggregate via Postgres barrier.
4. **Reviewer & Bounded Fixer**: Multi-file audit with strict 3-cycle cap; enforces clean rebase against upstream base branch.
5. **PR Raiser**: Pushes branch `nagi/issue-{number}` and opens PR with `Fixes #{issue}`.

### Pipeline B: PR Enhancement & Review
- Scoped strictly to PR diff, review comments, and base branch updates.
- Executes targeted single-coder run ➔ bounded 3-cycle review-fix loop ➔ rebase check ➔ direct push to PR branch.

---

## 📦 Workspace Crates

| Path | Crate | Role |
| :--- | :--- | :--- |
| `crates/gh-core` | `gh-core` | GitHub App auth (RS256 JWT, tokens), Octocrab factory, HMAC verification |
| `crates/nagi-lambda` | `nagi-lambda` | Lambda entrypoint, webhook routing, Postgres locking, `/tmp` clone lifecycle |
| `infra/aws` | Terraform | Lambda runtime (`provided.al2023`), Function URLs, IAM policies, Git layer |

---

## 🗺️ Roadmap & Status

| Stage | Milestone | Status |
| :---: | :--- | :---: |
| **0** | Foundation: GitHub App Auth (`gh-core`), Initial Postgres schema, Terraform base | ✅ Merged (#1, #2) |
| **1** | Gateway: Webhook HMAC, Neon concurrency lock, `/tmp` shallow clone | ✅ Merged (#3) |
| **2** | LLM Engine: Prioritized provider fallback, retry/backoff, `needs_human` escalation | 🟡 In Progress |
| **3–7** | Pipeline A: Context Gatherer/Confirmer, Multi-Coder fan-in, Reviewer, PR Raiser | 📋 Planned |
| **8–11**| Pipeline B, SQS DLQ resilience, Postgres dashboard, end-to-end regression | 📋 Planned |

---

## 🛠️ Dev & Deployment Runbook

### Local Development
```bash
docker compose up -d                                         # Start local PostgreSQL 18
sqlx migrate run --source ./migrations                       # Run migrations
cargo test --workspace                                       # Run all unit/integration tests
cargo lambda watch -p nagi-lambda                            # Emulate Lambda runtime locally
```

### Build & Deploy (AWS Lambda)
```powershell
# Compile release binary for Amazon Linux 2023
cargo lambda build --release --target x86_64-unknown-linux-gnu -p nagi-lambda

# Deploy Lambda code via Terraform (Windows DNS fix included)
cd infra/aws
$env:GODEBUG='netdns=cgo'
& 'C:\Terraform\terraform.exe' apply '-target=aws_lambda_function.nagi' -auto-approve -no-color
```

---

## 📄 License

MIT License. See [LICENSE](LICENSE) for details.
