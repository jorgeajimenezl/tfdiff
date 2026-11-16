//! Secret safety, enforced by types.
//!
//! * [`Raw`] wraps plan JSON. It cannot be displayed, serialized or debug-printed.
//! * [`Shown`] is the only value type a report can hold, and the only way to
//!   build one is through a [`Redactor`], which masks:
//!   - anything flagged in `before_sensitive` / `after_sensitive`,
//!   - any string equal to (or containing) a value flagged sensitive anywhere
//!     in the plan — providers often copy a secret into an unflagged attribute,
//!   - strings that look like credentials, by attribute name or by shape.
//!
//! Renderers only ever receive `Shown`, so no output format can leak.

use std::collections::HashSet;
use std::fmt;

use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Number, Value};

use crate::diff::path::Path;
use crate::labels;
use crate::plan::Plan;
use crate::schema::Shape;

static NULL: Value = Value::Null;

/// Unredacted plan JSON. Deliberately opaque outside this crate.
#[derive(Clone, Default, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct Raw(Value);

impl Raw {
    pub(crate) fn expose(&self) -> &Value {
        &self.0
    }
}

impl From<Value> for Raw {
    fn from(value: Value) -> Self {
        Self(value)
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<raw>")
    }
}

/// A redacted value tree, safe to print.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    /// A hidden scalar.
    Masked,
    /// A hidden list, map or object; only its element count is reported.
    MaskedItems(usize),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    fn contains_mask(&self) -> bool {
        match self {
            Self::Masked | Self::MaskedItems(_) => true,
            Self::Array(items) => items.iter().any(Self::contains_mask),
            Self::Object(fields) => fields.iter().any(|(_, node)| node.contains_mask()),
            _ => false,
        }
    }
}

impl Serialize for Node {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Number(value) => value.serialize(serializer),
            Self::String(value) => serializer.serialize_str(value),
            Self::Masked => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry(labels::MASKED, &true)?;
                map.end()
            }
            Self::MaskedItems(count) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry(labels::MASKED, &true)?;
                map.serialize_entry("count", count)?;
                map.end()
            }
            Self::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Self::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (key, node) in fields {
                    map.serialize_entry(key, node)?;
                }
                map.end()
            }
        }
    }
}

/// A value that has passed redaction. Cannot be constructed outside [`Redactor`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Shown {
    node: Node,
}

impl Shown {
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// True when any part of the value was hidden.
    pub fn is_masked(&self) -> bool {
        self.node.contains_mask()
    }
}

/// Strings shorter than this are only matched exactly against known secrets;
/// longer ones are also searched for inside other strings.
const MIN_EMBEDDED_SECRET_LEN: usize = 8;

/// Decides what may be shown, for one plan.
#[derive(Default)]
pub struct Redactor {
    /// Every string flagged sensitive anywhere in the plan.
    tainted: HashSet<String>,
    /// The subset of `tainted` long enough to search for as a substring.
    tainted_long: Vec<String>,
}

impl fmt::Debug for Redactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Redactor({} known secrets)", self.tainted.len())
    }
}

impl Redactor {
    /// Collects every string flagged sensitive in the plan.
    pub fn from_plan(plan: &Plan) -> Self {
        let mut tainted = HashSet::new();
        for resource in &plan.resource_changes {
            let change = &resource.change;
            // A value is treated as secret if either side flags its position:
            // an attribute that is sensitive now most likely held a secret before.
            for value in [&change.before, &change.after] {
                for mask in [&change.before_sensitive, &change.after_sensitive] {
                    collect_flagged(value.expose(), mask.expose(), &mut tainted);
                }
            }
        }
        let tainted_long = tainted
            .iter()
            .filter(|secret| secret.len() >= MIN_EMBEDDED_SECRET_LEN)
            .cloned()
            .collect();
        Self {
            tainted,
            tainted_long,
        }
    }

    /// Redacts `value` found at `path`, honouring its sensitivity `mask`
    /// (the matching subtree of `before_sensitive` / `after_sensitive`).
    /// `shape` is what the provider schema says about the value, when known;
    /// it lets secret-looking map keys be hidden too.
    pub(crate) fn show(
        &self,
        path: &Path,
        value: &Value,
        mask: &Value,
        shape: Option<&Shape>,
    ) -> Shown {
        let hinted = path.attrs().any(name_hints_secret);
        Shown {
            node: self.redact(hinted, value, mask, shape),
        }
    }

    /// True when a map key must not be printed: it equals or contains a value
    /// flagged elsewhere in the plan, or is shaped like a credential.
    pub(crate) fn hides_key(&self, key: &str) -> bool {
        self.is_secret(key)
    }

    /// Hides `value` entirely, reporting only an element count for collections.
    pub(crate) fn hide(value: &Value) -> Shown {
        Shown {
            node: hidden(value),
        }
    }

    fn redact(&self, hinted: bool, value: &Value, mask: &Value, shape: Option<&Shape>) -> Node {
        if mask == &Value::Bool(true) {
            return hidden(value);
        }
        match value {
            Value::Null => Node::Null,
            Value::Bool(flag) => Node::Bool(*flag),
            Value::Number(number) => Node::Number(number.clone()),
            Value::String(text) => {
                if hinted || self.is_secret(text) {
                    Node::Masked
                } else {
                    Node::String(text.clone())
                }
            }
            Value::Array(items) => Node::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        self.redact(
                            hinted,
                            item,
                            mask.get(index).unwrap_or(&NULL),
                            shape.and_then(Shape::element),
                        )
                    })
                    .collect(),
            ),
            Value::Object(fields) => {
                let is_map = shape.is_some_and(Shape::is_map);
                let mut hidden_keys = 0_usize;
                Node::Object(
                    fields
                        .iter()
                        .map(|(key, item)| {
                            let node = self.redact(
                                hinted || name_hints_secret(key),
                                item,
                                mask.get(key).unwrap_or(&NULL),
                                shape.and_then(|shape| shape.child(key)),
                            );
                            if is_map && self.hides_key(key) {
                                hidden_keys += 1;
                                (masked_key(hidden_keys), node)
                            } else {
                                (key.clone(), node)
                            }
                        })
                        .collect(),
                )
            }
        }
    }

    fn is_secret(&self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        self.tainted.contains(text)
            || self
                .tainted_long
                .iter()
                .any(|secret| text.contains(secret.as_str()))
            || looks_like_credential(text)
    }
}

/// Stand-in for the n-th hidden key of a map: `(masked)`, `(masked)#2`, …
fn masked_key(nth: usize) -> String {
    if nth == 1 {
        format!("({})", labels::MASKED)
    } else {
        format!("({})#{nth}", labels::MASKED)
    }
}

fn hidden(value: &Value) -> Node {
    match value {
        Value::Array(items) => Node::MaskedItems(items.len()),
        Value::Object(fields) => Node::MaskedItems(fields.len()),
        _ => Node::Masked,
    }
}

fn collect_flagged(value: &Value, mask: &Value, out: &mut HashSet<String>) {
    match (value, mask) {
        (_, Value::Bool(true)) => collect_strings(value, out),
        (Value::Array(items), Value::Array(masks)) => {
            for (item, mask) in items.iter().zip(masks) {
                collect_flagged(item, mask, out);
            }
        }
        (Value::Object(fields), Value::Object(masks)) => {
            for (key, mask) in masks {
                if let Some(item) = fields.get(key) {
                    collect_flagged(item, mask, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_strings(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::String(text) if !text.is_empty() => {
            out.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// Attribute-name fragments that suggest the value is a credential.
const SECRET_NAME_PARTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "credential",
    "private_key",
    "api_key",
    "apikey",
    "access_key",
    "account_key",
    "shared_key",
    "primary_key",
    "secondary_key",
    "connection_string",
];

/// Short fragments that only count as a whole `_`-separated word.
const SECRET_NAME_WORDS: &[&str] = &["pwd", "sas"];

/// Suffixes that mark an attribute as a reference to, or a setting about, a
/// secret rather than the secret itself (`key_vault_secret_id`, `token_ttl`).
const NOT_SECRET_SUFFIXES: &[&str] = &[
    "_id", "_ids", "_name", "_names", "_uri", "_url", "_version", "_enabled", "_type", "_length",
    "_ttl", "_path", "_count",
];

/// Whether an attribute name suggests its string value is a credential.
pub(crate) fn name_hints_secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    if NOT_SECRET_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        return false;
    }
    SECRET_NAME_PARTS.iter().any(|part| name.contains(part))
        || name
            .split(['_', '-'])
            .any(|word| SECRET_NAME_WORDS.contains(&word))
}

const CREDENTIAL_PREFIXES: &[&str] = &[
    "-----BEGIN ",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "sk-",
    "sk_live_",
];

/// `key=value` markers found in connection strings and SAS URLs (lowercase).
const CREDENTIAL_MARKERS: &[&str] = &[
    "accountkey=",
    "sharedaccesskey=",
    "sharedaccesssignature=",
    "password=",
    "pwd=",
    "client_secret=",
    "sig=",
];

/// Minimum length of an unbroken token before its randomness is judged.
const MIN_RANDOM_TOKEN_LEN: usize = 32;
/// Shannon entropy (bits per character) above which a long token counts as random.
const MIN_RANDOM_TOKEN_ENTROPY: f64 = 4.0;

/// Whether a string has the shape of a credential.
pub(crate) fn looks_like_credential(text: &str) -> bool {
    let trimmed = text.trim_start();
    if CREDENTIAL_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }
    let lower = text.to_ascii_lowercase();
    if CREDENTIAL_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return true;
    }
    is_aws_access_key_id(trimmed)
        || is_jwt(trimmed)
        || has_url_password(text)
        || text
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '='))
            .any(is_random_token)
}

fn is_aws_access_key_id(text: &str) -> bool {
    text.len() == 20
        && (text.starts_with("AKIA") || text.starts_with("ASIA"))
        && text
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn is_jwt(text: &str) -> bool {
    text.starts_with("eyJ") && text.split('.').count() == 3
}

/// `scheme://user:password@host`
fn has_url_password(text: &str) -> bool {
    text.split_once("://").is_some_and(|(_, rest)| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority
            .rsplit_once('@')
            .is_some_and(|(userinfo, _)| userinfo.contains(':'))
    })
}

/// A long run mixing upper case, lower case and digits with high entropy.
/// Hex-only runs (GUIDs, digests) and ordinary words never qualify, which
/// keeps Azure resource IDs readable.
fn is_random_token(token: &str) -> bool {
    if token.len() < MIN_RANDOM_TOKEN_LEN {
        return false;
    }
    let has = |pred: fn(&u8) -> bool| token.as_bytes().iter().any(pred);
    has(u8::is_ascii_uppercase)
        && has(u8::is_ascii_lowercase)
        && has(u8::is_ascii_digit)
        && shannon_entropy(token) >= MIN_RANDOM_TOKEN_ENTROPY
}

fn shannon_entropy(text: &str) -> f64 {
    let mut counts = [0u32; 256];
    for byte in text.bytes() {
        counts[usize::from(byte)] += 1;
    }
    let total = f64::from(u32::try_from(text.len()).unwrap_or(u32::MAX));
    counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = f64::from(count) / total;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::path::Segment;
    use serde_json::json;

    fn path(attrs: &[&str]) -> Path {
        let mut path = Path::default();
        for attr in attrs {
            path.push(Segment::Attr((*attr).to_owned()));
        }
        path
    }

    fn show(redactor: &Redactor, attrs: &[&str], value: &Value, mask: &Value) -> Node {
        redactor
            .show(&path(attrs), value, mask, None)
            .node()
            .clone()
    }

    #[test]
    fn flagged_scalar_is_masked() {
        let node = show(&Redactor::default(), &["x"], &json!("v"), &json!(true));
        assert_eq!(node, Node::Masked);
    }

    #[test]
    fn flagged_block_reports_only_a_count() {
        let value = json!([{"a": "one"}, {"a": "two"}]);
        let node = show(&Redactor::default(), &["x"], &value, &json!(true));
        assert_eq!(node, Node::MaskedItems(2));
    }

    #[test]
    fn mask_applies_per_leaf() {
        let value = json!({"user": "alice", "pass": "p"});
        let node = show(&Redactor::default(), &["x"], &value, &json!({"pass": true}));
        assert_eq!(
            node,
            Node::Object(vec![
                ("pass".into(), Node::Masked),
                ("user".into(), Node::String("alice".into())),
            ])
        );
    }

    #[test]
    fn secrets_copied_to_unflagged_attributes_are_masked() {
        let plan = Plan::from_slice(
            serde_json::to_vec(&json!({
                "format_version": "1.2",
                "resource_changes": [{
                    "address": "a.b",
                    "change": {
                        "before": {"input": "hunter2", "output": "hunter2"},
                        "before_sensitive": {"input": true},
                    },
                }],
            }))
            .unwrap()
            .as_slice(),
        )
        .unwrap();
        let redactor = Redactor::from_plan(&plan);
        assert_eq!(
            show(&redactor, &["output"], &json!("hunter2"), &NULL),
            Node::Masked
        );
        assert_eq!(
            show(&redactor, &["output"], &json!("other"), &NULL),
            Node::String("other".into())
        );
    }

    #[test]
    fn long_secrets_are_found_inside_other_strings() {
        let redactor = Redactor {
            tainted: HashSet::from(["s3cr3t-value".to_owned()]),
            tainted_long: vec!["s3cr3t-value".to_owned()],
        };
        let node = show(
            &redactor,
            &["url"],
            &json!("https://h/?q=s3cr3t-value"),
            &NULL,
        );
        assert_eq!(node, Node::Masked);
    }

    #[test]
    fn attribute_names_hint_at_secrets() {
        for name in [
            "password",
            "admin_password",
            "clientSecret",
            "sas_token",
            "db_pwd",
        ] {
            assert!(name_hints_secret(name), "{name}");
        }
        for name in [
            "name",
            "key",
            "key_vault_secret_id",
            "secret_name",
            "token_ttl",
            "disaster_recovery",
            "password_authentication_enabled",
        ] {
            assert!(!name_hints_secret(name), "{name}");
        }
    }

    #[test]
    fn hint_is_inherited_by_nested_values() {
        let node = show(
            &Redactor::default(),
            &["secrets"],
            &json!({"a": "plain"}),
            &NULL,
        );
        assert_eq!(node, Node::Object(vec![("a".into(), Node::Masked)]));
    }

    #[test]
    fn credential_shapes_are_detected() {
        for text in [
            "-----BEGIN RSA PRIVATE KEY-----\nabc",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "AKIAIOSFODNN7EXAMPLE",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2lnbmF0dXJl",
            "DefaultEndpointsProtocol=https;AccountName=a;AccountKey=abc==",
            "https://acct.blob.core.windows.net/c?sv=2022&sig=abc",
            "postgres://admin:hunter2@db.internal:5432/app",
            "9xK2mPq7Lw4Zt8Rv3Ns6Yb1Hd5Gf0JcAeUiOoWzXyTn",
        ] {
            assert!(looks_like_credential(text), "{text}");
        }
    }

    #[test]
    fn ordinary_values_are_not_credentials() {
        for text in [
            "",
            "frontend",
            "Standard_D2s_v3",
            "2026-11-16T17:01:18Z",
            "0b1f6471-1bf0-4dda-aec3-cb9272f09590",
            "/subscriptions/0b1f6471-1bf0-4dda-aec3-cb9272f09590/resourceGroups/rg-Prod-01/providers/Microsoft.Network/virtualNetworks/vnet-Hub-WestEurope-001",
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "https://example.com/path?query=1",
            "this-is-a-very-long-resource-name-for-production-westeurope",
        ] {
            assert!(!looks_like_credential(text), "{text}");
        }
    }

    #[test]
    fn masks_serialize_unambiguously() {
        let node = Node::Array(vec![
            Node::Masked,
            Node::MaskedItems(3),
            Node::String("x".into()),
        ]);
        assert_eq!(
            serde_json::to_value(&node).unwrap(),
            json!([{"masked": true}, {"masked": true, "count": 3}, "x"])
        );
    }
}
