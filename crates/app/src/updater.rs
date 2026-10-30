use ado_core::UpdaterPrompt;

const DEFAULT_ENDPOINT: &str =
    "https://github.com/codeviking428/ado-planner/releases/download/rust-latest/latest.json";

const COMPILED_PUBKEY: &str = match option_env!("ADO_PLANNER_UPDATER_PUBKEY") {
    Some(value) => value,
    None => "",
};

const COMPILED_ENDPOINT: &str = match option_env!("ADO_PLANNER_UPDATER_ENDPOINT") {
    Some(value) => value,
    None => DEFAULT_ENDPOINT,
};

#[derive(Debug, Clone, Default)]
pub struct Updater {
    pub prompt: Option<UpdaterPrompt>,
    snoozed: bool,
    pubkey: String,
    endpoint: String,
}

#[cfg(debug_assertions)]
fn runtime_override(var: &str) -> Option<String> {
    std::env::var(var)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn pubkey() -> String {
    #[cfg(debug_assertions)]
    if let Some(value) = runtime_override("ADO_PLANNER_UPDATER_PUBKEY") {
        return value;
    }
    COMPILED_PUBKEY.to_string()
}

fn endpoint() -> String {
    #[cfg(debug_assertions)]
    if let Some(value) = runtime_override("ADO_PLANNER_UPDATER_ENDPOINT") {
        return value;
    }
    COMPILED_ENDPOINT.to_string()
}

impl Updater {
    pub fn from_env() -> Self {
        Self {
            prompt: None,
            snoozed: false,
            pubkey: pubkey(),
            endpoint: endpoint(),
        }
    }

    pub fn inject(&mut self, prompt: UpdaterPrompt) {
        if !self.snoozed {
            self.prompt = Some(prompt);
        }
    }

    pub fn check_blocking(
        &mut self,
        current_version: &str,
    ) -> Result<Option<UpdaterPrompt>, String> {
        if self.snoozed || self.pubkey.trim().is_empty() {
            return Ok(self.prompt.clone());
        }
        let Ok(endpoint) = self.endpoint.parse() else {
            return Ok(None);
        };
        let config = cargo_packager_updater::Config {
            endpoints: vec![endpoint],
            pubkey: self.pubkey.clone(),
            ..Default::default()
        };
        let version = current_version
            .parse()
            .unwrap_or_else(|_| "0.1.0".parse().expect("semver"));
        match cargo_packager_updater::check_update(version, config) {
            Ok(Some(update)) => {
                let prompt = UpdaterPrompt {
                    version: update.version.clone(),
                    release_notes: update.body.clone(),
                };
                self.prompt = Some(prompt.clone());
                Ok(Some(prompt))
            }
            Ok(None) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }

    pub fn apply(&mut self) -> Result<(), String> {
        if self.pubkey.trim().is_empty() {
            self.prompt = None;
            return Ok(());
        }
        let Some(_prompt) = self.prompt.clone() else {
            return Ok(());
        };
        let Ok(endpoint) = self.endpoint.parse() else {
            return Err("invalid updater endpoint".into());
        };
        let config = cargo_packager_updater::Config {
            endpoints: vec![endpoint],
            pubkey: self.pubkey.clone(),
            ..Default::default()
        };
        let updater = cargo_packager_updater::UpdaterBuilder::new(
            env!("CARGO_PKG_VERSION")
                .parse()
                .unwrap_or_else(|_| "0.1.0".parse().expect("semver")),
            config,
        )
        .build()
        .map_err(|err| err.to_string())?;
        let update = updater.check().map_err(|err| err.to_string())?;
        if let Some(update) = update {
            update
                .download_and_install()
                .map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    pub fn snooze(&mut self) {
        self.snoozed = true;
        self.prompt = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pubkey_skips_network() {
        let mut updater = Updater::from_env();
        updater.pubkey.clear();
        assert!(updater.check_blocking("0.1.0").unwrap().is_none());
    }

    #[test]
    fn compiled_pubkey_comes_from_build_env() {
        assert_eq!(
            COMPILED_PUBKEY,
            option_env!("ADO_PLANNER_UPDATER_PUBKEY").unwrap_or("")
        );
        assert_eq!(COMPILED_ENDPOINT, DEFAULT_ENDPOINT);
    }

    #[test]
    fn snooze_hides_later_prompts() {
        let mut updater = Updater::default();
        updater.snooze();
        updater.inject(UpdaterPrompt {
            version: "9.0.0".into(),
            release_notes: None,
        });
        assert!(updater.prompt.is_none());
    }
}
