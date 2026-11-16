#!/usr/bin/env bash
# Runs in the scenario's working directory between the apply and the plan:
# removes both files outside Terraform, which the next refresh reports as drift.
set -euo pipefail
rm notes.txt token.txt
