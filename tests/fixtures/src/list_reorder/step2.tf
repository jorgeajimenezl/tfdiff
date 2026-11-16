resource "terraform_data" "app" {
  input = {
    label = "frontend"
    rules = [
      { name = "https", port = 443, access = "Allow" },
      { name = "ssh", port = 22, access = "Allow" },
      { name = "http", port = 80, access = "Allow" },
    ]
  }
}
