# TODO

Status of the original spec. Checked items are implemented and tested; the rest is ordered roughly by how much later work depends on it.

## Done (foundation)

- [x] Diff `before`/`after` per `resource_change`
- [x] Match list elements by key: config override, then provider schema, then position; duplicates become `name#2`
- [x] Never print values flagged in `before_sensitive`/`after_sensitive`; flagged blocks report only a count
- [x] Mask copies of flagged values and credential-shaped strings
- [x] Plan JSON kept in memory only; errors never quote the plan
- [x] Classify differences as real or unknown-after-apply
- [x] Moves (`previous_address`), recreations with the forcing path (`replace_paths`), creates, destroys
- [x] Output formats `text` and `json` behind a pluggable `Renderer` registry
- [x] Labels without common words, enforced by a test
- [x] Gates: `--expect no-destroy | only-moves | no-changes`, exit 0 / 2 / 3
- [x] `terraform_data` fixtures, secret-leak test, property tests for list matching
- [x] clippy + fmt + tests in CI; release workflow for static Linux and Windows binaries

### Schema-based key inference
- [x] `--schema <file>` reading `terraform providers schema -json`
- [x] Infer the key of each nested block from attributes named `name`, `key`, `id` (in that order of preference)
- [x] Precedence: config override → schema → position
- [x] Use the schema to tell sets from lists (sets have no order: never report `^v`)
- [x] Use the schema to tell maps from objects, then mask secret-looking map keys in paths and values
- [x] Trimmed real azurerm schema as a fixture

### Classification
- [x] Case-only differences in Azure resource IDs: class `recased`, behind a `provider::Provider` registry
- [x] Flag differences that force a replacement (`forcing`; `Aa!` in text for case-only ones)
- [x] Drift: read `resource_drift` and report it in its own section
- [x] Gates: an update with only case-only differences is undetermined; drift has no effect

## Next

### Classification follow-ups
- [ ] Re-cased IDs inside sets without a key (paired by whole value today: shows as removed + added)
- [ ] Long values are cut at 120 characters, which can hide where two re-cased IDs differ

### JSON-encoded string attributes
- [ ] Detect strings that hold JSON (policies, `jsonencode(...)` bodies) and diff inside them
- [ ] Apply redaction and list matching to the decoded content
- [ ] Ignore formatting-only differences (whitespace, key order)

### Output
- [ ] `markdown` format (for PR comments), with collapsible per-resource sections
- [ ] Verbose mode that lists attributes of created and destroyed resources

### Fix hints
- [ ] Positional list churn → "sort the input by key"
- [ ] Case-only ID differences → "use the data source/resource output instead of a string-built ID"
- [ ] Hints in every format (a field in the report, not renderer logic)

### Fixtures and tests
- [x] Extend `azurerm_synthetic.plan.json` (nested blocks done) with case-only IDs and sensitive attributes
- [x] Fixture with `resource_drift` (`drift`: real, from `hashicorp/local`)
- [ ] Fixture with JSON-encoded string attributes

## Housekeeping

- [ ] Choose a license
- [ ] Push to a remote and confirm both workflows pass (CI and release have not run yet)
- [ ] Verify the MSVC static Windows build on a real runner
- [ ] Composite list keys (`["name", "priority"]`) in `tfdiff.toml`
- [ ] Wildcards in `list_keys` (`"*.security_rule"`)
