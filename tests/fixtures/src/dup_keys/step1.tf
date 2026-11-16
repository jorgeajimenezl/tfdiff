resource "terraform_data" "app" {
  input = {
    rules = [
      { name = "web", port = 80 },
      { name = "web", port = 443 },
      { name = "db", port = 5432 },
    ]
  }
}
