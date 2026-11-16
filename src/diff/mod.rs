//! Compares `before` and `after` of every resource change and produces a
//! redaction-safe [`Report`].

pub mod list;
pub mod path;

use std::collections::BTreeSet;

use serde_json::Value;

use crate::config::Config;
use crate::plan::{Plan, ResourceChange};
use crate::redact::{Redactor, Shown};
use crate::report::{Action, AttrChange, Class, Op, Report, ResourceReport, Totals};
use crate::schema::{Schemas, Shape};

use self::list::{Matching, match_keyed, match_positional, unique_keys};
use self::path::{Path, Segment};

static NULL: Value = Value::Null;

/// Builds the report for a whole plan.
///
/// List elements are paired by the key configured for the list, else by what
/// the provider schema says (key attribute, set semantics), else by position.
pub fn diff_plan(plan: &Plan, config: &Config, schemas: Option<&Schemas>) -> Report {
    let redactor = Redactor::from_plan(plan);
    let mut totals = Totals::default();
    let mut resources = Vec::new();

    for resource in &plan.resource_changes {
        // Data sources are re-read on every plan; they are not infrastructure changes.
        if resource.mode == "data" {
            continue;
        }
        let action = Action::from_plan(&resource.change.actions);
        let moved = resource.previous_address.is_some();
        totals.count(action, moved);
        if action == Action::Steady && !moved {
            continue;
        }
        resources.push(diff_resource(resource, action, &redactor, config, schemas));
    }

    Report {
        totals,
        resources,
        gate: None,
    }
}

fn diff_resource(
    resource: &ResourceChange,
    action: Action,
    redactor: &Redactor,
    config: &Config,
    schemas: Option<&Schemas>,
) -> ResourceReport {
    let change = &resource.change;
    let mut changes = Vec::new();

    // Creations and removals are reported as one line; listing every attribute
    // of a new resource is not a diff.
    if matches!(action, Action::Mutate | Action::Respawn | Action::Steady) {
        let mut walker = Walker {
            redactor,
            config,
            resource_type: &resource.resource_type,
            out: &mut changes,
        };
        walker.walk(
            &mut Path::default(),
            Sides {
                before: Some(change.before.expose()),
                after: Some(change.after.expose()),
                before_mask: change.before_sensitive.expose(),
                after_mask: change.after_sensitive.expose(),
                unknown: change.after_unknown.expose(),
                shape: schemas.and_then(|schemas| {
                    schemas.resource(resource.provider_name.as_deref(), &resource.resource_type)
                }),
            },
        );
    }

    ResourceReport {
        address: resource.address.clone(),
        previous_address: resource.previous_address.clone(),
        resource_type: resource.resource_type.clone(),
        action,
        forced_by: change
            .replace_paths
            .iter()
            .map(|segments| replace_path(segments).to_string())
            .collect(),
        changes,
    }
}

/// Converts a `replace_paths` entry (attribute names and indexes) into a [`Path`].
fn replace_path(segments: &[Value]) -> Path {
    let mut path = Path::default();
    for segment in segments {
        match segment {
            Value::String(name) => path.push(Segment::Attr(name.clone())),
            Value::Number(number) => {
                let index = number.as_u64().and_then(|n| usize::try_from(n).ok());
                path.push(Segment::Index(index.unwrap_or_default()));
            }
            _ => {}
        }
    }
    path
}

/// The same location in every tree of a resource change.
#[derive(Clone, Copy)]
struct Sides<'a> {
    before: Option<&'a Value>,
    after: Option<&'a Value>,
    before_mask: &'a Value,
    after_mask: &'a Value,
    unknown: &'a Value,
    /// What the provider schema says about this location, when known.
    shape: Option<&'a Shape>,
}

fn child<I: serde_json::value::Index>(value: &Value, index: I) -> &Value {
    value.get(index).unwrap_or(&NULL)
}

fn is_true(value: &Value) -> bool {
    value == &Value::Bool(true)
}

struct Walker<'a> {
    redactor: &'a Redactor,
    config: &'a Config,
    resource_type: &'a str,
    out: &'a mut Vec<AttrChange>,
}

impl Walker<'_> {
    fn walk(&mut self, path: &mut Path, at: Sides<'_>) {
        // Terraform writes absent attributes as null; treat both the same.
        let before = at.before.filter(|value| !value.is_null());
        let after = at.after.filter(|value| !value.is_null());

        if is_true(at.unknown) {
            let op = if before.is_some() {
                Op::Altered
            } else {
                Op::Gained
            };
            let shown =
                before.map(|value| self.redactor.show(path, value, at.before_mask, at.shape));
            self.emit(path, Class::Undetermined, op, shown, None);
            return;
        }

        if is_true(at.before_mask) || is_true(at.after_mask) {
            if before != after {
                self.emit(
                    path,
                    Class::Effective,
                    presence_op(before, after),
                    before.map(Redactor::hide),
                    after.map(Redactor::hide),
                );
            }
            return;
        }

        match (before, after) {
            (None, None) => {}
            (Some(Value::Object(old)), Some(Value::Object(new))) => {
                let mut keys: BTreeSet<&str> =
                    old.keys().chain(new.keys()).map(String::as_str).collect();
                // Unknown attributes are missing from `after` and may be missing from `before`.
                if let Value::Object(unknown) = at.unknown {
                    keys.extend(unknown.keys().map(String::as_str));
                }
                let is_map = at.shape.is_some_and(Shape::is_map);
                for key in keys {
                    // Map keys are data and can be secret; attribute names cannot.
                    if is_map && self.redactor.hides_key(key) {
                        path.push(Segment::Masked);
                    } else {
                        path.push(Segment::Attr(key.to_owned()));
                    }
                    self.walk(
                        path,
                        Sides {
                            before: old.get(key),
                            after: new.get(key),
                            before_mask: child(at.before_mask, key),
                            after_mask: child(at.after_mask, key),
                            unknown: child(at.unknown, key),
                            shape: at.shape.and_then(|shape| shape.child(key)),
                        },
                    );
                    path.pop();
                }
            }
            (Some(Value::Array(old)), Some(Value::Array(new))) => {
                self.walk_list(path, old, new, at);
            }
            (Some(old), Some(new)) if old == new => {}
            _ => {
                let shown_before =
                    before.map(|value| self.redactor.show(path, value, at.before_mask, at.shape));
                let shown_after =
                    after.map(|value| self.redactor.show(path, value, at.after_mask, at.shape));
                self.emit(
                    path,
                    Class::Effective,
                    presence_op(before, after),
                    shown_before,
                    shown_after,
                );
            }
        }
    }

    fn walk_list(&mut self, path: &mut Path, old: &[Value], new: &[Value], at: Sides<'_>) {
        let element = at.shape.and_then(Shape::element);
        let is_set = at.shape.is_some_and(Shape::is_set);
        // Config override, then the key the schema suggests.
        let key_attr = self
            .config
            .list_key(self.resource_type, &path.attr_path())
            .or_else(|| element.and_then(Shape::key_attr));
        let keyed = key_attr.and_then(|key_attr| {
            Some((
                key_attr,
                element_keys(old, key_attr)?,
                element_keys(new, key_attr)?,
            ))
        });

        let positions = |len: usize| (0..len).map(Segment::Index).collect::<Vec<_>>();
        let (matching, old_labels, new_labels) = if let Some((key_attr, old_keys, new_keys)) = keyed
        {
            let old_keys = unique_keys(&old_keys);
            let new_keys = unique_keys(&new_keys);
            let matching = match_keyed(&old_keys, &new_keys);
            (
                matching,
                self.labels(path, key_attr, old, old_keys, at.before_mask),
                self.labels(path, key_attr, new, new_keys, at.after_mask),
            )
        } else if is_set {
            // A set element has no identity but its whole value. The values
            // only drive the pairing; they never label a path.
            let matching = match_keyed(
                &unique_keys(&whole_values(old)),
                &unique_keys(&whole_values(new)),
            );
            (matching, positions(old.len()), positions(new.len()))
        } else {
            (
                match_positional(old.len(), new.len()),
                positions(old.len()),
                positions(new.len()),
            )
        };
        let Matching {
            pairs,
            removed,
            added,
            reordered,
        } = matching;

        for (old_index, new_index) in pairs {
            path.push(new_labels[new_index].clone());
            self.walk(
                path,
                Sides {
                    before: old.get(old_index),
                    after: new.get(new_index),
                    before_mask: child(at.before_mask, old_index),
                    after_mask: child(at.after_mask, new_index),
                    unknown: child(at.unknown, new_index),
                    shape: element,
                },
            );
            path.pop();
        }
        for old_index in removed {
            path.push(old_labels[old_index].clone());
            self.walk(
                path,
                Sides {
                    before: old.get(old_index),
                    after: None,
                    before_mask: child(at.before_mask, old_index),
                    after_mask: &NULL,
                    unknown: &NULL,
                    shape: element,
                },
            );
            path.pop();
        }
        for new_index in added {
            path.push(new_labels[new_index].clone());
            self.walk(
                path,
                Sides {
                    before: None,
                    after: new.get(new_index),
                    before_mask: &NULL,
                    after_mask: child(at.after_mask, new_index),
                    unknown: child(at.unknown, new_index),
                    shape: element,
                },
            );
            path.pop();
        }
        // The order of a set carries no meaning.
        if reordered && !is_set {
            self.emit(path, Class::Effective, Op::Shuffled, None, None);
        }
    }

    /// Path segments for the elements of one side of a keyed list. A key that
    /// would be redacted as a value is never printed in a path either; such
    /// elements, and plain scalars, are labelled by position.
    fn labels(
        &self,
        path: &mut Path,
        key_attr: &str,
        items: &[Value],
        keys: Vec<String>,
        mask: &Value,
    ) -> Vec<Segment> {
        path.push(Segment::Attr(key_attr.to_owned()));
        let labels = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| {
                let item = &items[index];
                let item_mask = child(mask, index);
                let printable = item.get(key_attr).is_some_and(|key_value| {
                    !is_true(item_mask)
                        && !self
                            .redactor
                            .show(path, key_value, child(item_mask, key_attr), None)
                            .is_masked()
                });
                if printable {
                    Segment::Key(key)
                } else {
                    Segment::Index(index)
                }
            })
            .collect();
        path.pop();
        labels
    }

    fn emit(
        &mut self,
        path: &Path,
        class: Class,
        op: Op,
        before: Option<Shown>,
        after: Option<Shown>,
    ) {
        self.out.push(AttrChange {
            path: path.to_string(),
            class,
            op,
            before,
            after,
        });
    }
}

fn presence_op(before: Option<&Value>, after: Option<&Value>) -> Op {
    match (before, after) {
        (None, Some(_)) => Op::Gained,
        (Some(_), None) => Op::Dropped,
        _ => Op::Altered,
    }
}

/// Identity of each element: the key attribute for objects, the value itself
/// for scalars. `None` when any element has no usable identity, in which case
/// the list is compared by position (or by whole value, for a set).
fn element_keys(items: &[Value], key_attr: &str) -> Option<Vec<String>> {
    items
        .iter()
        .map(|item| match item {
            Value::Object(fields) => match fields.get(key_attr)? {
                Value::String(key) => Some(key.clone()),
                Value::Number(key) => Some(key.to_string()),
                _ => None,
            },
            // The JSON form keeps the string "1" apart from the number 1.
            Value::String(_) | Value::Number(_) | Value::Bool(_) => Some(item.to_string()),
            Value::Null | Value::Array(_) => None,
        })
        .collect()
}

/// Canonical text of each element (`serde_json` keeps object keys sorted), so
/// equal values get equal keys.
fn whole_values(items: &[Value]) -> Vec<String> {
    items.iter().map(Value::to_string).collect()
}
