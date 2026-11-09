use std::path::PathBuf;

use ado_core::UpdaterPrompt;

const DEFAULT_ENDPOINT: &str =
    "https://github.com/codeviking428/ado-planner/releases/latest/download/latest.json";

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
    executable_path: Option<PathBuf>,
}

/// An update whose bytes passed the minisign check against the built-in key.
pub struct VerifiedUpdate {
    update: cargo_packager_updater::Update,
    bytes: Vec<u8>,
}

impl VerifiedUpdate {
    pub fn version(&self) -> &str {
        &self.update.version
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Replaces the AppImage in place on Linux. On Windows this starts the
    /// NSIS installer with `/P /R` and exits the process.
    pub fn install(self) -> Result<(), String> {
        self.update
            .install(self.bytes)
            .map_err(|err| err.to_string())
    }
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

fn parse_version(version: &str) -> cargo_packager_updater::semver::Version {
    version
        .parse()
        .unwrap_or_else(|_| "0.1.0".parse().expect("semver"))
}

impl Updater {
    pub fn from_env() -> Self {
        Self::new(endpoint(), pubkey())
    }

    pub fn new(endpoint: impl Into<String>, pubkey: impl Into<String>) -> Self {
        Self {
            prompt: None,
            snoozed: false,
            pubkey: pubkey.into(),
            endpoint: endpoint.into(),
            executable_path: None,
        }
    }

    /// The file a Linux update replaces. Defaults to `$APPIMAGE`, then the
    /// running executable.
    pub fn with_executable_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.executable_path = Some(path.into());
        self
    }

    pub fn inject(&mut self, prompt: UpdaterPrompt) {
        if !self.snoozed {
            self.prompt = Some(prompt);
        }
    }

    fn packager(&self, current_version: &str) -> Result<cargo_packager_updater::Updater, String> {
        let endpoint = self
            .endpoint
            .parse()
            .map_err(|_| "invalid updater endpoint".to_string())?;
        let config = cargo_packager_updater::Config {
            endpoints: vec![endpoint],
            pubkey: self.pubkey.clone(),
            ..Default::default()
        };
        let mut builder =
            cargo_packager_updater::UpdaterBuilder::new(parse_version(current_version), config);
        if let Some(path) = &self.executable_path {
            builder = builder.executable_path(path);
        }
        builder.build().map_err(|err| err.to_string())
    }

    pub fn check_blocking(
        &mut self,
        current_version: &str,
    ) -> Result<Option<UpdaterPrompt>, String> {
        if self.snoozed || self.pubkey.trim().is_empty() {
            return Ok(self.prompt.clone());
        }
        let Ok(packager) = self.packager(current_version) else {
            return Ok(None);
        };
        match packager.check() {
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

    /// Fetches the feed again and downloads the announced build. Returns an
    /// error if its signature does not match the built-in key.
    pub fn download_blocking(
        &self,
        current_version: &str,
    ) -> Result<Option<VerifiedUpdate>, String> {
        let packager = self.packager(current_version)?;
        let Some(update) = packager.check().map_err(|err| err.to_string())? else {
            return Ok(None);
        };
        let bytes = update.download().map_err(|err| err.to_string())?;
        Ok(Some(VerifiedUpdate { update, bytes }))
    }

    pub fn apply(&mut self) -> Result<(), String> {
        self.apply_from(env!("CARGO_PKG_VERSION"))
    }

    pub fn apply_from(&mut self, current_version: &str) -> Result<(), String> {
        if self.pubkey.trim().is_empty() {
            self.prompt = None;
            return Ok(());
        }
        if self.prompt.is_none() {
            return Ok(());
        }
        if let Some(update) = self.download_blocking(current_version)? {
            update.install()?;
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
