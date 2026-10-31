use url::Url;

fn parse_url(value: &str) -> Option<Url> {
    Url::parse(value)
        .ok()
        .or_else(|| Url::parse(&format!("https://{value}")).ok())
}

/// Keep `https://dev.azure.com/{org}` (or the visualstudio.com host) and drop path, query, and hash.
pub fn shorten_organization_url(value: &str) -> String {
    let input = value.trim();
    if input.is_empty() {
        return value.to_string();
    }
    let Some(url) = parse_url(input) else {
        return value.to_string();
    };
    let hostname = url.host_str().unwrap_or("").to_lowercase();
    if hostname == "dev.azure.com" || hostname == "www.dev.azure.com" {
        let org = url
            .path_segments()
            .into_iter()
            .flatten()
            .find(|segment| !segment.is_empty())
            .unwrap_or("");
        if org.is_empty() || !is_org_slug(org) {
            return value.to_string();
        }
        return format!("https://dev.azure.com/{org}");
    }
    if let Some(org) = legacy_visualstudio_org(&hostname) {
        return format!("https://{org}.visualstudio.com");
    }
    value.to_string()
}

fn is_org_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 50 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_alphanumeric() {
        return false;
    }
    bytes[1..]
        .iter()
        .all(|ch| ch.is_ascii_alphanumeric() || *ch == b'-')
}

fn legacy_visualstudio_org(hostname: &str) -> Option<&str> {
    let suffix = ".visualstudio.com";
    let org = hostname.strip_suffix(suffix)?;
    if is_org_slug(org) { Some(org) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_org_slug_on_a_pasted_azure_devops_url() {
        assert_eq!(
            shorten_organization_url(
                "https://dev.azure.com/contoso/Shop/_workitems/edit/123?_a=edit#discussion"
            ),
            "https://dev.azure.com/contoso"
        );
    }

    #[test]
    fn strips_a_trailing_slash_from_an_org_url() {
        assert_eq!(
            shorten_organization_url("https://dev.azure.com/contoso/"),
            "https://dev.azure.com/contoso"
        );
    }

    #[test]
    fn leaves_an_already_short_org_url_unchanged() {
        assert_eq!(
            shorten_organization_url("https://dev.azure.com/contoso"),
            "https://dev.azure.com/contoso"
        );
    }

    #[test]
    fn accepts_a_url_without_a_scheme() {
        assert_eq!(
            shorten_organization_url("dev.azure.com/contoso/Shop/_boards"),
            "https://dev.azure.com/contoso"
        );
    }

    #[test]
    fn shortens_a_legacy_visualstudio_com_url_to_the_org_host() {
        assert_eq!(
            shorten_organization_url("https://contoso.visualstudio.com/Shop/_workitems/edit/1"),
            "https://contoso.visualstudio.com"
        );
    }

    #[test]
    fn leaves_an_org_slug_alone() {
        assert_eq!(shorten_organization_url("contoso"), "contoso");
    }

    #[test]
    fn leaves_incomplete_or_unrelated_input_alone() {
        assert_eq!(
            shorten_organization_url("https://dev.azure.com/"),
            "https://dev.azure.com/"
        );
        assert_eq!(
            shorten_organization_url("https://github.com/contoso/repo"),
            "https://github.com/contoso/repo"
        );
        assert_eq!(shorten_organization_url("not a url"), "not a url");
    }

    #[test]
    fn org_slug_pattern_matches_typescript() {
        assert!(is_org_slug("contoso"));
        assert!(!is_org_slug(""));
    }
}
