//! `--expect` gates for pull requests that must not change infrastructure.

use clap::ValueEnum;
use serde::{Serialize, Serializer};

use crate::labels;
use crate::report::{Action, Class, Report, ResourceReport};

/// What the plan is expected to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Expect {
    /// Nothing is destroyed or recreated.
    NoDestroy,
    /// Resources may change address, nothing else.
    OnlyMoves,
    /// The plan is empty: no changes and no moves.
    NoChanges,
}

impl Expect {
    /// The name used on the command line.
    pub fn name(self) -> &'static str {
        match self {
            Self::NoDestroy => "no-destroy",
            Self::OnlyMoves => "only-moves",
            Self::NoChanges => "no-changes",
        }
    }
}

impl Serialize for Expect {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The expectation holds.
    Upheld,
    /// The plan contradicts the expectation.
    Breached,
    /// Nothing contradicts the expectation, but it cannot be proven either:
    /// some values are only known after apply, or an update is planned whose
    /// differences are only in letter case.
    Undetermined,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upheld => labels::UPHELD,
            Self::Breached => labels::BREACHED,
            Self::Undetermined => labels::UNDETERMINED,
        }
    }
}

impl Serialize for Verdict {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GateResult {
    pub expect: Expect,
    pub verdict: Verdict,
    /// Addresses responsible for a breached or undetermined verdict.
    pub offenders: Vec<String>,
}

/// How one resource bears on an expectation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finding {
    Fine,
    Unprovable,
    Breach,
}

/// Checks `report` against `expect`.
pub fn evaluate(expect: Expect, report: &Report) -> GateResult {
    let findings: Vec<(Finding, &str)> = report
        .resources
        .iter()
        .map(|resource| (judge(expect, resource), resource.address.as_str()))
        .collect();
    let offenders = |wanted: Finding| -> Vec<String> {
        findings
            .iter()
            .filter(|(finding, _)| *finding == wanted)
            .map(|(_, address)| (*address).to_owned())
            .collect()
    };

    let breaches = offenders(Finding::Breach);
    let (verdict, offenders) = if breaches.is_empty() {
        let unprovable = offenders(Finding::Unprovable);
        if unprovable.is_empty() {
            (Verdict::Upheld, unprovable)
        } else {
            (Verdict::Undetermined, unprovable)
        }
    } else {
        (Verdict::Breached, breaches)
    };
    GateResult {
        expect,
        verdict,
        offenders,
    }
}

fn judge(expect: Expect, resource: &ResourceReport) -> Finding {
    match expect {
        // Destruction is decided at plan time, so unknown values never matter here.
        Expect::NoDestroy => match resource.action {
            Action::Teardown | Action::Respawn => Finding::Breach,
            _ => Finding::Fine,
        },
        Expect::OnlyMoves => attribute_finding(resource),
        Expect::NoChanges => {
            if resource.previous_address.is_some() {
                Finding::Breach
            } else {
                attribute_finding(resource)
            }
        }
    }
}

/// Whether the resource itself changes, ignoring its address.
fn attribute_finding(resource: &ResourceReport) -> Finding {
    // Unknown values may turn out equal, and a re-cased value is the same to
    // the provider; but Terraform still plans an update, so neither is proof.
    let nothing_effective = !resource.changes.is_empty()
        && resource
            .changes
            .iter()
            .all(|change| matches!(change.class, Class::Undetermined | Class::Recased));
    match resource.action {
        Action::Steady if resource.changes.is_empty() => Finding::Fine,
        Action::Steady | Action::Mutate if nothing_effective => Finding::Unprovable,
        _ => Finding::Breach,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{AttrChange, Op, Totals};

    fn resource(action: Action, moved: bool, classes: &[Class]) -> ResourceReport {
        ResourceReport {
            address: "a.b".into(),
            previous_address: moved.then(|| "a.old".into()),
            resource_type: "a".into(),
            action,
            forced_by: Vec::new(),
            changes: classes
                .iter()
                .map(|&class| AttrChange {
                    path: "p".into(),
                    class,
                    op: Op::Altered,
                    forcing: false,
                    before: None,
                    after: None,
                })
                .collect(),
        }
    }

    fn verdict(expect: Expect, resources: Vec<ResourceReport>) -> Verdict {
        verdict_with_drift(expect, resources, Vec::new())
    }

    fn verdict_with_drift(
        expect: Expect,
        resources: Vec<ResourceReport>,
        drift: Vec<ResourceReport>,
    ) -> Verdict {
        let report = Report {
            totals: Totals::default(),
            resources,
            drift,
            gate: None,
        };
        evaluate(expect, &report).verdict
    }

    #[test]
    fn recased_only_updates_cannot_be_proven() {
        for expect in [Expect::OnlyMoves, Expect::NoChanges] {
            assert_eq!(
                verdict(
                    expect,
                    vec![resource(Action::Mutate, false, &[Class::Recased])]
                ),
                Verdict::Undetermined
            );
            assert_eq!(
                verdict(
                    expect,
                    vec![resource(
                        Action::Mutate,
                        false,
                        &[Class::Recased, Class::Undetermined]
                    )]
                ),
                Verdict::Undetermined
            );
            assert_eq!(
                verdict(
                    expect,
                    vec![resource(
                        Action::Mutate,
                        false,
                        &[Class::Recased, Class::Effective]
                    )]
                ),
                Verdict::Breached
            );
            // A recreation is a recreation, whatever forced it.
            assert_eq!(
                verdict(
                    expect,
                    vec![resource(Action::Respawn, false, &[Class::Recased])]
                ),
                Verdict::Breached
            );
        }
        assert_eq!(
            verdict(
                Expect::NoDestroy,
                vec![resource(Action::Respawn, false, &[Class::Recased])]
            ),
            Verdict::Breached
        );
    }

    #[test]
    fn drift_never_affects_a_gate() {
        for expect in [Expect::NoDestroy, Expect::OnlyMoves, Expect::NoChanges] {
            let drift = vec![
                resource(Action::Teardown, false, &[]),
                resource(Action::Mutate, false, &[Class::Effective]),
            ];
            assert_eq!(
                verdict_with_drift(expect, Vec::new(), drift),
                Verdict::Upheld
            );
        }
    }

    #[test]
    fn empty_plan_upholds_everything() {
        for expect in [Expect::NoDestroy, Expect::OnlyMoves, Expect::NoChanges] {
            assert_eq!(verdict(expect, Vec::new()), Verdict::Upheld);
        }
    }

    #[test]
    fn no_destroy() {
        use Expect::NoDestroy;
        let real = [Class::Effective];
        assert_eq!(
            verdict(NoDestroy, vec![resource(Action::Mutate, false, &real)]),
            Verdict::Upheld
        );
        assert_eq!(
            verdict(NoDestroy, vec![resource(Action::Provision, false, &[])]),
            Verdict::Upheld
        );
        assert_eq!(
            verdict(NoDestroy, vec![resource(Action::Teardown, false, &[])]),
            Verdict::Breached
        );
        assert_eq!(
            verdict(NoDestroy, vec![resource(Action::Respawn, false, &real)]),
            Verdict::Breached
        );
    }

    #[test]
    fn only_moves() {
        use Expect::OnlyMoves;
        assert_eq!(
            verdict(OnlyMoves, vec![resource(Action::Steady, true, &[])]),
            Verdict::Upheld
        );
        assert_eq!(
            verdict(
                OnlyMoves,
                vec![resource(Action::Mutate, true, &[Class::Effective])]
            ),
            Verdict::Breached
        );
        assert_eq!(
            verdict(OnlyMoves, vec![resource(Action::Provision, false, &[])]),
            Verdict::Breached
        );
        assert_eq!(
            verdict(
                OnlyMoves,
                vec![resource(Action::Mutate, false, &[Class::Undetermined])]
            ),
            Verdict::Undetermined
        );
    }

    #[test]
    fn no_changes() {
        use Expect::NoChanges;
        assert_eq!(
            verdict(NoChanges, vec![resource(Action::Steady, true, &[])]),
            Verdict::Breached
        );
        assert_eq!(
            verdict(
                NoChanges,
                vec![resource(Action::Mutate, false, &[Class::Undetermined])]
            ),
            Verdict::Undetermined
        );
        assert_eq!(
            verdict(
                NoChanges,
                vec![resource(
                    Action::Mutate,
                    false,
                    &[Class::Undetermined, Class::Effective]
                )]
            ),
            Verdict::Breached
        );
        // An in-place update that shows no attribute difference is still not an empty plan.
        assert_eq!(
            verdict(NoChanges, vec![resource(Action::Mutate, false, &[])]),
            Verdict::Breached
        );
    }

    #[test]
    fn a_breach_outranks_an_unprovable_resource() {
        let result = verdict(
            Expect::NoChanges,
            vec![
                resource(Action::Mutate, false, &[Class::Undetermined]),
                resource(Action::Teardown, false, &[]),
            ],
        );
        assert_eq!(result, Verdict::Breached);
    }
}
