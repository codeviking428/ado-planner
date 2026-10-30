pub const PRODUCTION_ADO_BASE: &str = "https://dev.azure.com";
pub const PRODUCTION_VSSPS_BASE: &str = "https://app.vssps.visualstudio.com";
pub const PRODUCTION_VSSPS_IDENTITIES_HOST: &str = "https://vssps.dev.azure.com";

pub fn trim_base(value: &str) -> String {
    value.trim().trim_end_matches('/').to_string()
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(trim_base)
}

/// Org REST host: env/explicit override, otherwise `https://dev.azure.com`.
pub fn resolve_ado_base(explicit: Option<&str>, env_ado: Option<&str>) -> String {
    nonempty(explicit)
        .or_else(|| nonempty(env_ado))
        .unwrap_or_else(|| PRODUCTION_ADO_BASE.into())
}

/// Profile/accounts host. Production is `https://app.vssps.visualstudio.com`.
/// A custom ADO host (the mock) serves profile on the same origin.
pub fn resolve_vssps_base(
    explicit: Option<&str>,
    env_vssps: Option<&str>,
    ado_base: &str,
) -> String {
    nonempty(explicit)
        .or_else(|| nonempty(env_vssps))
        .unwrap_or_else(|| {
            if ado_base == PRODUCTION_ADO_BASE {
                PRODUCTION_VSSPS_BASE.into()
            } else {
                ado_base.to_string()
            }
        })
}

/// Identity search host. Production is org-scoped VSSPS; the mock shares `ado_base`.
pub fn identities_search_host(ado_base: &str, org: &str) -> String {
    if ado_base == PRODUCTION_ADO_BASE {
        format!("{PRODUCTION_VSSPS_IDENTITIES_HOST}/{org}")
    } else {
        ado_base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_defaults_split_org_and_vssps_hosts() {
        assert_eq!(resolve_ado_base(None, None), PRODUCTION_ADO_BASE);
        assert_eq!(
            resolve_vssps_base(None, None, PRODUCTION_ADO_BASE),
            PRODUCTION_VSSPS_BASE
        );
        assert_eq!(
            identities_search_host(PRODUCTION_ADO_BASE, "contoso"),
            "https://vssps.dev.azure.com/contoso"
        );
    }

    #[test]
    fn custom_ado_base_reuses_the_same_origin_for_vssps() {
        let mock = "http://127.0.0.1:9";
        assert_eq!(resolve_ado_base(Some(mock), None), mock);
        assert_eq!(resolve_vssps_base(None, None, mock), mock);
        assert_eq!(identities_search_host(mock, "contoso"), mock);
    }

    #[test]
    fn explicit_vssps_wins_over_the_ado_host() {
        assert_eq!(
            resolve_vssps_base(
                Some("https://vssps.example/"),
                Some("https://ignored"),
                PRODUCTION_ADO_BASE
            ),
            "https://vssps.example"
        );
    }
}
