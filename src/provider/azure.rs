//! Azure providers (`azurerm`, `azapi`).

use super::Provider;

pub struct Azure;

/// Provider names without the registry host.
const PROVIDERS: &[&str] = &["hashicorp/azurerm", "azure/azapi"];
/// Used when the plan does not name the provider.
const TYPE_PREFIXES: &[&str] = &["azurerm_", "azapi_"];
/// How an Azure Resource Manager ID starts.
const ID_PREFIXES: &[&str] = &["/subscriptions/", "/providers/"];

impl Provider for Azure {
    fn name(&self) -> &'static str {
        "azure"
    }

    fn owns(&self, provider_name: Option<&str>, resource_type: &str) -> bool {
        match provider_name {
            Some(name) => PROVIDERS.iter().any(|known| {
                name.strip_suffix(known)
                    .is_some_and(|host| host.is_empty() || host.ends_with('/'))
            }),
            None => TYPE_PREFIXES
                .iter()
                .any(|prefix| resource_type.starts_with(prefix)),
        }
    }

    /// Resource Manager treats resource IDs as case-insensitive, and its APIs
    /// do not always return them in the case they were written in.
    fn case_insensitive(&self, before: &str, after: &str) -> bool {
        before != after
            && is_resource_id(before)
            && is_resource_id(after)
            && before.eq_ignore_ascii_case(after)
    }
}

fn is_resource_id(text: &str) -> bool {
    ID_PREFIXES.iter().any(|prefix| {
        text.get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owns_azure_resources() {
        assert!(Azure.owns(
            Some("registry.terraform.io/hashicorp/azurerm"),
            "azurerm_subnet"
        ));
        assert!(Azure.owns(Some("registry.opentofu.org/azure/azapi"), "azapi_resource"));
        assert!(Azure.owns(None, "azurerm_subnet"));
        assert!(!Azure.owns(Some("registry.terraform.io/hashicorp/aws"), "aws_vpc"));
        // The plan's provider wins over what the type looks like.
        assert!(!Azure.owns(
            Some("registry.terraform.io/example/notazurerm"),
            "azurerm_subnet"
        ));
        assert!(!Azure.owns(None, "terraform_data"));
    }

    #[test]
    fn resource_ids_that_differ_only_in_case() {
        let lower = "/subscriptions/0000/resourcegroups/rg-net";
        let mixed = "/subscriptions/0000/resourceGroups/RG-net";
        assert!(Azure.case_insensitive(lower, mixed));
        assert!(Azure.case_insensitive(
            "/providers/Microsoft.Management/managementGroups/a",
            "/PROVIDERS/microsoft.management/managementgroups/A"
        ));
        assert!(!Azure.case_insensitive(lower, lower));
        assert!(!Azure.case_insensitive(lower, "/subscriptions/0000/resourceGroups/rg-app"));
    }

    #[test]
    fn other_strings_keep_their_case() {
        assert!(!Azure.case_insensitive("Prod", "prod"));
        assert!(!Azure.case_insensitive("westeurope", "WestEurope"));
        // Not ASCII: `get(..n)` must not split a character.
        assert!(!Azure.case_insensitive("/subscriptionß/é", "/SUBSCRIPTIONß/É"));
    }
}
