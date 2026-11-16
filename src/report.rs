//! The redaction-safe result of a diff. This is all a renderer ever sees.

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::gate::GateResult;
use crate::labels;
use crate::redact::Shown;

#[derive(Debug, Serialize)]
pub struct Report {
    pub totals: Totals,
    /// Resources that change or move, in plan order. Untouched resources are only counted.
    pub resources: Vec<ResourceReport>,
    /// Outcome of `--expect`, when requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateResult>,
}

#[derive(Debug, Serialize)]
pub struct ResourceReport {
    pub address: String,
    /// Set when the resource was moved (`moved` block or `terraform state mv`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_address: Option<String>,
    #[serde(rename = "type")]
    pub resource_type: String,
    pub action: Action,
    /// Attribute paths that force the recreation.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forced_by: Vec<String>,
    pub changes: Vec<AttrChange>,
}

impl ResourceReport {
    /// A move with nothing else going on.
    pub fn is_pure_move(&self) -> bool {
        self.action == Action::Steady && self.previous_address.is_some() && self.changes.is_empty()
    }
}

#[derive(Debug, Serialize)]
pub struct AttrChange {
    pub path: String,
    pub class: Class,
    pub op: Op,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Shown>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Shown>,
}

/// What happens to a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    /// Created.
    Provision,
    /// Destroyed.
    Teardown,
    /// Destroyed and created again.
    Respawn,
    /// Updated in place.
    Mutate,
    /// Removed from state without being destroyed.
    Detach,
    /// No infrastructure change.
    Steady,
}

impl Action {
    /// Maps the plan's `actions` array.
    pub fn from_plan(actions: &[String]) -> Self {
        let actions: Vec<&str> = actions.iter().map(String::as_str).collect();
        match actions.as_slice() {
            ["no-op" | "read"] | [] => Self::Steady,
            ["create"] => Self::Provision,
            ["delete"] => Self::Teardown,
            ["delete", "create"] | ["create", "delete"] => Self::Respawn,
            [.., "forget"] | ["forget", ..] => Self::Detach,
            // "update", and anything a future Terraform adds: show the attribute diff.
            _ => Self::Mutate,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Provision => labels::PROVISION,
            Self::Teardown => labels::TEARDOWN,
            Self::Respawn => labels::RESPAWN,
            Self::Mutate => labels::MUTATE,
            Self::Detach => labels::DETACH,
            Self::Steady => labels::STEADY,
        }
    }
}

/// How much weight an attribute difference carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Class {
    /// A real difference known at plan time.
    Effective,
    /// The new value is only known after apply.
    Undetermined,
}

impl Class {
    pub fn label(self) -> &'static str {
        match self {
            Self::Effective => labels::EFFECTIVE,
            Self::Undetermined => labels::UNDETERMINED,
        }
    }
}

/// The shape of an attribute difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
    /// Present only after.
    Gained,
    /// Present only before.
    Dropped,
    /// Present on both sides with different values.
    Altered,
    /// Same list elements in a different order.
    Shuffled,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Self::Gained => labels::GAINED,
            Self::Dropped => labels::DROPPED,
            Self::Altered => labels::ALTERED,
            Self::Shuffled => labels::SHUFFLED,
        }
    }
}

macro_rules! serialize_as_label {
    ($($ty:ty),+) => {$(
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.label())
            }
        }
    )+};
}
serialize_as_label!(Action, Class, Op);

/// Resource counts for the whole plan.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub provision: usize,
    pub teardown: usize,
    pub respawn: usize,
    pub mutate: usize,
    pub detach: usize,
    /// Moved resources; these are also counted under their action.
    pub rehome: usize,
    pub steady: usize,
}

impl Totals {
    pub(crate) fn count(&mut self, action: Action, moved: bool) {
        match action {
            Action::Provision => self.provision += 1,
            Action::Teardown => self.teardown += 1,
            Action::Respawn => self.respawn += 1,
            Action::Mutate => self.mutate += 1,
            Action::Detach => self.detach += 1,
            Action::Steady => self.steady += 1,
        }
        if moved {
            self.rehome += 1;
        }
    }
}

impl Serialize for Totals {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entries = [
            (labels::PROVISION, self.provision),
            (labels::TEARDOWN, self.teardown),
            (labels::RESPAWN, self.respawn),
            (labels::MUTATE, self.mutate),
            (labels::DETACH, self.detach),
            (labels::REHOME, self.rehome),
            (labels::STEADY, self.steady),
        ];
        let mut map = serializer.serialize_map(Some(entries.len()))?;
        for (label, count) in entries {
            map.serialize_entry(label, &count)?;
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(actions: &[&str]) -> Action {
        Action::from_plan(&actions.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn plan_actions_map_to_report_actions() {
        assert_eq!(action(&["no-op"]), Action::Steady);
        assert_eq!(action(&["read"]), Action::Steady);
        assert_eq!(action(&["create"]), Action::Provision);
        assert_eq!(action(&["delete"]), Action::Teardown);
        assert_eq!(action(&["delete", "create"]), Action::Respawn);
        assert_eq!(action(&["create", "delete"]), Action::Respawn);
        assert_eq!(action(&["update"]), Action::Mutate);
        assert_eq!(action(&["forget"]), Action::Detach);
        assert_eq!(action(&["create", "forget"]), Action::Detach);
        assert_eq!(action(&["something-new"]), Action::Mutate);
    }
}
