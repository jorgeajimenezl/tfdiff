//! Matching list elements by key instead of by position.
//!
//! Terraform diffs lists positionally, so inserting one element at the front
//! makes every later element look changed. These pure functions pair elements
//! by a stable key so only genuine additions, removals and edits remain.

use std::collections::{HashMap, HashSet};

/// How the elements of a `before` list correspond to those of an `after` list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Matching {
    /// `(before index, after index)` of elements present on both sides, in `before` order.
    pub pairs: Vec<(usize, usize)>,
    /// `before` indexes with no counterpart.
    pub removed: Vec<usize>,
    /// `after` indexes with no counterpart.
    pub added: Vec<usize>,
    /// The surviving elements appear in a different relative order.
    pub reordered: bool,
}

/// Makes keys unique: the second `name` becomes `name#2`, the third `name#3`.
///
/// Order of appearance decides the suffix, so the n-th duplicate before lines
/// up with the n-th duplicate after. A suffix that would collide with a
/// literal key is skipped.
pub fn unique_keys<S: AsRef<str>>(keys: &[S]) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::with_capacity(keys.len());
    let mut next_suffix: HashMap<&str, usize> = HashMap::new();
    keys.iter()
        .map(|key| {
            let key = key.as_ref();
            let mut candidate = key.to_owned();
            while used.contains(&candidate) {
                let suffix = next_suffix.entry(key).or_insert(2);
                candidate = format!("{key}#{suffix}");
                *suffix += 1;
            }
            used.insert(candidate.clone());
            candidate
        })
        .collect()
}

/// Pairs elements with equal keys. Both slices must hold unique keys
/// (see [`unique_keys`]).
pub fn match_keyed<S: AsRef<str>>(before: &[S], after: &[S]) -> Matching {
    let after_index: HashMap<&str, usize> = after
        .iter()
        .enumerate()
        .map(|(index, key)| (key.as_ref(), index))
        .collect();

    let mut matching = Matching::default();
    let mut matched_after = vec![false; after.len()];
    for (index, key) in before.iter().enumerate() {
        match after_index.get(key.as_ref()) {
            Some(&counterpart) if !matched_after[counterpart] => {
                matched_after[counterpart] = true;
                matching.pairs.push((index, counterpart));
            }
            _ => matching.removed.push(index),
        }
    }
    matching.added = (0..after.len())
        .filter(|&index| !matched_after[index])
        .collect();
    matching.reordered = matching.pairs.windows(2).any(|pair| pair[0].1 > pair[1].1);
    matching
}

/// Pairs elements by position, for lists without a usable key.
pub fn match_positional(before_len: usize, after_len: usize) -> Matching {
    let common = before_len.min(after_len);
    Matching {
        pairs: (0..common).map(|index| (index, index)).collect(),
        removed: (common..before_len).collect(),
        added: (common..after_len).collect(),
        reordered: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicates_get_numbered_suffixes() {
        assert_eq!(
            unique_keys(&["web", "db", "web", "web"]),
            ["web", "db", "web#2", "web#3"]
        );
    }

    #[test]
    fn suffixes_avoid_literal_keys() {
        assert_eq!(unique_keys(&["a", "a#2", "a"]), ["a", "a#2", "a#3"]);
        assert_eq!(unique_keys(&["a", "a", "a#2"]), ["a", "a#2", "a#2#2"]);
    }

    #[test]
    fn insertion_at_the_front_is_a_single_addition() {
        let matching = match_keyed(&["ssh", "http"], &["dns", "ssh", "http"]);
        assert_eq!(matching.pairs, [(0, 1), (1, 2)]);
        assert_eq!(matching.added, [0]);
        assert_eq!(matching.removed, [] as [usize; 0]);
        assert!(!matching.reordered);
    }

    #[test]
    fn reorder_is_detected() {
        let matching = match_keyed(&["a", "b", "c"], &["c", "a", "b"]);
        assert_eq!(matching.pairs, [(0, 1), (1, 2), (2, 0)]);
        assert!(matching.reordered);
    }

    #[test]
    fn positional_pairs_then_tail() {
        let matching = match_positional(3, 1);
        assert_eq!(matching.pairs, [(0, 0)]);
        assert_eq!(matching.removed, [1, 2]);
        assert_eq!(matching.added, [] as [usize; 0]);
    }
}
