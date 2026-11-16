//! Property tests for key-based list matching.

use std::collections::BTreeSet;

use proptest::prelude::*;
use serde_json::{Value, json};
use tfdiff::config::Config;
use tfdiff::diff::diff_plan;
use tfdiff::diff::list::{match_keyed, match_positional, unique_keys};
use tfdiff::plan::Plan;
use tfdiff::report::{AttrChange, Op, Report};
use tfdiff::schema::Schemas;

/// Few distinct keys, so duplicates and literal `a#2` collisions are common.
fn key() -> impl Strategy<Value = String> {
    prop_oneof!["[ab]", "[ab]#[23]", "[a-d]{1,2}",]
}

fn keys() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(key(), 0..10)
}

fn element() -> impl Strategy<Value = Value> {
    (key(), 0..4i64, any::<bool>())
        .prop_map(|(name, port, on)| json!({"name": name, "port": port, "on": on}))
}

fn elements() -> impl Strategy<Value = Vec<Value>> {
    prop::collection::vec(element(), 0..8)
}

fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::from),
        (0..4i64).prop_map(Value::from),
        "[a-c]{0,2}".prop_map(Value::from),
    ];
    leaf.prop_recursive(4, 48, 5, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..5).prop_map(Value::from),
            prop::collection::btree_map(prop_oneof!["[a-c]", Just("name".to_owned())], inner, 0..5)
                .prop_map(|fields| Value::Object(fields.into_iter().collect())),
        ]
    })
}

fn report(change: &Value, config: &Config, schemas: Option<&Schemas>) -> Report {
    let bytes = serde_json::to_vec(&json!({
        "format_version": "1.2",
        "resource_changes": [{"address": "t.x", "mode": "managed", "type": "t", "change": change}],
    }))
    .unwrap();
    diff_plan(&Plan::from_slice(&bytes).unwrap(), config, schemas)
}

/// `rules` keyed by `name`, as a user would configure it.
fn keyed() -> Config {
    Config::parse("[list_keys]\n\"t.rules\" = \"name\"\n").unwrap()
}

/// Schema declaring `t.rules` as a set of dynamic values.
fn set_schema() -> Schemas {
    let bytes = serde_json::to_vec(&json!({
        "format_version": "1.0",
        "provider_schemas": {"p": {"resource_schemas": {"t": {"block": {"attributes": {
            "rules": {"type": ["set", "dynamic"]},
        }}}}}},
    }))
    .unwrap();
    Schemas::from_slice(&bytes, std::path::Path::new("test")).unwrap()
}

fn changes_with(
    before: &Value,
    after: &Value,
    config: &Config,
    schemas: Option<&Schemas>,
) -> Vec<AttrChange> {
    report(
        &json!({"actions": ["update"], "before": before, "after": after}),
        config,
        schemas,
    )
    .resources
    .pop()
    .map_or_else(Vec::new, |resource| resource.changes)
}

/// Changes with `rules` keyed by `name`.
fn changes(before: &Value, after: &Value) -> Vec<AttrChange> {
    changes_with(before, after, &keyed(), None)
}

proptest! {
    #[test]
    fn unique_keys_are_unique_and_keep_first_occurrences(keys in keys()) {
        let unique = unique_keys(&keys);
        prop_assert_eq!(unique.len(), keys.len());
        prop_assert_eq!(unique.iter().collect::<BTreeSet<_>>().len(), keys.len());
        let mut seen = BTreeSet::new();
        for (original, made_unique) in keys.iter().zip(&unique) {
            prop_assert!(made_unique.starts_with(original.as_str()));
            if seen.insert(original) && !unique.iter().any(|other| other != made_unique && other == original) {
                prop_assert_eq!(original, made_unique);
            }
        }
    }

    #[test]
    fn unique_keys_are_stable_under_appending(keys in keys(), extra in keys()) {
        let mut longer = keys.clone();
        longer.extend(extra);
        let short = unique_keys(&keys);
        prop_assert_eq!(&unique_keys(&longer)[..keys.len()], &short[..]);
    }

    #[test]
    fn matching_partitions_both_lists(before in keys(), after in keys()) {
        let before = unique_keys(&before);
        let after = unique_keys(&after);
        let matching = match_keyed(&before, &after);

        let mut before_seen: Vec<usize> = matching.pairs.iter().map(|pair| pair.0).chain(matching.removed.clone()).collect();
        let mut after_seen: Vec<usize> = matching.pairs.iter().map(|pair| pair.1).chain(matching.added.clone()).collect();
        before_seen.sort_unstable();
        after_seen.sort_unstable();
        prop_assert_eq!(before_seen, (0..before.len()).collect::<Vec<_>>());
        prop_assert_eq!(after_seen, (0..after.len()).collect::<Vec<_>>());

        for &(old, new) in &matching.pairs {
            prop_assert_eq!(&before[old], &after[new]);
        }
        for &old in &matching.removed {
            prop_assert!(!after.contains(&before[old]));
        }
        for &new in &matching.added {
            prop_assert!(!before.contains(&after[new]));
        }
    }

    #[test]
    fn matching_a_list_with_itself_is_the_identity(keys in keys()) {
        let keys = unique_keys(&keys);
        let matching = match_keyed(&keys, &keys);
        prop_assert_eq!(matching.pairs, (0..keys.len()).map(|i| (i, i)).collect::<Vec<_>>());
        prop_assert!(matching.removed.is_empty() && matching.added.is_empty() && !matching.reordered);
    }

    #[test]
    fn permuting_unique_keys_only_reorders(keys in prop::collection::btree_set(key(), 0..10), seed in any::<u64>()) {
        let before: Vec<String> = keys.into_iter().collect();
        let mut after = before.clone();
        // Deterministic shuffle driven by the seed.
        let mut state = seed;
        for i in (1..after.len()).rev() {
            state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let j = usize::try_from(state >> 33).unwrap() % (i + 1);
            after.swap(i, j);
        }
        let matching = match_keyed(&before, &after);
        prop_assert_eq!(matching.pairs.len(), before.len());
        prop_assert!(matching.removed.is_empty() && matching.added.is_empty());
        prop_assert_eq!(matching.reordered, before != after);
    }

    #[test]
    fn positional_matching_covers_both_lists(before in 0..12usize, after in 0..12usize) {
        let matching = match_positional(before, after);
        prop_assert_eq!(matching.pairs.len() + matching.removed.len(), before);
        prop_assert_eq!(matching.pairs.len() + matching.added.len(), after);
        prop_assert!(!matching.reordered);
    }

    #[test]
    fn a_value_never_differs_from_itself(value in any_json()) {
        prop_assert!(changes(&value, &value).is_empty());
    }

    #[test]
    fn rotating_a_keyed_list_reports_at_most_a_reorder(mut items in elements(), by in 0..8usize) {
        // Unique names, so rotation cannot re-pair duplicates with each other.
        let mut seen = BTreeSet::new();
        items.retain(|item| seen.insert(item["name"].as_str().unwrap().to_owned()));
        let mut rotated = items.clone();
        if !rotated.is_empty() {
            let by = by % rotated.len();
            rotated.rotate_left(by);
        }
        let found = changes(&json!({"rules": items}), &json!({"rules": rotated}));
        prop_assert!(found.len() <= 1, "{found:?}");
        prop_assert!(found.iter().all(|change| change.op == Op::Shuffled && change.path == "rules"));
        prop_assert_eq!(found.is_empty(), items == rotated);
    }

    #[test]
    fn inserting_one_element_is_one_addition(items in elements(), at in 0..9usize, port in 0..4i64) {
        let mut seen = BTreeSet::new();
        let mut items = items;
        items.retain(|item| seen.insert(item["name"].as_str().unwrap().to_owned()));
        let mut longer = items.clone();
        longer.insert(at.min(items.len()), json!({"name": "fresh-key", "port": port}));
        let found = changes(&json!({"rules": items}), &json!({"rules": longer}));
        prop_assert_eq!(found.len(), 1, "{:?}", found);
        prop_assert_eq!(found[0].op, Op::Gained);
        prop_assert_eq!(found[0].path.as_str(), "rules[fresh-key]");
    }

    #[test]
    fn without_config_or_schema_lists_are_positional(before in elements(), after in elements()) {
        let found = changes_with(&json!({"rules": before}), &json!({"rules": after}), &Config::default(), None);
        for change in &found {
            prop_assert_ne!(change.op, Op::Shuffled);
            // Only positions label elements: `rules[<digits>]`, never `rules[<name>]`.
            let label = change.path.trim_start_matches("rules[").split(']').next().unwrap();
            prop_assert!(label.parse::<usize>().is_ok(), "{}", change.path);
        }
        // Elements at the same position are compared with each other.
        let common = before.len().min(after.len());
        let differing = (0..common).filter(|&i| before[i] != after[i]).count();
        let touched: BTreeSet<&str> = found.iter().map(|change| change.path.split(']').next().unwrap()).collect();
        prop_assert_eq!(touched.len(), differing + before.len().abs_diff(after.len()));
    }

    #[test]
    fn permuting_a_schema_set_changes_nothing(items in prop::collection::vec(any_json(), 0..6), by in 0..6usize) {
        let schemas = set_schema();
        let mut rotated = items.clone();
        if !rotated.is_empty() {
            let by = by % rotated.len();
            rotated.rotate_left(by);
        }
        let found = changes_with(&json!({"rules": items}), &json!({"rules": rotated}), &Config::default(), Some(&schemas));
        prop_assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn adding_to_a_schema_set_is_one_addition(items in prop::collection::vec(any_json(), 0..6), at in 0..7usize) {
        let schemas = set_schema();
        let mut longer = items.clone();
        longer.insert(at.min(items.len()), json!({"fresh": "element"}));
        let found = changes_with(&json!({"rules": items}), &json!({"rules": longer}), &Config::default(), Some(&schemas));
        prop_assert_eq!(found.len(), 1, "{:?}", found);
        prop_assert_eq!(found[0].op, Op::Gained);
    }

    #[test]
    fn arbitrary_plans_never_panic(
        before in any_json(),
        after in any_json(),
        before_sensitive in any_json(),
        after_sensitive in any_json(),
        after_unknown in any_json(),
        actions in prop::sample::select(vec!["update", "create", "delete", "no-op"]),
    ) {
        let change = json!({
            "actions": [actions],
            "before": before,
            "after": after,
            "before_sensitive": before_sensitive,
            "after_sensitive": after_sensitive,
            "after_unknown": after_unknown,
        });
        let schemas = set_schema();
        for schemas in [None, Some(&schemas)] {
        let report = report(&change, &keyed(), schemas);
        for renderer in tfdiff::render::registry() {
            renderer.render(&report, &mut Vec::new()).unwrap();
        }
        }
    }
}
