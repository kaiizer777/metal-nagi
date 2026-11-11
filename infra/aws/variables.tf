variable "aws_region" {
  type        = string
  default     = "us-east-1"
  description = "AWS region for deployment"
}

variable "lambda_name" {
  type        = string
  default     = "metal-nagi"
  description = "Name of the Lambda function"
}

variable "lambda_zip_path" {
  type        = string
  default     = "../../target/lambda/nagi-lambda/bootstrap.zip"
  description = "Path to the Lambda deployment zip archive"
}

variable "database_url" {
  type        = string
  sensitive   = true
  description = "Postgres connection string for database access"
}

variable "github_app_id" {
  type        = string
  description = "GitHub App ID"
}

variable "github_app_webhook_secret" {
  type        = string
  sensitive   = true
  description = "GitHub App Webhook Secret for HMAC verification"
}

variable "github_app_private_key" {
  type        = string
  sensitive   = true
  description = "GitHub App RSA private key (PEM format)"
}

variable "rust_log" {
  type        = string
  default     = "info,nagi_lambda=debug"
  description = "RUST_LOG logging level filter"
}

variable "opencode_api_key" {
  type        = string
  sensitive   = true
  description = "OpenCode API key for LLM inference"
}

variable "opencode_base_url" {
  type        = string
  default     = "https://opencode.ai/inference/go/openai/v1"
  description = "OpenCode base URL for LLM inference"
}

