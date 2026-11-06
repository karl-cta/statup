//! Who sees a service, and the set of services one viewer may see.

use std::str::FromStr;

/// What a service shows to visitors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "TEXT", rename_all = "lowercase")]
pub enum Visibility {
    /// Anyone, signed in or not.
    Everyone,
    /// Signed-in members only.
    Members,
    /// Members of the chosen groups only.
    Groups,
}

impl Visibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Everyone => "everyone",
            Self::Members => "members",
            Self::Groups => "groups",
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Everyone => "services.visibility_everyone",
            Self::Members => "services.visibility_members",
            Self::Groups => "services.visibility_groups",
        }
    }
}

/// A visibility name that is not `everyone`, `members` or `groups`.
#[derive(Debug, PartialEq, Eq)]
pub struct UnknownVisibility;

impl FromStr for Visibility {
    type Err = UnknownVisibility;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [Self::Everyone, Self::Members, Self::Groups]
            .into_iter()
            .find(|mode| mode.as_str() == s)
            .ok_or(UnknownVisibility)
    }
}

/// Who a restricted service or folder is opened to: groups and single members.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grants<'a> {
    pub groups: &'a [i64],
    pub members: &'a [i64],
}

/// The services one viewer may see.
///
/// There is deliberately no `Default`: a query that forgets to take a scope
/// must fail to compile rather than quietly show every service.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Nothing is hidden from this viewer.
    All,
    /// Only these service ids, sorted and without duplicates.
    Visible(Vec<i64>),
}

impl Scope {
    pub fn visible(mut ids: Vec<i64>) -> Self {
        ids.sort_unstable();
        ids.dedup();
        Self::Visible(ids)
    }

    pub fn sees_service(&self, id: i64) -> bool {
        match self {
            Self::All => true,
            Self::Visible(ids) => ids.binary_search(&id).is_ok(),
        }
    }

    /// An event naming no service is seen by everyone; otherwise it takes
    /// one visible service to see it.
    pub fn sees_event(&self, service_ids: &[i64]) -> bool {
        match self {
            Self::All => true,
            Self::Visible(_) => {
                service_ids.is_empty() || service_ids.iter().any(|id| self.sees_service(*id))
            }
        }
    }

    /// The visible ids as a JSON array to bind to `json_each`, or `None`
    /// when nothing is hidden and the query needs no filter.
    pub fn json(&self) -> Option<String> {
        match self {
            Self::All => None,
            Self::Visible(ids) => Some(serde_json::to_string(ids).unwrap_or_default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_visibility_parses_from_its_name() {
        for mode in [
            Visibility::Everyone,
            Visibility::Members,
            Visibility::Groups,
        ] {
            assert_eq!(mode.as_str().parse::<Visibility>(), Ok(mode));
        }
        assert!("Members".parse::<Visibility>().is_err());
        assert!("".parse::<Visibility>().is_err());
    }

    #[test]
    fn visible_sorts_and_removes_duplicates() {
        assert_eq!(
            Scope::visible(vec![3, 1, 3, 2]),
            Scope::Visible(vec![1, 2, 3])
        );
    }

    #[test]
    fn a_scope_sees_the_services_it_lists() {
        let scope = Scope::visible(vec![5, 2]);
        assert!(scope.sees_service(2));
        assert!(scope.sees_service(5));
        assert!(!scope.sees_service(3));
        assert!(Scope::All.sees_service(3));
        assert!(!Scope::visible(Vec::new()).sees_service(1));
    }

    #[test]
    fn an_event_is_seen_when_it_names_no_service_or_a_visible_one() {
        let scope = Scope::visible(vec![2, 4]);
        assert!(scope.sees_event(&[]));
        assert!(scope.sees_event(&[1, 4]));
        assert!(!scope.sees_event(&[1, 3]));
        assert!(Scope::All.sees_event(&[1, 3]));
        assert!(Scope::visible(Vec::new()).sees_event(&[]));
    }

    #[test]
    fn json_is_absent_for_all_and_a_sorted_array_otherwise() {
        assert_eq!(Scope::All.json(), None);
        assert_eq!(
            Scope::visible(vec![9, 4, 9]).json().as_deref(),
            Some("[4,9]")
        );
        assert_eq!(Scope::visible(Vec::new()).json().as_deref(), Some("[]"));
    }
}
