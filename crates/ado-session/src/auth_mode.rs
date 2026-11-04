const PLACEHOLDER_CLIENT_ID: &str = "00000000-0000-0000-0000-000000000000";

pub fn entra_client_id_from_env(env: &std::collections::HashMap<String, String>) -> Option<String> {
    let raw = env
        .get("ENTRA_CLIENT_ID")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    let raw = if raw.is_empty() {
        env.get("MAIN_VITE_ENTRA_CLIENT_ID")
            .map(String::as_str)
            .unwrap_or("")
            .trim()
    } else {
        raw
    };
    if raw.is_empty() || raw == PLACEHOLDER_CLIENT_ID {
        None
    } else {
        Some(raw.to_string())
    }
}

pub fn entra_client_id() -> Option<String> {
    let mut env = std::collections::HashMap::new();
    for key in ["ENTRA_CLIENT_ID", "MAIN_VITE_ENTRA_CLIENT_ID"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_string(), value);
        }
    }
    entra_client_id_from_env(&env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn missing_or_blank_env_means_entra_is_not_configured() {
        assert_eq!(entra_client_id_from_env(&env(&[])), None);
        assert_eq!(
            entra_client_id_from_env(&env(&[("ENTRA_CLIENT_ID", "")])),
            None
        );
        assert_eq!(
            entra_client_id_from_env(&env(&[("ENTRA_CLIENT_ID", "   ")])),
            None
        );
        assert_eq!(
            entra_client_id_from_env(&env(&[("MAIN_VITE_ENTRA_CLIENT_ID", "")])),
            None
        );
    }

    #[test]
    fn placeholder_uuid_is_treated_as_unset() {
        assert_eq!(
            entra_client_id_from_env(&env(&[(
                "ENTRA_CLIENT_ID",
                "00000000-0000-0000-0000-000000000000"
            )])),
            None
        );
    }

    #[test]
    fn a_real_public_client_id_is_configured() {
        assert_eq!(
            entra_client_id_from_env(&env(&[(
                "ENTRA_CLIENT_ID",
                "11111111-2222-3333-4444-555555555555"
            )])),
            Some("11111111-2222-3333-4444-555555555555".into())
        );
    }

    #[test]
    fn whitespace_entra_does_not_hide_main_vite() {
        assert_eq!(
            entra_client_id_from_env(&env(&[
                ("ENTRA_CLIENT_ID", "   "),
                (
                    "MAIN_VITE_ENTRA_CLIENT_ID",
                    "11111111-2222-3333-4444-555555555555"
                ),
            ])),
            Some("11111111-2222-3333-4444-555555555555".into())
        );
    }
}
