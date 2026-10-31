use ado_core::{Error, shorten_organization_url};
use url::Url;

pub fn normalize_organization(value: &str) -> Result<String, Error> {
    let input = shorten_organization_url(value).trim().to_string();
    if is_org_slug(&input) {
        return Ok(input);
    }
    let url = Url::parse(&input).map_err(|_| Error::InvalidOrganizationUrl)?;
    if url.scheme() != "https" || url.query().is_some() || url.fragment().is_some() {
        return Err(Error::InvalidOrganizationUrl);
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let host = url.host_str().unwrap_or("").to_lowercase();
    if host == "dev.azure.com" && segments.len() == 1 {
        return normalize_organization(segments[0]);
    }
    if let Some(org) = host.strip_suffix(".visualstudio.com")
        && segments.is_empty()
        && is_org_slug(org)
    {
        return Ok(org.to_string());
    }
    Err(Error::InvalidOrganizationUrl)
}

fn is_org_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 50 {
        return false;
    }
    bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|ch| ch.is_ascii_alphanumeric() || *ch == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_path_shortens_to_org_slug() {
        assert_eq!(
            normalize_organization("https://dev.azure.com/contoso/Shop/_workitems/edit/123")
                .unwrap(),
            "contoso"
        );
    }

    #[test]
    fn legacy_visualstudio_host() {
        assert_eq!(
            normalize_organization("https://contoso.visualstudio.com/").unwrap(),
            "contoso"
        );
    }
}
