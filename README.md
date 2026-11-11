![Rust](https://img.shields.io/badge/Rust-1.85%2B-black?style=for-the-badge&logo=rust&logoColor=white)
![AWS Lambda](https://img.shields.io/badge/AWS%20Lambda-Serverless-orange?style=for-the-badge&logo=awslambda&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/Neon%20Postgres-Serverless-00e599?style=for-the-badge&logo=postgresql&logoColor=white)
![License](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)

# Metal-Nagi

**Metal-Nagi** is a serverless AI engineering agent built in pure Rust, running on AWS Lambda with Neon Postgres for transactional state and concurrency control. It autonomously analyzes issues and pull requests, writes code, reviews implementations, and opens surgical PRs.

---

## Trigger

Tag `@metal-nagi` or `@metal-nagi[bot]` on any GitHub Issue or Pull Request:

```markdown
@metal-nagi fix the connection pool leak in the lambda handler
```



---

## Workspace Structure

- [`crates/gh-core`](crates/gh-core): GitHub App authentication (RS256 JWT, short-lived installation access tokens, HMAC-SHA256 webhook signature verification).
- [`crates/nagi-lambda`](crates/nagi-lambda): Serverless entry point on AWS Lambda, Neon Postgres partial unique index concurrency lock, ephemeral shallow clone, and phase router.
- [`infra/aws`](infra/aws): Terraform infrastructure configurations (ARM64 Lambda, Git Layer, API Gateway HTTP endpoints, Secrets Manager).
