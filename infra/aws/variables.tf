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
