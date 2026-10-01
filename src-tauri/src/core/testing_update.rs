use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::LazyLock, time::Duration};
use tokio::{io::AsyncWriteExt, sync::Mutex};

const MANIFEST_URL: &str = "https://raw.githubusercontent.com/VinHanakan/Clash-Verge-Clew-Testing/main/updates/windows-x64.json";
const DOWNLOAD_PREFIX: &str = "https://github.com/VinHanakan/Clash-Verge-Clew-Testing/releases/download/";
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Default, Serialize)]
pub struct UpdateState {
    pub phase: String,
    pub version: Option<String>,
    pub received: u64,
    pub total: Option<u64>,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Deserialize)]
struct Manifest {
    schema_version: u32,
    target: String,
    available: bool,
    version: Option<String>,
    url: Option<String>,
    sha256: Option<String>,
    source_commit: Option<String>,
}

fn test_version(version: &str) -> Result<(u32, u32, u32, u32)> {
    let (base, iteration) = version.split_once("-test.").context("unsupported testing version")?;
    let numbers = base.split('.').map(str::parse::<u32>).collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(numbers.len() == 3, "invalid version");
    Ok((numbers[0], numbers[1], numbers[2], iteration.parse()?))
}

fn validate(manifest: &Manifest) -> Result<bool> {
    ensure!(manifest.schema_version == 1 && manifest.target == "windows-x64-internal-test", "unsupported update manifest");
    if !manifest.available { return Ok(false); }
    let version = manifest.version.as_deref().context("missing update version")?;
    let url = manifest.url.as_deref().context("missing installer URL")?;
    let parsed = reqwest::Url::parse(url)?;
    ensure!(parsed.as_str().starts_with(DOWNLOAD_PREFIX) && url.ends_with(".exe") && !url.contains(['?', '#']) && parsed.username().is_empty(), "installer must belong to the Testing repository");
    let hash = manifest.sha256.as_deref().context("missing SHA-256")?;
    ensure!(hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()), "invalid installer SHA-256");
    let source = manifest.source_commit.as_deref().context("missing source commit")?;
    ensure!(source.len() == 40 && source.bytes().all(|b| b.is_ascii_hexdigit()), "invalid source commit");
    Ok(test_version(version)? > test_version(env!("CARGO_PKG_VERSION"))?)
}

#[derive(Default)]
pub struct TestingUpdater {
    operation: Mutex<()>,
    manifest: Mutex<Option<Manifest>>,
    state: parking_lot::RwLock<UpdateState>,
}

pub static UPDATER: LazyLock<TestingUpdater> = LazyLock::new(TestingUpdater::default);

fn download_dir() -> Result<PathBuf> {
    Ok(crate::utils::dirs::app_home_dir()?.join("updates"))
}

impl TestingUpdater {
    pub fn state(&self) -> UpdateState { self.state.read().clone() }

    async fn client() -> Result<reqwest::Client> {
        Ok(reqwest::Client::builder().user_agent("Clash-Verge-Clew-Testing")
            .connect_timeout(Duration::from_secs(15)).timeout(Duration::from_secs(600)).build()?)
    }

    pub async fn check(&self) -> Result<UpdateState> {
        let _operation = self.operation.try_lock().context("update operation already running")?;
        let result = async {
            let response = Self::client().await?.get(MANIFEST_URL).timeout(Duration::from_secs(30)).send().await?.error_for_status()?;
            ensure!(response.content_length().unwrap_or(0) <= 16 * 1024, "update manifest too large");
            let bytes = response.bytes().await?;
            ensure!(bytes.len() <= 16 * 1024, "update manifest too large");
            let manifest: Manifest = serde_json::from_slice(&bytes)?;
            let newer = validate(&manifest)?;
            let state = UpdateState {
                phase: if newer { "available" } else if manifest.available { "current" } else { "unpublished" }.into(),
                version: manifest.version.clone(), ..Default::default()
            };
            *self.manifest.lock().await = newer.then_some(manifest);
            Ok::<_, anyhow::Error>(state)
        }.await;
        match result {
            Ok(state) => { *self.state.write() = state.clone(); Ok(state) }
            Err(error) => { self.fail(&error); Err(error) }
        }
    }

    fn fail(&self, error: &anyhow::Error) {
        let mut state = self.state.write();
        state.phase = "error".into();
        state.error = Some(format!("{error:#}"));
        clash_verge_logging::logging!(error, clash_verge_logging::Type::Core, "Testing update failed: {error:#}");
    }

    pub async fn download(&self) -> Result<UpdateState> {
        let _operation = self.operation.try_lock().context("update operation already running")?;
        let manifest = self.manifest.lock().await.clone().context("check for an available update first")?;
        ensure!(validate(&manifest)?, "no newer testing version");
        let dir = download_dir()?;
        tokio::fs::create_dir_all(&dir).await?;
        let version = manifest.version.as_deref().context("missing version")?;
        let destination = dir.join(format!("Clash-Verge-Clew-Internal_{version}_x64-setup.exe"));
        let partial = destination.with_extension("exe.part");
        let result = async {
            let mut response = Self::client().await?.get(manifest.url.as_deref().context("missing URL")?).send().await?.error_for_status()?;
            let total = response.content_length();
            ensure!(total.unwrap_or(0) <= MAX_INSTALLER_BYTES, "installer too large");
            *self.state.write() = UpdateState { phase: "downloading".into(), version: Some(version.into()), total, ..Default::default() };
            let mut file = tokio::fs::File::create(&partial).await?;
            let mut digest = Sha256::new();
            let mut received = 0u64;
            while let Some(chunk) = response.chunk().await? {
                received += chunk.len() as u64;
                ensure!(received <= MAX_INSTALLER_BYTES, "installer too large");
                digest.update(&chunk);
                file.write_all(&chunk).await?;
                self.state.write().received = received;
            }
            file.sync_all().await?;
            drop(file);
            ensure!(received > 0 && total.is_none_or(|size| size == received), "incomplete installer download");
            let actual = digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            ensure!(Some(actual.as_str()) == manifest.sha256.as_deref().map(str::to_ascii_lowercase).as_deref(), "installer SHA-256 mismatch");
            tokio::fs::rename(&partial, &destination).await?;
            let mut state = self.state.write();
            state.phase = "downloaded".into();
            state.path = Some(destination.display().to_string());
            Ok::<_, anyhow::Error>(state.clone())
        }.await;
        match result {
            Ok(state) => Ok(state),
            Err(error) => { let _ = tokio::fs::remove_file(partial).await; self.fail(&error); Err(error) }
        }
    }
}

#[tauri::command]
pub async fn check_testing_update() -> crate::cmd::CmdResult<UpdateState> {
    use crate::cmd::StringifyErr;
    UPDATER.check().await.stringify_err()
}

#[tauri::command]
pub async fn download_testing_update() -> crate::cmd::CmdResult<UpdateState> {
    use crate::cmd::StringifyErr;
    UPDATER.download().await.stringify_err()
}

#[tauri::command]
pub fn get_testing_update_state() -> UpdateState { UPDATER.state() }

#[tauri::command]
pub fn open_testing_update_folder() -> crate::cmd::CmdResult<()> {
    use crate::cmd::StringifyErr;
    download_dir().and_then(|path| { std::fs::create_dir_all(&path)?; open::that(path).map_err(Into::into) }).stringify_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_upstream_installers_and_requires_integrity_metadata() {
        let mut manifest = Manifest { schema_version: 1, target: "windows-x64-internal-test".into(), available: true,
            version: Some("0.1.0-test.999".into()), url: Some(format!("{DOWNLOAD_PREFIX}test/installer.exe")),
            sha256: Some("a".repeat(64)), source_commit: Some("b".repeat(40)) };
        assert!(validate(&manifest).unwrap());
        manifest.url = Some("https://github.com/clash-verge-rev/clash-verge-rev/releases/download/test/installer.exe".into());
        assert!(validate(&manifest).is_err());
        manifest.url = Some(format!("{DOWNLOAD_PREFIX}test/installer.exe"));
        manifest.sha256 = None;
        assert!(validate(&manifest).is_err());
    }
}
