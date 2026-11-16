# tfdiff

Turns `terraform show -json <plan>` into a short, reviewable summary of what really changes.

```console
$ terraform plan -out plan.bin
$ terraform show -json plan.bin | tfdiff
tfdiff: +0 -0 -/+0 ~1 x0 =>0 ==0
~ terraform_data.app
    ~ input.rules[https].port: 443 -> 8443
    - input.rules[http]: {access="Allow", name="http", port=80}
    + input.rules[dns]: {access="Allow", name="dns", port=53}
    ^v input.rules
    ? output
```

Terraform's own diff for that plan marks every rule as changed, because it compares lists by position. tfdiff pairs the rules by `name` here because it was told to: by a provider schema (`--schema`) or by `tfdiff.toml`. See [List matching](#list-matching).

## Usage

```
tfdiff [PLAN] [--format text|json] [--config FILE] [--schema FILE] [--expect no-destroy|only-moves|no-changes]
```

`PLAN` is a plan JSON file; standard input is read when it is omitted or `-`. The plan is only held in memory.

### Reading the text format

The text format uses sigils rather than words, because CI systems mask secret values wherever they appear in a log, and a secret that is literally `replace` would mangle the report.

| Resource line | Meaning | Attribute line | Meaning |
|---|---|---|---|
| `+` | created | `+` | added |
| `-` | destroyed | `-` | removed |
| `-/+` | destroyed and created again | `~` | changed |
| `~` | updated in place | `^v` | same list elements, different order |
| `x` | removed from state, not destroyed | `?` | known only after apply |
| `=>` | moved | `!` | path that forces the recreation |
| | | `<` | previous address |

The first line counts resources per sigil; `==` counts untouched resources. Moved resources are counted under `=>` and also under what else happens to them.

The JSON format (`schema_version: 1`) carries the same information with uncommon words as labels: `provision`, `teardown`, `respawn`, `mutate`, `detach`, `rehome`, `steady` for resources; `gained`, `dropped`, `altered`, `shuffled` for attributes; `effective` or `undetermined` as the class of each difference.

### List matching

The plan does not say what identifies a list element, so tfdiff compares lists by position unless it is told better. For each list, the first of these that applies wins:

1. **`tfdiff.toml`** names the key attribute of that list.
2. **The provider schema** (`--schema`) declares the list. Elements with an attribute `name`, `key` or `id` (first one declared as a string or number) are paired by it. Sets are paired by key when they have one and by whole value otherwise, and their order is never reported.
3. **Position.**

Duplicate keys become `name#2`, `name#3`. When some element lacks the key, or its key is only known after apply, the list falls back to positions (a set, to whole values).

```console
$ terraform providers schema -json > schema.json
$ terraform show -json plan.bin | tfdiff --schema schema.json
```

```toml
# tfdiff.toml — picked up from the working directory, or passed with --config
[list_keys]
"azurerm_network_security_group.security_rule" = "name"
"terraform_data.input.rules" = "port"
```

The key is `"<resource type>.<attribute path>"`, the path without list positions. In a list of plain values configured this way, elements are paired by value, whatever attribute is named.

The schema also tells maps from blocks: a map key that equals a sensitive value or is shaped like a credential is printed as `(masked)`.

### Secrets

tfdiff never prints:

- values flagged in `before_sensitive` / `after_sensitive` — a flagged list or block is reduced to its element count, `(masked: 3)`;
- any string equal to, or containing, a value flagged anywhere in the plan (providers often copy a secret into an unflagged attribute, as `terraform_data.output` does);
- strings under attributes named like credentials (`password`, `token`, `connection_string`, …);
- strings shaped like credentials (PEM blocks, JWTs, cloud and Git tokens, connection strings, SAS signatures, URLs with a password, long random tokens).

Error messages never quote the plan either.

### Gates

`--expect` turns tfdiff into a check for pull requests that must not change infrastructure.

| Expectation | Breached by |
|---|---|
| `no-destroy` | any destroy or recreation |
| `only-moves` | anything other than address changes |
| `no-changes` | any change, including moves |

| Exit status | Meaning |
|---|---|
| 0 | expectation upheld, or none given |
| 1 | error: unreadable or invalid plan, config or arguments |
| 2 | expectation breached |
| 3 | undetermined: nothing breaches it, but values known only after apply remain |

## Development

```console
$ cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
$ tests/fixtures/regen.sh           # rebuild the plan fixtures; needs terraform, no cloud access
$ tests/fixtures/schema/regen.sh    # rebuild azurerm.schema.json; downloads the azurerm provider, needs jq
```

Fixtures are real `terraform show -json` output generated from the `terraform_data` configs in `tests/fixtures/src/`, except `azurerm_synthetic.plan.json`, which is written by hand to exercise `azurerm.schema.json` (the real provider schema, trimmed to four resource types). `tests/leak.rs` fails if any flagged or `CANARY`-marked value reaches any output of any format, so new fixtures and new formats are covered without extra work.

### Adding an output format

1. Add `src/render/<name>.rs` with a type implementing `render::Renderer`.
2. Add it to `render::registry()`.

`--format`, `--help` and the leak test read the registry. A renderer only receives a `Report`, whose values have already been redacted.

### Static binaries

```console
$ rustup target add x86_64-unknown-linux-musl
$ cargo build --release --target x86_64-unknown-linux-musl
$ cargo build --release --target x86_64-pc-windows-msvc    # on Windows; the C runtime is linked statically
```

Pushing a `v*` tag builds both and attaches them to the GitHub release.
