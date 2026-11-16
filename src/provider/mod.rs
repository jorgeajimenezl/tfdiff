//! Knowledge about specific Terraform providers.
//!
//! The diff itself is provider-agnostic. Whatever depends on how one provider
//! behaves (which values it compares without regard to case, for instance)
//! lives behind [`Provider`].
//!
//! To add a provider: create a module with a type implementing [`Provider`]
//! and add it to [`registry`].

mod azure;

/// What tfdiff knows about one provider, or a family of related providers.
pub trait Provider: Sync {
    /// Short name, for diagnostics and tests.
    fn name(&self) -> &'static str;

    /// Whether a resource belongs to this provider. `provider_name` is the
    /// fully qualified name from the plan, when the plan has one.
    fn owns(&self, provider_name: Option<&str>, resource_type: &str) -> bool;

    /// True when two different strings denote the same thing to the provider's
    /// API because they only differ in letter case.
    fn case_insensitive(&self, before: &str, after: &str) -> bool;
}

/// Every known provider.
pub fn registry() -> &'static [&'static dyn Provider] {
    &[&azure::Azure]
}

/// The provider that owns a resource, if tfdiff knows it.
pub fn find(provider_name: Option<&str>, resource_type: &str) -> Option<&'static dyn Provider> {
    registry()
        .iter()
        .copied()
        .find(|provider| provider.owns(provider_name, resource_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names_are_unique() {
        let mut names: Vec<_> = registry().iter().map(|p| p.name()).collect();
        names.sort_unstable();
        let total = names.len();
        names.dedup();
        assert_eq!(names.len(), total);
    }

    #[test]
    fn unknown_providers_have_no_quirks() {
        assert!(find(Some("terraform.io/builtin/terraform"), "terraform_data").is_none());
        assert!(find(None, "aws_instance").is_none());
    }
}
