resource "terraform_data" "app" {
  input = {
    rules = [
      { name = "db", port = 5432 },
      { name = "web", port = 80 },
      { name = "web", port = 8443 },
    ]
  }
}
