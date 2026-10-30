use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacklogLevel {
    pub rank: Option<i64>,
    pub work_item_types: Option<Vec<BacklogType>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BacklogType {
    pub name: Option<String>,
}

fn type_names(level: &BacklogLevel) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for ty in level.work_item_types.iter().flatten() {
        if let Some(name) = ty.name.as_deref().filter(|name| !name.is_empty())
            && seen.insert(name.to_string())
        {
            names.push(name.to_string());
        }
    }
    names
}

pub fn top_backlog_types_from_levels(levels: &[BacklogLevel]) -> Vec<String> {
    let with_types: Vec<&BacklogLevel> = levels
        .iter()
        .filter(|level| !type_names(level).is_empty())
        .collect();
    if with_types.is_empty() {
        return Vec::new();
    }
    let ranked: Vec<&BacklogLevel> = with_types
        .iter()
        .copied()
        .filter(|level| level.rank.is_some())
        .collect();
    let top = if ranked.is_empty() {
        with_types[0]
    } else {
        ranked
            .into_iter()
            .max_by_key(|level| level.rank.unwrap_or(0))
            .unwrap_or(with_types[0])
    };
    type_names(top)
}

pub fn resolve_root_types(
    stored: Option<Option<Vec<String>>>,
    loaded_types: &[String],
    top_backlog_types: &[String],
) -> Option<Vec<String>> {
    let loaded = loaded_types.to_vec();
    match stored {
        Some(None) => None,
        None => seed_root_types(&loaded, top_backlog_types),
        Some(Some(stored)) if stored.is_empty() => Some(Vec::new()),
        Some(Some(stored)) => {
            let intersection: Vec<String> = loaded
                .iter()
                .filter(|ty| stored.iter().any(|item| item == *ty))
                .cloned()
                .collect();
            if intersection.is_empty() {
                seed_root_types(&loaded, top_backlog_types)
            } else {
                Some(intersection)
            }
        }
    }
}

fn seed_root_types(loaded: &[String], top_backlog_types: &[String]) -> Option<Vec<String>> {
    let seed: Vec<String> = top_backlog_types
        .iter()
        .filter(|ty| loaded.iter().any(|item| item == *ty))
        .cloned()
        .collect();
    if seed.is_empty() {
        if loaded.is_empty() {
            Some(Vec::new())
        } else {
            None
        }
    } else {
        Some(seed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(rank: Option<i64>, names: &[&str]) -> BacklogLevel {
        BacklogLevel {
            rank,
            work_item_types: Some(
                names
                    .iter()
                    .map(|name| BacklogType {
                        name: Some((*name).into()),
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn picks_the_highest_rank_when_ranks_are_present() {
        assert_eq!(
            top_backlog_types_from_levels(&[
                level(Some(10), &["Task"]),
                level(Some(30), &["Epic", "Portfolio Epic"]),
                level(Some(20), &["Feature"]),
            ]),
            vec!["Epic", "Portfolio Epic"]
        );
    }

    #[test]
    fn uses_the_first_level_when_ranks_are_missing() {
        assert_eq!(
            top_backlog_types_from_levels(&[level(None, &["Epic"]), level(None, &["Feature"])]),
            vec!["Epic"]
        );
    }

    #[test]
    fn missing_prefs_seeds_the_top_backlog_types_that_exist() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(
            resolve_root_types(None, &loaded, &["Epic".into()]),
            Some(vec!["Epic".into()])
        );
    }

    #[test]
    fn missing_prefs_and_no_top_backlog_means_every_loaded_type_is_a_root_type() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(resolve_root_types(None, &loaded, &[]), None);
    }

    #[test]
    fn empty_stored_list_is_hide_all() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(
            resolve_root_types(Some(Some(vec![])), &loaded, &["Epic".into()]),
            Some(vec![])
        );
    }

    #[test]
    fn null_stored_list_is_show_all() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(
            resolve_root_types(Some(None), &loaded, &["Epic".into()]),
            None
        );
    }

    #[test]
    fn intersects_stored_names_with_loaded_types() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(
            resolve_root_types(
                Some(Some(vec!["Epic".into(), "Technical Roadmap Item".into()])),
                &loaded,
                &["Epic".into()]
            ),
            Some(vec!["Epic".into()])
        );
    }

    #[test]
    fn empty_intersection_reseeds_from_the_top_backlog() {
        let loaded = vec!["Epic".into(), "Feature".into(), "Task".into()];
        assert_eq!(
            resolve_root_types(
                Some(Some(vec!["Technical Roadmap Item".into()])),
                &loaded,
                &["Epic".into()]
            ),
            Some(vec!["Epic".into()])
        );
    }
}
