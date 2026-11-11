# metal-nagi

<div align="center">

![Rust](https://img.shields.io/badge/Rust-1.85%2B-black?style=for-the-badge&logo=rust&logoColor=white)
![AWS Lambda](https://img.shields.io/badge/AWS%20Lambda-Serverless-orange?style=for-the-badge&logo=awslambda&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/Neon%20Postgres-Serverless-00e599?style=for-the-badge&logo=postgresql&logoColor=white)
![Architecture](https://img.shields.io/badge/Architecture-Event--Driven-blue?style=for-the-badge)
![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)

**Autonomous GitHub Engineering Bot built in high-performance Rust, executing on AWS Lambda with Neon Serverless Postgres.**

*Part of the **metal-crew** autonomous engineering fleet (Nagi [Coder] · Reviewer · Manager).*

</div>

---

## ⚡ Vision & Overview

`metal-nagi` is an autonomous GitHub engineering agent built from the ground up for extreme speed, strict resource isolation, and zero idle cost. It listens to repository events, verifies authenticity cryptographically, orchestrates parallel multi-phase coder subagents in isolated environments, executes automated review-and-fix iterations, and opens pull requests without human intervention unless an unresolvable conflict occurs.

- **Zero Idle Infrastructure**: Purely event-driven execution on AWS Lambda custom runtime (`provided.al2023`) powered by Neon Serverless Postgres.
- **Multi-Agent Orchestration**: Hand-rolled asynchronous Lambda chaining with Postgres persistence — no Step Functions overhead or vendor lock-in.
- **Strict Scope Boundaries**: Implements code solutions and authors Pull Requests. It never self-merges and never acts as the final merge review gate.
- **Resilient Engineering**: Hardened concurrency deduplication, bounded review-fix loops, multi-tiered LLM fallback, and clean `needs_human` escalation.

---

## 🎯 Bot Trigger

The bot is activated strictly by mentioning `@metal-nagi` in an issue or pull request comment:

```markdown
@metal-nagi please implement support for HMAC SHA-256 webhook verification in crates/gh-core
```

### Trigger Rules
- **Regex Boundary**: Evaluated with case-insensitive word boundaries (`\b@metal-nagi\b`).
- **Pipeline Routing**:
  - Mentioned on a **GitHub Issue** (`issue.pull_request == null`) $\rightarrow$ Dispatches **Pipeline A** (Issue to PR).
  - Mentioned on an **Existing Pull Request** (`issue.pull_request != null`) $\rightarrow$ Dispatches **Pipeline B** (PR Enhancement & Review).
- **Access Control**: Sender validation against repo-level allowlists in `repo_configs` (defaults to repo owner and authorized contributors). Unauthorized mentions are cleanly ignored (HTTP 200).

---

## 🏗️ Architecture & Core Webhook Flow

```mermaid
sequenceDiagram
    autonumber
    actor Dev as Developer / GitHub
    participant GW as Lambda Webhook Gateway
    participant DB as Neon Postgres
    participant S3 as Ephemeral /tmp
    participant Pipe as Pipeline Orchestrator

    Dev->>GW: POST / (issue_comment webhook)
    GW->>GW: Verify HMAC-SHA256 (X-Hub-Signature-256)
    GW->>GW: Match @metal-nagi & Verify Sender
    GW->>DB: Check & Acquire Job Lock (idx_jobs_active_lock)
    alt Active Job Already In Progress
        GW->>Dev: Post status comment ("Job #{id} running...")
        GW-->>Dev: HTTP 200 OK (Drop duplicate)
    else Lock Acquired
        GW->>DB: INSERT INTO jobs (status='active', phase=1)
        GW->>S3: Ephemeral shallow clone (/tmp/repos/{job_id})
        GW-)Pipe: Async Invoke Phase 1 (InvocationType: Event)
        GW-->>Dev: HTTP 200 OK
    end
```

### 1. Webhook Verification & Gateway
Every incoming payload is validated against `GITHUB_APP_WEBHOOK_SECRET` via constant-time HMAC-SHA256 comparison (`gh_core::webhook::verify_webhook_signature`). Invalid signatures return HTTP 401 immediately.

### 2. Concurrency & Dedup Lock
To prevent duplicate execution and race conditions from rapid double-comments, `metal-nagi` leverages PostgreSQL partial unique indexing:

```sql
CREATE UNIQUE INDEX idx_jobs_active_lock 
ON jobs (repo, issue_or_pr_number) 
WHERE status = 'active';
```

- When a job is active, any secondary mention triggers an immediate informational response comment stating the active job ID and progress.
- No second pipeline is dispatched. The gateway returns HTTP 200 to prevent GitHub webhook redelivery loops.

### 3. Ephemeral Shallow Clone
Before code execution, the worker fetches a short-lived GitHub App Installation Token and executes a shallow clone (`--depth 1`) using the pre-compiled Lambda Git layer (`/opt/bin/git`) into isolated job storage at `/tmp/repos/{job_id}`.

### 4. Asynchronous Pipeline Chaining
Phases do not block Lambda invocations. When a phase completes:
1. Intermediate results and diffs are written to `job_phases` or `coder_outputs` in Postgres.
2. The next phase Lambda is invoked asynchronously via AWS SDK (`InvocationType::Event`), passing only `{ "job_id": "<uuid>", "phase": <next> }`.
3. The subsequent Lambda spins up, pulls its context from Postgres, and continues execution.

---

## 🔄 Dual Pipeline Breakdown

### Pipeline A: Issue $\rightarrow$ PR Workflow

Used for new feature development and bug fixes filed in GitHub issues.

```mermaid
flowchart TD
    Start([Issue Comment Trigger]) --> G[Phase 1: Context Gatherer]
    G -->|File Tree + Symbol Outlines| C[Phase 2: Context Confirmer]
    C -->|More context needed| G
    C -->|Decompose task 1..5 coders| D{Phase 3: Dispatcher}
    
    D --> Cod1[Coder 1]
    D --> Cod2[Coder 2]
    D --> CodN[Coder N (max 5)]
    
    Cod1 --> F[Fan-In Synchronizer]
    Cod2 --> F
    CodN --> F
    
    F -->|All Coders Complete| R[Phase 4: Reviewer]
    R -->|Red Verdict & Cycle < 3| Fix[Fixer Subagent]
    Fix --> R
    
    R -->|Green or Cycle Cap 3| Reb{Clean Git Rebase?}
    Reb -->|Conflict| Esc[🚨 Escalation: needs_human]
    Reb -->|Clean| P[Phase 5: PR Raiser]
    P --> PR[(Open GitHub Pull Request)]
```

1. **Phase 1: Context Gatherer**  
   Employs a two-tier retrieval engine:
   - *Tier 1 (Lightweight Structure)*: Reads repository file tree and symbol index.
   - *Tier 2 (Surgical Content)*: Selectively reads full file contents only for candidate files identified by issue heuristics (never dumps entire repos).
2. **Phase 2: Context Confirmer & Splitter**  
   Evaluates gathered context for completeness. Bounded loop-back (max 2) to Phase 1 if files are missing. Divides the task across 1 to 5 parallel coder slices.
3. **Phase 3: Parallel Coders & Fan-In**  
   Dispatches up to 5 concurrent Coder Lambdas. Each coder works in an isolated sandbox, writes patches to `coder_outputs`, and synchronizes via a Postgres fan-in barrier.
4. **Phase 4: Reviewer & Fixer Loop**  
   Combines diffs and reviews against issue requirements:
   - Green verdict $\rightarrow$ Proceed to PR Raiser.
   - Red verdict $\rightarrow$ Invoke Fixer subagent with actionable review comments.
   - **Fixer Cap**: Capped at 3 cycles (Cycle 1: full audit; Cycles 2–3: targeted audit of flagged items).
   - Pre-advance rebase check against upstream base branch.
5. **Phase 5: PR Raiser**  
   Creates branch `nagi/issue-{number}`, commits changes, and submits PR linked to `Fixes #{issue}`. If the 3-cycle cap was exhausted without full green, it opens the PR with a prominent degradation banner detailing unresolved feedback.

---

### Pipeline B: PR Enhancement & Review Workflow

Triggered when `@metal-nagi` is tagged on an active Pull Request to address comments, apply refactors, or fix CI failures.

```mermaid
flowchart LR
    Start([PR Comment Trigger]) --> G[Phase 1: PR Context Gatherer]
    G --> C[Phase 2: Targeted Coder]
    C --> R[Phase 3: Reviewer & Fixer Loop]
    R -->|Max 3 Cycles| Reb{Rebase Check}
    Reb -->|Conflict| Esc[🚨 Escalation: needs_human]
    Reb -->|Clean| Push[Phase 4: Push to PR Branch]
```

- Scoped to existing PR diff, discussion context, and instructions.
- Single coder iteration followed by the bounded 3-cycle review-fix loop.
- Automatically rebases and pushes new commits directly to the active PR branch.

---

## 🛡️ Hardening & Fault Tolerance

| Threat / Event | Mitigation Strategy | Failure Result |
| :--- | :--- | :--- |
| **Duplicate Webhooks** | Postgres unique index `idx_jobs_active_lock` on `(repo, issue_or_pr_number)` WHERE `status = 'active'`. | HTTP 200 returned, status comment posted on issue, zero duplicate jobs. |
| **Reviewer-Fixer Cycles** | Strict 3-cycle limit. Attempts 2–3 focus strictly on previously flagged findings. | Graceful degradation: PR is raised with `⚠️ Unresolved review feedback` banner. Never stalls. |
| **Git Merge Conflicts** | Upstream base rebase check before PR submission or branch push. | Clean halt: Job marked `needs_human`, conflict diff posted in comment. No hallucinated resolutions. |
| **LLM Provider Outage** | Tiered hierarchical fallback: Model $M_1 \rightarrow M_2 \rightarrow M_k$, then Provider $P_1 \rightarrow P_2$. 3 retries per model with 10s backoff for hard errors (5xx/timeout). | Total chain exhaustion marks job `needs_human` and alerts repo owner. |
| **Lambda Infra Crash** | 1 automatic retry on transient error with exponential backoff; AWS SQS Dead-Letter Queue (DLQ). | DLQ consumer transitions job to `needs_human` and comments on issue. |

---

## 📦 Workspace Crates & Structure

```
metal-nagi/
├── Cargo.toml                    # Workspace manifest (LTO, strip, codegen-units=1)
├── docker-compose.yml            # Local PostgreSQL 18 dev environment
├── migrations/                   # SQLx migration files (Jobs, Phases, Providers)
│   ├── 20261110000001_initial_schema.sql
│   └── 20261110000001_initial_schema.down.sql
├── crates/
│   ├── gh-core/                  # Shared GitHub App foundation crate
│   │   ├── src/
│   │   │   ├── client.rs         # Authenticated Octocrab & reqwest client factories
│   │   │   ├── config.rs         # GitHub App credentials & key validation
│   │   │   ├── jwt.rs            # RS256 JWT generation (10m exp, clock-skew leeway)
│   │   │   ├── token.rs          # GitHub App Installation token exchange
│   │   │   └── webhook.rs        # Constant-time HMAC-SHA256 signature verification
│   │   └── tests/                # Live and mocked unit & integration tests
│   └── nagi-lambda/              # AWS Lambda event ingestion & orchestration runtime
│       ├── src/
│       │   ├── db.rs             # Concurrency locks, state queries, comment dispatch
│       │   ├── git.rs            # Ephemeral /tmp shallow clone & cleanup
│       │   ├── webhook.rs        # Header parsing, mention check, event types
│       │   └── main.rs           # Lambda runtime bootstrap and entrypoint
│       └── Cargo.toml
└── infra/
    └── aws/                      # Production Terraform deployment definitions
        ├── main.tf               # AWS provider & backend setup
        ├── lambda.tf             # Function URL, Git Layer, timeout & memory config
        ├── iam.tf                # Least-privilege IAM execution roles & logging
        ├── variables.tf          # Parameter declarations
        └── outputs.tf            # Exported Function URLs and ARNs
```

---

## 🗺️ Project Roadmap & Stage Tracking

- [x] **Stage 0: Foundation & GitHub Auth Engine** *(Merged in PR #1 & #2)*
  - `gh-core` crate: RS256 JWT signing, token exchange, HMAC verification.
  - Initial Postgres schema with active-job partial unique index lock.
  - Terraform AWS Lambda skeleton with custom runtime (`provided.al2023`), Function URL, and Git layer.
- [x] **Stage 1: Webhook Entry Point & Concurrency Lock** *(Merged in PR #3)*
  - Webhook signature validation and `@metal-nagi` mention filter.
  - Concurrency lock enforcement in Neon Postgres with status response comments.
  - Ephemeral shallow clone to `/tmp/repos/{job_id}` using temporary installation tokens.
- [ ] **Stage 2: LLM Provider Fallback Engine** *(In Progress)*
  - Implement `crates/llm-client` with prioritized provider and model escalation loops.
  - 3 attempts with 10s backoff for hard errors; `needs_human` escalation on total exhaustion.
- [ ] **Stage 3: Pipeline A — Context Gatherer**
  - Two-tier repo inspection (file tree outline $\rightarrow$ surgical content fetch).
  - Context persistence in `job_phases` and async Phase 2 invocation.
- [ ] **Stage 4: Pipeline A — Context Confirmer & Splitter**
  - Completeness verification and task decomposition into 1–5 coder slices.
- [ ] **Stage 5: Pipeline A — Parallel Coders & Fan-In Synchronizer**
  - Concurrent Coder Lambdas writing to `coder_outputs` with Postgres count barrier.
- [ ] **Stage 6: Pipeline A — Reviewer & Bounded Fixer Loop**
  - Green/Red diff evaluation, 3-cycle cap enforcement, upstream rebase validation.
- [ ] **Stage 7: Pipeline A — PR Raiser**
  - Branch creation, commit authoring, and PR creation with issue linkage.
- [ ] **Stage 8: Pipeline B — PR Enhancement Loop**
  - Context gathering and single-coder execution on existing PR branches.
- [ ] **Stage 9: Dead-Letter Queue (DLQ) & Resilience**
  - AWS SQS DLQ attachment with automated failure comment notifications.
- [ ] **Stage 10: PostgreSQL Configuration Dashboard**
  - Centralized repo allowlists and model priority toggles (zero DynamoDB).
- [ ] **Stage 11: End-to-End Regression & Verification**
  - Live verification across issue implementation, multi-coder features, and PR updates.

---

## 🛠️ Local Development & Testing

### Prerequisites
- **Rust Toolchain**: 1.85+ (`rustup default stable`)
- **Cargo Lambda**: `cargo install cargo-lambda`
- **Docker**: For running the local PostgreSQL container
- **Terraform**: `C:\Terraform\terraform.exe` (v1.5+)
- **Smee Client** (Optional): For forwarding webhooks locally (`npm install --global smee-client`)

### 1. Database Setup
Launch local PostgreSQL using the bundled Docker Compose definition:

```bash
docker compose up -d
```

Apply migrations using `sqlx-cli`:

```bash
cargo install sqlx-cli --no-default-features --features postgres
sqlx migrate run --source ./migrations
```

### 2. Running Workspace Tests
Run unit and integration tests across all workspace crates:

```bash
cargo test --workspace
```

For live GitHub token exchange tests, set the required environment variables:
```bash
$env:GITHUB_APP_ID="5699171"
$env:GITHUB_APP_INSTALLATION_ID="180574327"
$env:GITHUB_APP_PRIVATE_KEY_PEM="-----BEGIN RSA PRIVATE KEY-----..."
cargo test -p gh-core --test live_token_exchange -- --ignored
```

### 3. Local Lambda Execution
Emulate the AWS Lambda environment locally:

```bash
cargo lambda watch -p nagi-lambda
```

Forward GitHub webhooks using Smee:

```bash
smee -u https://smee.io/wPxaoo00O2NXMeRx --target http://localhost:9000
```

---

## 🚀 Deployment Runbook

Deployments manage the AWS Lambda binary, public Function URL, and IAM configurations via Terraform.

### 1. Build Custom Linux Runtime Binary
Compile the binary targeted for the Amazon Linux execution environment:

```powershell
cargo lambda build --release --target x86_64-unknown-linux-gnu -p nagi-lambda
```

The compiled `bootstrap` binary is output to `target/lambda/nagi-lambda/bootstrap`.

### 2. Terraform Deployment Standard

> [!IMPORTANT]
> Always execute Terraform with `$env:GODEBUG='netdns=cgo'` on Windows to bypass Go DNS resolver timeouts when contacting AWS endpoints.

```powershell
# Navigate to Terraform infrastructure directory
cd infra/aws

# Mandatory DNS fix for Windows host
$env:GODEBUG='netdns=cgo'

# Review planned changes
& 'C:\Terraform\terraform.exe' plan -no-color

# Code-only apply (preserves Function URL and Secrets)
& 'C:\Terraform\terraform.exe' apply '-target=aws_lambda_function.nagi' -auto-approve -no-color
```

> [!TIP]
> Never run `terraform taint` on the Lambda resource, as this will destroy and regenerate the public Function URL, invalidating the registered GitHub App Webhook configuration.

---

## 📄 License

This project is licensed under the [MIT License](LICENSE).
