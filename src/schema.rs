//! Provider schemas from `terraform providers schema -json`.
//!
//! The plan JSON does not say whether an array is a list or a set, nor whether
//! an object is a block or a map. The schema does, and it names the attributes
//! of nested blocks, which lets tfdiff infer the key of a list element.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::error::Error;

/// Attributes that identify an element of a nested block, in order of preference.
pub const KEY_CANDIDATES: [&str; 3] = ["name", "key", "id"];

/// What the schema says about a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// Block or object with named attributes.
    Object(BTreeMap<String, Shape>),
    /// Ordered collection.
    List(Box<Shape>),
    /// Unordered collection; the JSON order carries no meaning.
    Set(Box<Shape>),
    /// Keys are data, not attribute names.
    Map(Box<Shape>),
    Text,
    Number,
    /// Bool, dynamic, tuple, or anything tfdiff has no use for.
    Other,
}

impl Shape {
    /// Shape of attribute (or map entry) `name`.
    pub fn child(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Object(fields) => fields.get(name),
            Self::Map(element) => Some(element),
            _ => None,
        }
    }

    /// Shape of the elements of a list or set.
    pub fn element(&self) -> Option<&Self> {
        match self {
            Self::List(element) | Self::Set(element) => Some(element),
            _ => None,
        }
    }

    /// Attribute that identifies this object among its siblings: the first of
    /// [`KEY_CANDIDATES`] declared as a string or number.
    pub fn key_attr(&self) -> Option<&'static str> {
        let Self::Object(fields) = self else {
            return None;
        };
        KEY_CANDIDATES
            .into_iter()
            .find(|&name| matches!(fields.get(name), Some(Self::Text | Self::Number)))
    }

    pub fn is_set(&self) -> bool {
        matches!(self, Self::Set(_))
    }

    pub fn is_map(&self) -> bool {
        matches!(self, Self::Map(_))
    }
}

/// Resource shapes of every provider in a schema file.
#[derive(Debug, Default)]
pub struct Schemas {
    /// provider name → resource type → shape of the resource.
    providers: BTreeMap<String, BTreeMap<String, Shape>>,
}

impl Schemas {
    /// Reads and parses a schema file.
    ///
    /// # Errors
    ///
    /// Fails when the file is unreadable, is not schema JSON, or has a non-1.x
    /// format version.
    pub fn from_file(path: &Path) -> Result<Self, Error> {
        let bytes = fs::read(path).map_err(|source| Error::ReadSchema {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_slice(&bytes, path)
    }

    /// Parses schema JSON held in memory; `origin` only names it in errors.
    ///
    /// # Errors
    ///
    /// Fails when the bytes are not schema JSON or have a non-1.x format version.
    pub fn from_slice(bytes: &[u8], origin: &Path) -> Result<Self, Error> {
        let raw: RawSchemas =
            serde_json::from_slice(bytes).map_err(|source| Error::ParseSchema {
                path: origin.to_path_buf(),
                source: Box::new(source),
            })?;
        if raw.format_version.split('.').next() != Some("1") {
            return Err(Error::UnsupportedSchemaVersion(raw.format_version));
        }
        let providers = raw
            .provider_schemas
            .into_iter()
            .map(|(provider, schema)| {
                let resources = schema
                    .resource_schemas
                    .into_iter()
                    .map(|(resource_type, resource)| (resource_type, resource.block.shape()))
                    .collect();
                (provider, resources)
            })
            .collect();
        Ok(Self { providers })
    }

    /// Shape of `resource_type`, looked up in `provider` when the plan names
    /// one and the file has it, otherwise in any provider declaring the type.
    pub fn resource(&self, provider: Option<&str>, resource_type: &str) -> Option<&Shape> {
        provider
            .and_then(|provider| self.providers.get(provider))
            .and_then(|resources| resources.get(resource_type))
            .or_else(|| {
                self.providers
                    .values()
                    .find_map(|resources| resources.get(resource_type))
            })
    }
}

#[derive(Deserialize)]
struct RawSchemas {
    format_version: String,
    /// Required, so that a plan passed by mistake is rejected.
    provider_schemas: BTreeMap<String, RawProvider>,
}

#[derive(Deserialize)]
struct RawProvider {
    #[serde(default)]
    resource_schemas: BTreeMap<String, RawResource>,
}

#[derive(Deserialize)]
struct RawResource {
    #[serde(default)]
    block: RawBlock,
}

#[derive(Default, Deserialize)]
struct RawBlock {
    #[serde(default)]
    attributes: BTreeMap<String, RawAttribute>,
    #[serde(default)]
    block_types: BTreeMap<String, RawBlockType>,
}

#[derive(Deserialize)]
struct RawAttribute {
    /// cty type: `"string"`, `["list", T]`, `["object", {…}]`, …
    #[serde(rename = "type", default)]
    cty: Option<Value>,
    #[serde(default)]
    nested_type: Option<RawNestedType>,
}

#[derive(Deserialize)]
struct RawNestedType {
    #[serde(default)]
    attributes: BTreeMap<String, RawAttribute>,
    #[serde(default)]
    nesting_mode: String,
}

#[derive(Deserialize)]
struct RawBlockType {
    #[serde(default)]
    nesting_mode: String,
    #[serde(default)]
    block: RawBlock,
}

impl RawBlock {
    fn shape(self) -> Shape {
        let attributes = self
            .attributes
            .into_iter()
            .map(|(name, attribute)| (name, attribute.shape()));
        let blocks = self
            .block_types
            .into_iter()
            .map(|(name, block)| (name, nest(&block.nesting_mode, block.block.shape())));
        Shape::Object(attributes.chain(blocks).collect())
    }
}

impl RawAttribute {
    fn shape(self) -> Shape {
        match (self.nested_type, self.cty) {
            (Some(nested), _) => {
                let fields = nested
                    .attributes
                    .into_iter()
                    .map(|(name, attribute)| (name, attribute.shape()))
                    .collect();
                nest(&nested.nesting_mode, Shape::Object(fields))
            }
            (None, Some(cty)) => cty_shape(&cty),
            (None, None) => Shape::Other,
        }
    }
}

/// Wraps the shape of one nested block according to its nesting mode.
fn nest(mode: &str, element: Shape) -> Shape {
    match mode {
        "list" => Shape::List(Box::new(element)),
        "set" => Shape::Set(Box::new(element)),
        "map" => Shape::Map(Box::new(element)),
        // "single" and "group" hold exactly one object.
        _ => element,
    }
}

fn cty_shape(cty: &Value) -> Shape {
    match cty {
        Value::String(name) => match name.as_str() {
            "string" => Shape::Text,
            "number" => Shape::Number,
            _ => Shape::Other,
        },
        Value::Array(parts) => match (parts.first().and_then(Value::as_str), parts.get(1)) {
            (Some("list"), Some(element)) => Shape::List(Box::new(cty_shape(element))),
            (Some("set"), Some(element)) => Shape::Set(Box::new(cty_shape(element))),
            (Some("map"), Some(element)) => Shape::Map(Box::new(cty_shape(element))),
            (Some("object"), Some(Value::Object(fields))) => Shape::Object(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), cty_shape(field)))
                    .collect(),
            ),
            _ => Shape::Other,
        },
        _ => Shape::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schemas(resource_schemas: &Value) -> Schemas {
        let bytes = serde_json::to_vec(&json!({
            "format_version": "1.0",
            "provider_schemas": {
                "registry.terraform.io/acme/one": {"resource_schemas": resource_schemas},
                "registry.terraform.io/acme/two": {"resource_schemas": {
                    "two_thing": {"block": {"attributes": {"id": {"type": "string"}}}},
                }},
            },
        }))
        .unwrap();
        Schemas::from_slice(&bytes, Path::new("test")).unwrap()
    }

    #[test]
    fn decodes_cty_types_and_nesting_modes() {
        let schemas = schemas(&json!({"one_thing": {"block": {
            "attributes": {
                "tags": {"type": ["map", "string"]},
                "dns": {"type": ["list", "string"]},
                "rule": {"type": ["set", ["object", {"name": "string", "priority": "number"}]]},
                "anything": {"type": "dynamic"},
                "nested": {"nested_type": {"nesting_mode": "list", "attributes": {"id": {"type": "string"}}}},
            },
            "block_types": {
                "pool": {"nesting_mode": "set", "block": {"attributes": {"key": {"type": "string"}, "id": {"type": "string"}}}},
                "single": {"nesting_mode": "single", "block": {"attributes": {"on": {"type": "bool"}}}},
            },
        }}}));
        let shape = schemas.resource(None, "one_thing").unwrap();

        assert_eq!(
            shape.child("tags"),
            Some(&Shape::Map(Box::new(Shape::Text)))
        );
        assert_eq!(
            shape.child("tags").unwrap().child("any-key"),
            Some(&Shape::Text)
        );
        assert_eq!(
            shape.child("dns"),
            Some(&Shape::List(Box::new(Shape::Text)))
        );
        assert_eq!(shape.child("anything"), Some(&Shape::Other));

        let rule = shape.child("rule").unwrap();
        assert!(rule.is_set());
        assert_eq!(rule.element().unwrap().key_attr(), Some("name"));

        assert!(matches!(shape.child("nested"), Some(Shape::List(_))));
        assert_eq!(
            shape.child("nested").unwrap().element().unwrap().key_attr(),
            Some("id")
        );
        // `key` is preferred over `id`.
        assert_eq!(
            shape.child("pool").unwrap().element().unwrap().key_attr(),
            Some("key")
        );
        assert!(matches!(shape.child("single"), Some(Shape::Object(_))));
        assert_eq!(shape.child("single").unwrap().key_attr(), None);
        assert_eq!(shape.child("missing"), None);
    }

    #[test]
    fn key_must_be_a_string_or_number() {
        let schemas = schemas(&json!({"one_thing": {"block": {"attributes": {
            "items": {"type": ["list", ["object", {"name": ["list", "string"], "id": "bool"}]]},
        }}}}));
        let items = schemas
            .resource(None, "one_thing")
            .unwrap()
            .child("items")
            .unwrap();
        assert_eq!(items.element().unwrap().key_attr(), None);
    }

    #[test]
    fn looks_up_by_provider_then_by_type_alone() {
        let schemas = schemas(&json!({}));
        assert!(
            schemas
                .resource(Some("registry.terraform.io/acme/two"), "two_thing")
                .is_some()
        );
        assert!(
            schemas
                .resource(Some("registry.example/mirror/two"), "two_thing")
                .is_some()
        );
        assert!(schemas.resource(None, "two_thing").is_some());
        assert!(schemas.resource(None, "three_thing").is_none());
    }

    #[test]
    fn rejects_other_major_versions() {
        let err = Schemas::from_slice(
            br#"{"format_version":"2.0","provider_schemas":{}}"#,
            Path::new("test"),
        )
        .unwrap_err();
        assert!(matches!(err, Error::UnsupportedSchemaVersion(v) if v == "2.0"));
    }

    #[test]
    fn rejects_json_that_is_not_a_schema() {
        let plan = br#"{"format_version":"1.2","resource_changes":[]}"#;
        let err = Schemas::from_slice(plan, Path::new("test")).unwrap_err();
        assert!(matches!(err, Error::ParseSchema { .. }));
    }
}
