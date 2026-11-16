terraform {
  required_providers {
    local = {
      source = "hashicorp/local"
    }
  }
}

# Lives in the scenario's throwaway directory; between.sh changes it behind Terraform's back.
resource "local_file" "notes" {
  filename = "${path.module}/notes.txt"
  content  = "written by terraform"
}

resource "local_sensitive_file" "token" {
  filename = "${path.module}/token.txt"
  content  = "drift-CANARY-1"
}
