//! Fails if any secret reaches any output of any registered format.
//!
//! Secrets are found mechanically: every string under a `true` flag in
//! `before_sensitive` / `after_sensitive`, plus every string containing the
//! `CANARY` marker that fixtures plant in credential-shaped, unflagged values.
//! New fixtures and new renderers are covered automatically.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::{Value, json};
use tfdiff::config::Config;
use tfdiff::diff::diff_plan;
use tfdiff::gate::{self, Expect};
use tfdiff::plan::Plan;
use tfdiff::render::registry;
use tfdiff::schema::Schemas;

const MARKER: &str = "CANARY";

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .expect("fixtures directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.to_string_lossy().ends_with(".plan.json"))
        .collect();
    paths.sort();
    assert_ne!(paths.len(), 0, "no fixtures found");
    paths
}

fn strings(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(text) if !text.is_empty() => {
            out.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

fn flagged(value: &Value, mask: &Value, out: &mut BTreeSet<String>) {
    match (value, mask) {
        (_, Value::Bool(true)) => strings(value, out),
        (Value::Array(items), Value::Array(masks)) => {
            for (item, mask) in items.iter().zip(masks) {
                flagged(item, mask, out);
            }
        }
        (Value::Object(fields), Value::Object(masks)) => {
            for (key, mask) in masks {
                if let Some(item) = fields.get(key) {
                    flagged(item, mask, out);
                }
            }
        }
        _ => {}
    }
}

/// Every string in the plan that must never be printed.
fn secrets(plan: &Value) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for resource in plan["resource_changes"].as_array().into_iter().flatten() {
        let change = &resource["change"];
        flagged(&change["before"], &change["before_sensitive"], &mut found);
        flagged(&change["after"], &change["after_sensitive"], &mut found);
    }
    let mut all = BTreeSet::new();
    strings(plan, &mut all);
    found.extend(all.into_iter().filter(|text| text.contains(MARKER)));
    found
}

fn assert_clean(output: &str, secrets: &BTreeSet<String>, context: &str) {
    assert!(
        !output.contains(MARKER),
        "{context}: output contains the {MARKER} marker:\n{output}"
    );
    for secret in secrets {
        assert!(
            !output.contains(secret.as_str()),
            "{context}: a secret of {} bytes leaked:\n{output}",
            secret.len()
        );
    }
}

/// Renders `plan` with every format and every expectation.
fn render_everything(
    plan_json: &[u8],
    config: &Config,
    schemas: Option<&Schemas>,
) -> Vec<(String, String)> {
    let plan = Plan::from_slice(plan_json).expect("plan parses");
    let mut outputs = Vec::new();
    for expect in [
        None,
        Some(Expect::NoDestroy),
        Some(Expect::OnlyMoves),
        Some(Expect::NoChanges),
    ] {
        let mut report = diff_plan(&plan, config, schemas);
        report.gate = expect.map(|expect| gate::evaluate(expect, &report));
        outputs.push(("debug".to_owned(), format!("{report:?} {plan:?}")));
        for renderer in registry() {
            let mut buffer = Vec::new();
            renderer
                .render(&report, &mut buffer)
                .expect("render succeeds");
            outputs.push((
                renderer.name().to_owned(),
                String::from_utf8(buffer).expect("utf-8 output"),
            ));
        }
    }
    outputs
}

#[test]
fn no_format_leaks_a_secret_from_any_fixture() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let positional = Config::default();
    let keyed = Config::parse(
        &fs::read_to_string(fixtures_dir.join("keyed.toml")).expect("keyed config readable"),
    )
    .expect("valid config");
    let schemas =
        Schemas::from_file(&fixtures_dir.join("azurerm.schema.json")).expect("schema fixture");
    let mut checked_secrets = 0;
    for path in fixtures() {
        let bytes = fs::read(&path).expect("fixture readable");
        let secrets = secrets(&serde_json::from_slice(&bytes).expect("fixture is JSON"));
        checked_secrets += secrets.len();
        // Positional, keyed by config, and keyed by schema.
        for (config, schemas) in [
            (&positional, None),
            (&keyed, None),
            (&positional, Some(&schemas)),
        ] {
            for (format, output) in render_everything(&bytes, config, schemas) {
                assert_clean(
                    &output,
                    &secrets,
                    &format!("{} as {format}", path.display()),
                );
            }
        }
    }
    assert!(
        checked_secrets >= 10,
        "the fixtures no longer contain secrets to look for"
    );
}

#[test]
fn the_binary_leaks_nothing_on_stdout_or_stderr() {
    for path in fixtures() {
        let bytes = fs::read(&path).expect("fixture readable");
        let secrets = secrets(&serde_json::from_slice(&bytes).expect("fixture is JSON"));
        for renderer in registry() {
            let output = Command::cargo_bin("tfdiff")
                .expect("binary is built")
                .arg(&path)
                .args(["--format", renderer.name(), "--expect", "no-changes"])
                .output()
                .expect("tfdiff runs");
            let context = format!("{} as {}", path.display(), renderer.name());
            assert_clean(&String::from_utf8_lossy(&output.stdout), &secrets, &context);
            assert_clean(&String::from_utf8_lossy(&output.stderr), &secrets, &context);
        }
    }
}

#[test]
fn truncated_and_corrupt_plans_leak_nothing_in_errors() {
    let bytes =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sensitive.plan.json"))
            .expect("fixture readable");
    let secrets = secrets(&serde_json::from_slice(&bytes).expect("fixture is JSON"));
    let text = String::from_utf8(bytes).expect("utf-8 fixture");

    // Wrong types where a secret sits right at the error position.
    let corrupt = |edit: fn(&mut Value)| {
        let mut plan: Value = serde_json::from_str(&text).expect("fixture is JSON");
        edit(&mut plan);
        plan.to_string()
    };
    let mut broken = vec![
        corrupt(|plan| plan["resource_changes"] = json!("CANARY-top-level")),
        corrupt(|plan| plan["resource_changes"][0]["change"]["actions"] = json!("CANARY-actions")),
        corrupt(|plan| plan["resource_changes"][0]["address"] = json!({"CANARY-key": 1})),
        corrupt(|plan| plan["format_version"] = json!(["CANARY-version"])),
    ];
    // Cut the document off in the middle of each secret.
    for secret in &secrets {
        let at = text
            .find(secret.as_str())
            .expect("secret is in the fixture");
        broken.push(text[..at + secret.len() / 2].to_owned());
    }

    for input in broken {
        let output = Command::cargo_bin("tfdiff")
            .expect("binary is built")
            .write_stdin(input)
            .output()
            .expect("tfdiff runs");
        assert_eq!(output.status.code(), Some(1));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_clean(&stderr, &secrets, "stderr of a corrupt plan");
        assert_clean(
            &String::from_utf8_lossy(&output.stdout),
            &secrets,
            "stdout of a corrupt plan",
        );
        // Not even a fragment of the half-cut secret.
        assert!(!stderr.contains("CANA"), "{stderr}");
    }
}

fn plan_with(change: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format_version": "1.2",
        "resource_changes": [{
            "address": "example.this",
            "mode": "managed",
            "type": "example",
            "change": change,
        }],
    }))
    .expect("serializable")
}

fn assert_synthetic_clean(change: &Value, config: &Config) {
    assert_synthetic_clean_with(change, config, None);
}

fn assert_synthetic_clean_with(change: &Value, config: &Config, schemas: Option<&Schemas>) {
    let bytes = plan_with(change);
    let secrets = secrets(&serde_json::from_slice(&bytes).expect("JSON"));
    assert!(!secrets.is_empty());
    for (format, output) in render_everything(&bytes, config, schemas) {
        assert_clean(&output, &secrets, &format);
    }
}

/// Schema for the `example` type: `labels` is a map, `members` a set of
/// objects without a key, `users` a list of objects with a `name`.
fn example_schema() -> Schemas {
    let bytes = serde_json::to_vec(&json!({
        "format_version": "1.0",
        "provider_schemas": {"registry.example/acme/example": {"resource_schemas": {"example": {"block": {
            "attributes": {
                "labels": {"type": ["map", "string"]},
                "members": {"type": ["set", ["object", {"role": "string", "token": "string"}]]},
                "users": {"type": ["list", ["object", {"name": "string", "role": "string"}]]},
            },
        }}}}},
    }))
    .expect("serializable");
    Schemas::from_slice(&bytes, Path::new("test")).expect("valid schema")
}

#[test]
fn secret_map_keys_never_appear_in_paths_or_values() {
    let schemas = example_schema();
    // The flagged value is reused as a map key; a credential-shaped key is planted too.
    let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJDQU5BUlkifQ.CANARYsignatureCANARYsignature";
    for after_labels in [json!({"CANARY-flagged-value": "b", jwt: "b"}), Value::Null] {
        assert_synthetic_clean_with(
            &json!({
                "actions": ["update"],
                "before": {"api_key": "CANARY-flagged-value", "labels": {"CANARY-flagged-value": "a", jwt: "a", "env": "dev"}},
                "after": {"api_key": "CANARY-flagged-value", "labels": after_labels},
                "before_sensitive": {"api_key": true},
                "after_sensitive": {"api_key": true},
                "after_unknown": {},
            }),
            &Config::default(),
            Some(&schemas),
        );
    }
}

#[test]
fn schema_inferred_keys_and_set_values_never_leak() {
    let schemas = example_schema();
    assert_synthetic_clean_with(
        &json!({
            "actions": ["update"],
            "before": {
                "users": [{"name": "CANARY-key-a", "role": "r1"}, {"name": "CANARY-key-b", "role": "r1"}],
                "members": [{"role": "r1", "token": "CANARY-token-one"}, {"role": "r2", "token": "CANARY-token-two"}],
            },
            "after": {
                "users": [{"name": "CANARY-key-b", "role": "r2"}, {"name": "CANARY-key-c", "role": "r1"}],
                "members": [{"role": "r3", "token": "CANARY-token-two"}, {"role": "r1", "token": "CANARY-token-three"}],
            },
            "before_sensitive": {"users": [{"name": true}, {"name": true}], "members": [{"token": true}, {"token": true}]},
            "after_sensitive": {"users": [{"name": true}, {"name": true}], "members": [{"token": true}, {"token": true}]},
            "after_unknown": {},
        }),
        &Config::default(),
        Some(&schemas),
    );
}

#[test]
fn sensitive_list_keys_never_appear_in_paths() {
    assert_synthetic_clean(
        &json!({
            "actions": ["update"],
            "before": {"users": [{"name": "CANARY-key-a", "role": "r1"}, {"name": "CANARY-key-b", "role": "r1"}]},
            "after": {"users": [{"name": "CANARY-key-b", "role": "r2"}, {"name": "CANARY-key-c", "role": "r1"}]},
            "before_sensitive": {"users": [{"name": true}, {"name": true}]},
            "after_sensitive": {"users": [{"name": true}, {"name": true}]},
            "after_unknown": {},
        }),
        &Config::parse("[list_keys]\n\"example.users\" = \"name\"\n").expect("valid config"),
    );
}

#[test]
fn credential_shaped_list_keys_never_appear_in_paths() {
    let config =
        Config::parse("[list_keys]\n\"example.users\" = \"conn\"\n").expect("valid config");
    assert_synthetic_clean(
        &json!({
            "actions": ["update"],
            "before": {"users": [{"conn": "Server=a;Password=CANARY-one", "role": "r1"}]},
            "after": {"users": [{"conn": "Server=a;Password=CANARY-one", "role": "r2"}]},
            "after_unknown": {},
        }),
        &config,
    );
}

#[test]
fn secrets_flagged_on_one_side_only_are_masked_on_both() {
    assert_synthetic_clean(
        &json!({
            "actions": ["update"],
            "before": {"value": "CANARY-was-plain", "copy": "CANARY-now-secret", "list": ["CANARY-now-secret", "x"]},
            "after": {"value": "CANARY-now-secret", "copy": "CANARY-was-plain", "list": ["x"]},
            "before_sensitive": {},
            "after_sensitive": {"value": true},
            "after_unknown": {},
        }),
        &Config::default(),
    );
}

#[test]
fn secrets_in_removed_and_recreated_values_are_masked() {
    assert_synthetic_clean(
        &json!({
            "actions": ["delete", "create"],
            "before": {"rules": [{"name": "a", "admin_password": "CANARY-hinted"}], "blob": {"k": "CANARY-flagged"}},
            "after": {"rules": [], "blob": null},
            "before_sensitive": {"blob": true},
            "after_sensitive": {},
            "after_unknown": {},
            "replace_paths": [["rules", 0, "admin_password"]],
        }),
        &Config::default(),
    );
}
