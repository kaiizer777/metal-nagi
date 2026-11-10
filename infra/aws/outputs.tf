output "function_url" {
  description = "Public Function URL of the metal-nagi Lambda"
  value       = aws_lambda_function_url.nagi_url.function_url
}

output "lambda_arn" {
  description = "ARN of the metal-nagi Lambda function"
  value       = aws_lambda_function.nagi.arn
}
