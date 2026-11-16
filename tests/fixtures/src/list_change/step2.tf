resource "terraform_data" "app" {
  input = {
    label = "frontend"
    rules = [
      { name = "dns", port = 53, access = "Allow" },
      { name = "https", port = 8443, access = "Allow" },
      { name = "ssh", port = 22, access = "Allow" },
    ]
  }
}
