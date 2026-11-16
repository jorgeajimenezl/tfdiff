//! End-to-end tests: the real binary against real `terraform show -json` output.

use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.plan.json"))
}

fn tfdiff() -> Command {
    let mut command = Command::cargo_bin("tfdiff").expect("binary is built");
    // Keep a developer's ./tfdiff.toml out of the tests.
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--config")
        .arg("tests/fixtures/empty.toml");
    command
}

fn text(name: &str) -> String {
    let output = tfdiff().arg(fixture(name)).output().expect("tfdiff runs");
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).expect("utf-8 output")
}

fn json(name: &str) -> serde_json::Value {
    let output = tfdiff()
        .arg(fixture(name))
        .args(["--format", "json"])
        .output()
        .expect("tfdiff runs");
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("valid JSON output")
}

#[test]
fn create() {
    assert_eq!(
        text("create"),
        "tfdiff: +1 -0 -/+0 ~0 x0 =>0 ==0\n+ terraform_data.app\n"
    );
}

#[test]
fn destroy() {
    assert_eq!(
        text("destroy"),
        "tfdiff: +0 -1 -/+0 ~0 x0 =>0 ==0\n- terraform_data.app\n"
    );
}

#[test]
fn noop() {
    assert_eq!(text("noop"), "tfdiff: +0 -0 -/+0 ~0 x0 =>0 ==1\n");
}

#[test]
fn list_reorder_is_one_line_not_positional_churn() {
    assert_eq!(
        text("list_reorder"),
        "tfdiff: +0 -0 -/+0 ~1 x0 =>0 ==0\n\
         ~ terraform_data.app\n\
         \x20   ^v input.rules\n\
         \x20   ? output\n"
    );
}

#[test]
fn list_elements_are_matched_by_key() {
    assert_eq!(
        text("list_change"),
        "tfdiff: +0 -0 -/+0 ~1 x0 =>0 ==0\n\
         ~ terraform_data.app\n\
         \x20   ~ input.rules[https].port: 443 -> 8443\n\
         \x20   - input.rules[http]: {access=\"Allow\", name=\"http\", port=80}\n\
         \x20   + input.rules[dns]: {access=\"Allow\", name=\"dns\", port=53}\n\
         \x20   ^v input.rules\n\
         \x20   ? output\n"
    );
}

#[test]
fn duplicate_keys_are_numbered() {
    assert_eq!(
        text("dup_keys"),
        "tfdiff: +0 -0 -/+0 ~1 x0 =>0 ==0\n\
         ~ terraform_data.app\n\
         \x20   ~ input.rules[web#2].port: 443 -> 8443\n\
         \x20   ^v input.rules\n\
         \x20   ? output\n"
    );
}

#[test]
fn moved_resource_shows_previous_address() {
    assert_eq!(
        text("moved"),
        "tfdiff: +0 -0 -/+0 ~0 x0 =>1 ==1\n\
         => terraform_data.new\n\
         \x20   < terraform_data.old\n"
    );
}

#[test]
fn recreation_names_the_forcing_path() {
    assert_eq!(
        text("recreate"),
        "tfdiff: +0 -0 -/+1 ~0 x0 =>0 ==0\n\
         -/+ terraform_data.app\n\
         \x20   ! triggers_replace\n\
         \x20   ? id\n\
         \x20   ? output\n\
         \x20   ~ triggers_replace.image: \"v1\" -> \"v2\"\n"
    );
}

#[test]
fn sensitive_values_are_masked_and_blocks_only_counted() {
    assert_eq!(
        text("sensitive"),
        "tfdiff: +0 -0 -/+0 ~2 x0 =>0 ==0\n\
         ~ terraform_data.block\n\
         \x20   ~ input: (masked: 2) -> (masked: 3)\n\
         \x20   ? output\n\
         ~ terraform_data.leaf\n\
         \x20   ~ input.api: (masked) -> (masked)\n\
         \x20   ~ input.endpoint: (masked) -> (masked)\n\
         \x20   ~ input.password: (masked) -> (masked)\n\
         \x20   ~ input.users[bob].token: (masked) -> (masked)\n\
         \x20   ^v input.users\n\
         \x20   ? output\n"
    );
}

#[test]
fn unknown_after_apply() {
    assert_eq!(
        text("unknown"),
        "tfdiff: +0 -0 -/+0 ~1 x0 =>0 ==0\n\
         ~ terraform_data.app\n\
         \x20   ? input\n\
         \x20   ? output\n"
    );
}

#[test]
fn json_output_has_a_stable_shape() {
    let report = json("list_change");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["totals"]["mutate"], 1);
    let resource = &report["resources"][0];
    assert_eq!(resource["address"], "terraform_data.app");
    assert_eq!(resource["action"], "mutate");
    assert_eq!(
        resource["changes"][0],
        serde_json::json!({
            "path": "input.rules[https].port",
            "class": "effective",
            "op": "altered",
            "before": 443,
            "after": 8443,
        })
    );
    assert!(report.get("gate").is_none());
}

#[test]
fn json_output_reports_moves_and_forcing_paths() {
    assert_eq!(
        json("moved")["resources"][0]["previous_address"],
        "terraform_data.old"
    );
    assert_eq!(
        json("recreate")["resources"][0]["forced_by"],
        serde_json::json!(["triggers_replace"])
    );
}

#[test]
fn reads_the_plan_from_stdin() {
    tfdiff()
        .pipe_stdin(fixture("noop"))
        .expect("fixture exists")
        .assert()
        .success()
        .stdout("tfdiff: +0 -0 -/+0 ~0 x0 =>0 ==1\n");
}

#[test]
fn config_overrides_the_list_key() {
    let dir = std::env::temp_dir().join(format!("tfdiff-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let config = dir.join("tfdiff.toml");
    std::fs::write(
        &config,
        "[list_keys]\n\"terraform_data.input.rules\" = \"port\"\n",
    )
    .expect("config written");

    // Keyed by port, changing https from 443 to 8443 is a removal plus an addition.
    Command::cargo_bin("tfdiff")
        .expect("binary is built")
        .arg(fixture("list_change"))
        .arg("--config")
        .arg(&config)
        .assert()
        .success()
        .stdout(predicate::str::contains("- input.rules[443]:"))
        .stdout(predicate::str::contains("+ input.rules[8443]:"));
    std::fs::remove_dir_all(&dir).expect("temp dir removed");
}

/// (expectation, fixture, exit status)
const GATES: &[(&str, &str, i32)] = &[
    ("no-changes", "noop", 0),
    ("no-changes", "moved", 2),
    ("no-changes", "list_change", 2),
    ("no-changes", "create", 2),
    ("no-changes", "unknown", 3),
    ("only-moves", "noop", 0),
    ("only-moves", "moved", 0),
    ("only-moves", "list_reorder", 2),
    ("only-moves", "unknown", 3),
    ("no-destroy", "create", 0),
    ("no-destroy", "list_change", 0),
    ("no-destroy", "unknown", 0),
    ("no-destroy", "destroy", 2),
    ("no-destroy", "recreate", 2),
];

#[test]
fn gates_set_the_exit_status() {
    for &(expect, name, code) in GATES {
        let output = tfdiff()
            .arg(fixture(name))
            .args(["--expect", expect])
            .output()
            .expect("tfdiff runs");
        assert_eq!(
            output.status.code(),
            Some(code),
            "--expect {expect} on {name}"
        );
        let verdict = match code {
            0 => "upheld",
            2 => "breached",
            _ => "undetermined",
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("[{expect}] {verdict}")),
            "--expect {expect} on {name}: {stdout}"
        );
    }
}

#[test]
fn gate_result_is_part_of_json_output() {
    let output = tfdiff()
        .arg(fixture("destroy"))
        .args(["--format", "json", "--expect", "no-destroy"])
        .output()
        .expect("tfdiff runs");
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
    assert_eq!(
        report["gate"],
        serde_json::json!({
            "expect": "no-destroy",
            "verdict": "breached",
            "offenders": ["terraform_data.app"],
        })
    );
}

#[test]
fn bad_input_exits_with_1() {
    tfdiff()
        .write_stdin("not json")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("syntax error"));
    tfdiff().arg("does-not-exist.json").assert().code(1);
    tfdiff()
        .write_stdin(r#"{"format_version":"9.0"}"#)
        .assert()
        .code(1);
}

#[test]
fn usage_errors_exit_with_1_not_2() {
    tfdiff().args(["--format", "nope"]).assert().code(1);
    tfdiff().args(["--expect", "nope"]).assert().code(1);
}

#[test]
fn help_lists_every_format() {
    tfdiff()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("possible values: text, json"));
}
