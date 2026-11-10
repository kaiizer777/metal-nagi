resource "aws_lambda_function" "nagi" {
  function_name = var.lambda_name
  role          = aws_iam_role.lambda_role.arn
  handler       = "bootstrap"
  runtime       = "provided.al2023"
  architectures = ["x86_64"]

  filename         = var.lambda_zip_path
  source_code_hash = filebase64sha256(var.lambda_zip_path)

  layers = ["arn:aws:lambda:us-east-1:553035198032:layer:git-lambda2:8"]

  timeout     = 30
  memory_size = 256
}

resource "aws_lambda_function_url" "nagi_url" {
  function_name      = aws_lambda_function.nagi.function_name
  authorization_type = "NONE"

  cors {
    allow_origins = ["*"]
    allow_methods = ["*"]
  }
}

resource "aws_lambda_permission" "allow_public_function_url" {
  statement_id           = "FunctionURLAllowPublicAccess"
  action                 = "lambda:InvokeFunctionUrl"
  function_name          = aws_lambda_function.nagi.function_name
  principal              = "*"
  function_url_auth_type = "NONE"
}
