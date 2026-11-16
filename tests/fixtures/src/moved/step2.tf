resource "terraform_data" "new" {
  input = { label = "frontend" }
}

moved {
  from = terraform_data.old
  to   = terraform_data.new
}
