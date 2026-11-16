resource "terraform_data" "app" {
  input            = { label = "frontend" }
  triggers_replace = { image = "v1" }
}
