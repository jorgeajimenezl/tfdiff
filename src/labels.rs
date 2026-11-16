//! Every word that tfdiff prints as a label lives here.
//!
//! CI systems mask secret values wherever they appear in a log. A secret that is
//! literally `replace` or `create` would then mangle any output that uses those
//! words, so labels are deliberately uncommon and must not even contain a common
//! word. The text format goes further and uses sigils instead of words.

pub const PROVISION: &str = "provision";
pub const TEARDOWN: &str = "teardown";
pub const RESPAWN: &str = "respawn";
pub const MUTATE: &str = "mutate";
pub const DETACH: &str = "detach";
pub const REHOME: &str = "rehome";
pub const STEADY: &str = "steady";

pub const EFFECTIVE: &str = "effective";
pub const UNDETERMINED: &str = "undetermined";
pub const RECASED: &str = "recased";

pub const DRIFTED: &str = "drifted";

pub const GAINED: &str = "gained";
pub const DROPPED: &str = "dropped";
pub const ALTERED: &str = "altered";
pub const SHUFFLED: &str = "shuffled";

pub const UPHELD: &str = "upheld";
pub const BREACHED: &str = "breached";

pub const MASKED: &str = "masked";

/// All label words, for the denylist test and for documentation.
pub const ALL: &[&str] = &[
    PROVISION,
    TEARDOWN,
    RESPAWN,
    MUTATE,
    DETACH,
    REHOME,
    STEADY,
    EFFECTIVE,
    UNDETERMINED,
    RECASED,
    DRIFTED,
    GAINED,
    DROPPED,
    ALTERED,
    SHUFFLED,
    UPHELD,
    BREACHED,
    MASKED,
];

#[cfg(test)]
mod tests {
    use super::ALL;

    /// Words likely to be somebody's (bad) secret, test value or environment name.
    const COMMON: &[&str] = &[
        "replace", "create", "delete", "update", "change", "destroy", "remove", "add", "move",
        "name", "id", "key", "value", "type", "true", "false", "null", "none", "test", "prod",
        "dev", "admin", "user", "password", "secret", "token", "default", "order", "build", "read",
        "write", "error", "new", "old",
    ];

    #[test]
    fn labels_do_not_contain_common_words() {
        for label in ALL {
            for word in COMMON {
                assert!(
                    !label.contains(word),
                    "label {label:?} contains the common word {word:?}"
                );
            }
        }
    }

    #[test]
    fn labels_are_unique() {
        let mut sorted = ALL.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ALL.len());
    }
}
