use std::time::Duration;

use serde::Deserialize;

use ado_client::{
    AuthScheme, authorization_header, connection_data_urls, profile_lookup_urls, resolve_ado_base,
    resolve_vssps_base,
};
use ado_core::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatIdentity {
    pub display_name: String,
    pub username: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDataPayload {
    pub authenticated_user: Option<AuthenticatedUser>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedUser {
    pub unique_name: Option<String>,
    pub provider_display_name: Option<String>,
    pub custom_display_name: Option<String>,
    pub properties: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProfilePayload {
    pub display_name: Option<String>,
    pub email_address: Option<String>,
}

fn property_value(
    properties: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> String {
    let Some(raw) = properties.and_then(|map| map.get(key)) else {
        return String::new();
    };
    match raw {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Object(obj) => obj
            .get("$value")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

pub fn identity_from_connection_data(payload: &ConnectionDataPayload) -> Option<PatIdentity> {
    let user = payload.authenticated_user.as_ref()?;
    let username = user
        .unique_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            let mail = property_value(user.properties.as_ref(), "Mail");
            if mail.is_empty() { None } else { Some(mail) }
        })
        .or_else(|| {
            let account = property_value(user.properties.as_ref(), "Account");
            if account.is_empty() {
                None
            } else {
                Some(account)
            }
        })
        .unwrap_or_default();
    let display_name = user
        .custom_display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            user.provider_display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| username.clone());
    if username.is_empty() {
        if display_name.is_empty() {
            None
        } else {
            Some(PatIdentity {
                display_name: display_name.clone(),
                username: display_name,
            })
        }
    } else {
        Some(PatIdentity {
            display_name: if display_name.is_empty() {
                username.clone()
            } else {
                display_name
            },
            username,
        })
    }
}

pub fn identity_from_profile(payload: &ProfilePayload) -> Option<PatIdentity> {
    let username = payload
        .email_address
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)?;
    let display_name = payload
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| username.clone());
    Some(PatIdentity {
        display_name,
        username,
    })
}

pub async fn fetch_pat_identity(
    org: &str,
    pat: &str,
    ado_base: Option<&str>,
    vssps_base: Option<&str>,
) -> Result<Option<PatIdentity>, Error> {
    fetch_session_identity(pat, AuthScheme::Pat, Some(org), ado_base, vssps_base).await
}

pub async fn fetch_session_identity(
    token: &str,
    scheme: AuthScheme,
    org: Option<&str>,
    ado_base: Option<&str>,
    vssps_base: Option<&str>,
) -> Result<Option<PatIdentity>, Error> {
    let env_ado = std::env::var("ADO_PLANNER_ADO_BASE_URL").ok();
    let env_vssps = std::env::var("ADO_PLANNER_VSSPS_URL").ok();
    let ado_base = resolve_ado_base(ado_base, env_ado.as_deref());
    let vssps = resolve_vssps_base(vssps_base, env_vssps.as_deref(), &ado_base);
    let header = authorization_header(token, scheme);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(8))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let connection_urls = org
        .filter(|org| !org.is_empty())
        .map(|org| connection_data_urls(&ado_base, org))
        .unwrap_or_default();
    let profile_urls = profile_lookup_urls(&ado_base, &vssps, org);

    let connection_fut = async {
        let mut last_err = None;
        for url in &connection_urls {
            match get_json::<ConnectionDataPayload>(&client, &header, url).await {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) => {}
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    };
    let profile_fut = async {
        let mut last_err = None;
        for url in &profile_urls {
            match get_json::<ProfilePayload>(&client, &header, url).await {
                Ok(Some(value)) => return Ok(Some(value)),
                Ok(None) => {}
                Err(err) => last_err = Some(err),
            }
        }
        if let Some(err) = last_err {
            Err(err)
        } else {
            Ok(None)
        }
    };

    let (connection, profile) = tokio::join!(connection_fut, profile_fut);
    if let Err(err) = &connection {
        tracing::warn!("connectionData identity lookup failed: {err}");
    }
    if let Err(err) = &profile {
        tracing::warn!("VSSPS profile identity lookup failed: {err}");
    }
    Ok(profile
        .ok()
        .flatten()
        .as_ref()
        .and_then(identity_from_profile)
        .or_else(|| {
            connection
                .ok()
                .flatten()
                .as_ref()
                .and_then(identity_from_connection_data)
        }))
}

async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    header: &str,
    url: &str,
) -> Result<Option<T>, Error> {
    let response = client
        .get(url)
        .header("Authorization", header)
        .send()
        .await
        .map_err(|err| Error::message(err.to_string()))?;
    if !response.status().is_success() {
        let status = response.status();
        if status.is_client_error() {
            tracing::debug!("identity {url} returned {status}");
            return Ok(None);
        }
        return Err(Error::message(format!("identity {url} returned {status}")));
    }
    let value = response
        .json()
        .await
        .map_err(|err| Error::message(err.to_string()))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_unique_name_and_display_name_from_connection_data() {
        let payload = ConnectionDataPayload {
            authenticated_user: Some(AuthenticatedUser {
                unique_name: Some("ada@contoso.com".into()),
                provider_display_name: Some("Ada Lovelace".into()),
                custom_display_name: Some("Ada".into()),
                properties: None,
            }),
        };
        assert_eq!(
            identity_from_connection_data(&payload),
            Some(PatIdentity {
                display_name: "Ada".into(),
                username: "ada@contoso.com".into(),
            })
        );
    }

    #[test]
    fn falls_back_to_mail_then_account_properties() {
        let mail = ConnectionDataPayload {
            authenticated_user: Some(AuthenticatedUser {
                unique_name: None,
                provider_display_name: Some("Ada Lovelace".into()),
                custom_display_name: None,
                properties: Some(
                    serde_json::from_value(
                        serde_json::json!({ "Mail": { "$value": "ada@contoso.com" } }),
                    )
                    .unwrap(),
                ),
            }),
        };
        assert_eq!(
            identity_from_connection_data(&mail),
            Some(PatIdentity {
                display_name: "Ada Lovelace".into(),
                username: "ada@contoso.com".into(),
            })
        );
        let account = ConnectionDataPayload {
            authenticated_user: Some(AuthenticatedUser {
                unique_name: None,
                provider_display_name: Some("Ada Lovelace".into()),
                custom_display_name: None,
                properties: Some(
                    serde_json::from_value(
                        serde_json::json!({ "Account": { "$value": "CONTOSO\\ada" } }),
                    )
                    .unwrap(),
                ),
            }),
        };
        assert_eq!(
            identity_from_connection_data(&account),
            Some(PatIdentity {
                display_name: "Ada Lovelace".into(),
                username: "CONTOSO\\ada".into(),
            })
        );
    }

    #[test]
    fn profile_uses_email_as_unique_name() {
        assert_eq!(
            identity_from_profile(&ProfilePayload {
                display_name: Some("Ada Lovelace".into()),
                email_address: Some("ada@contoso.com".into()),
            }),
            Some(PatIdentity {
                display_name: "Ada Lovelace".into(),
                username: "ada@contoso.com".into(),
            })
        );
    }

    #[test]
    fn empty_payloads_are_ignored() {
        assert_eq!(
            identity_from_connection_data(&ConnectionDataPayload::default()),
            None
        );
        assert_eq!(
            identity_from_profile(&ProfilePayload {
                display_name: Some("Ada Lovelace".into()),
                email_address: None,
            }),
            None
        );
    }
}
