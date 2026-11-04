use serde::{Deserialize, Serialize};

use crate::types::{ASSIGNEE_SPECIALS, IdentityValue};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TeamMemberIdentityRow {
    pub identity: Option<TeamMemberIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberIdentity {
    pub display_name: Option<String>,
    pub unique_name: Option<String>,
    pub id: Option<String>,
    pub is_container: Option<bool>,
    pub is_deleted_in_origin: Option<bool>,
}

pub fn map_team_member_identities(rows: &[TeamMemberIdentityRow]) -> Vec<IdentityValue> {
    let reserved: std::collections::HashSet<&str> = ASSIGNEE_SPECIALS.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let mut members = Vec::new();
    for row in rows {
        let Some(identity) = &row.identity else {
            continue;
        };
        if identity.is_container == Some(true) || identity.is_deleted_in_origin == Some(true) {
            continue;
        }
        let unique_name = identity
            .unique_name
            .as_deref()
            .or(identity.display_name.as_deref())
            .unwrap_or("")
            .trim()
            .to_string();
        let display_name = identity
            .display_name
            .as_deref()
            .or(identity.unique_name.as_deref())
            .unwrap_or("")
            .trim()
            .to_string();
        if unique_name.is_empty()
            || display_name.is_empty()
            || reserved.contains(unique_name.to_lowercase().as_str())
        {
            continue;
        }
        let key = unique_name.to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        members.push(IdentityValue {
            display_name,
            unique_name,
            id: identity.id.clone(),
        });
    }
    members.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    members
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(display: &str, unique: &str, id: Option<&str>) -> TeamMemberIdentityRow {
        TeamMemberIdentityRow {
            identity: Some(TeamMemberIdentity {
                display_name: Some(display.into()),
                unique_name: Some(unique.into()),
                id: id.map(ToOwned::to_owned),
                is_container: None,
                is_deleted_in_origin: None,
            }),
        }
    }

    #[test]
    fn keeps_people_and_skips_containers_deleted_and_reserved_names() {
        let rows = vec![
            person("Grace Hopper", "grace@contoso.com", Some("g1")),
            person("Ada Lovelace", "ada@contoso.com", Some("a1")),
            TeamMemberIdentityRow {
                identity: Some(TeamMemberIdentity {
                    display_name: Some("Platform".into()),
                    unique_name: Some("vstfs://".into()),
                    id: None,
                    is_container: Some(true),
                    is_deleted_in_origin: None,
                }),
            },
            TeamMemberIdentityRow {
                identity: Some(TeamMemberIdentity {
                    display_name: Some("Gone".into()),
                    unique_name: Some("gone@contoso.com".into()),
                    id: None,
                    is_container: None,
                    is_deleted_in_origin: Some(true),
                }),
            },
            person("Me", "me", None),
            person("Ada Lovelace", "ada@contoso.com", None),
        ];
        assert_eq!(
            map_team_member_identities(&rows),
            vec![
                IdentityValue {
                    display_name: "Ada Lovelace".into(),
                    unique_name: "ada@contoso.com".into(),
                    id: Some("a1".into()),
                },
                IdentityValue {
                    display_name: "Grace Hopper".into(),
                    unique_name: "grace@contoso.com".into(),
                    id: Some("g1".into()),
                },
            ]
        );
    }
}
