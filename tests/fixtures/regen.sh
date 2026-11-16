#!/usr/bin/env bash
# Regenerates tests/fixtures/*.plan.json from the configs in src/.
# Usage: regen.sh [SCENARIO...]   (all scenarios when none is named)
# Each scenario applies step1.tf (if present) to throwaway local state, runs
# between.sh (if present) to change things behind Terraform's back, then plans step2.tf.
# State lives in a temp dir that is removed afterwards; only the plan JSON is kept.
# Scenarios use terraform_data, except `drift`, which downloads hashicorp/local.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TF_IN_AUTOMATION=1 TF_CLI_ARGS="-no-color"

if (($# > 0)); then
  scenarios=("${@/#/$here/src/}")
else
  scenarios=("$here"/src/*/)
fi

for scenario in "${scenarios[@]}"; do
  [[ -f "$scenario/step2.tf" ]] || { echo "no such scenario: $scenario" >&2; exit 1; }
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
    if [[ -x "$scenario/between.sh" ]]; then
      "$scenario/between.sh"
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
