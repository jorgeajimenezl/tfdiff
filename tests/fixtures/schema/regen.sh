#!/usr/bin/env bash
# Regenerates tests/fixtures/azurerm.schema.json: the real azurerm provider schema,
# trimmed to a few resource types and stripped of descriptions.
# Downloads the provider (no cloud credentials needed); needs terraform and jq.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
out="$here/../azurerm.schema.json"
export TF_IN_AUTOMATION=1 TF_CLI_ARGS="-no-color"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
cd "$work"

cat >main.tf <<'EOF'
terraform {
  required_providers {
    azurerm = {
      source  = "hashicorp/azurerm"
      version = "4.14.0"
    }
  }
}
EOF

terraform init -input=false >/dev/null
terraform providers schema -json | jq --sort-keys '
  def keep: ["azurerm_network_security_group", "azurerm_virtual_network",
             "azurerm_key_vault", "azurerm_application_gateway"];
  {
    format_version,
    provider_schemas: (.provider_schemas | map_values({
      resource_schemas: (.resource_schemas | with_entries(select(.key as $k | keep | index($k))))
    }))
  }
  | walk(if type == "object" then del(.description, .description_kind) else . end)
' >"$out"
echo "wrote $(basename "$out")"
