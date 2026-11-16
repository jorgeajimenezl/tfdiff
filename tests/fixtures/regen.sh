#!/usr/bin/env bash
# Regenerates tests/fixtures/*.plan.json from the terraform_data configs in src/.
# Each scenario applies step1.tf (if present) to throwaway local state, then plans step2.tf.
# State lives in a temp dir that is removed afterwards; only the plan JSON is kept.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TF_IN_AUTOMATION=1 TF_CLI_ARGS="-no-color"

for scenario in "$here"/src/*/; do
  name="$(basename "$scenario")"
  work="$(mktemp -d)"
  trap 'rm -rf "$work"' EXIT
  (
    cd "$work"
    if [[ -f "$scenario/step1.tf" ]]; then
      cp "$scenario/step1.tf" main.tf
      terraform init -input=false >/dev/null
      terraform apply -input=false -auto-approve >/dev/null
    fi
    cp "$scenario/step2.tf" main.tf
    terraform init -input=false >/dev/null
    terraform plan -input=false -out=plan.bin >/dev/null
    terraform show -json plan.bin >"$here/$name.plan.json"
  )
  rm -rf "$work"
  trap - EXIT
  echo "wrote $name.plan.json"
done
