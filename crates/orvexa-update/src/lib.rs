//! Signed self-update for ORVEXA Desktop.
//!
//! 1. Every few hours the app downloads the channel manifest and its
//!    signature from the site and verifies it against keys built into the app.
//! 2. A newer release (never an older one than already seen) is downloaded
//!    in the background and accepted only if its SHA-256 matches the manifest.
//! 3. The user presses "Restart and update": the verified build replaces the
//!    current one (the previous build is kept until the new one starts) and the
//!    app restarts. Nothing is installed without that press.
//!
//! The update routes need the same per-launch session token as private v2
//! wallets: only the app window can trigger an install.

pub mod install;
pub mod manifest;

use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use ed25519_dalek::VerifyingKey;
use manifest::{ALLOWED_PREFIX, Manifest, compare, platform};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;

pub const SESSION_HEADER: &str = "x-orvexa-session";
pub const DEFAULT_MANIFEST_URL: &str = "https://waxiechc.github.io/orvexa/updates/preview.json";
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Release this binary was packaged as; unset for development builds.
pub const CURRENT: Option<&str> = option_env!("ORVEXA_RELEASE");

/// Update signing keys trusted by this build (hex, one per line).
pub fn trusted_keys() -> Vec<VerifyingKey> {
    include_str!("../update-key.pub")
        .lines()
        .filter_map(|l| hex::decode(l.trim()).ok())
        .filter_map(|b| <[u8; 32]>::try_from(b).ok())
        .filter_map(|b| VerifyingKey::from_bytes(&b).ok())
        .collect()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// Automatic updates: background checks and downloads. Off means this
    /// app never contacts the update server on its own.
    automatic: bool,
    /// Highest release this installation has seen; older ones are refused.
    highest_seen: Option<String>,
}

#[derive(Default)]
struct UpdateState {
    settings: Settings,
    available: Option<Manifest>,
    staged: Option<PathBuf>,
    checking: bool,
    error: Option<String>,
    checked_at: Option<u64>,
}

pub struct Updater {
    dir: PathBuf,
    token: String,
    current: Option<String>,
    manifest_url: String,
    artifact_prefix: String,
    trusted: Vec<VerifyingKey>,
    http: reqwest::Client,
    state: Mutex<UpdateState>,
}

#[derive(Serialize)]
pub struct Available {
    pub release: String,
    pub notes_ru: String,
    pub notes_en: String,
    pub bytes: u64,
}

#[derive(Serialize)]
pub struct StateView {
    pub current: Option<String>,
    pub platform: &'static str,
    pub automatic: bool,
    pub checking: bool,
    pub checked_at: Option<u64>,
    pub error: Option<String>,
    pub available: Option<Available>,
    pub ready: bool,
    pub can_install: bool,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Updater {
    /// The production updater for this build, storing state in `<data>/update`.
    pub fn new(data: &Path, token: String) -> Arc<Self> {
        // Mirrors may serve the manifest and files elsewhere (also used by the
        // release smoke test). Safe: the signature is still checked against the
        // keys built into this binary, and every file against its signed hash.
        let manifest = std::env::var("ORVEXA_UPDATE_MANIFEST_URL")
            .unwrap_or_else(|_| DEFAULT_MANIFEST_URL.into());
        let prefix = std::env::var("ORVEXA_UPDATE_ARTIFACT_PREFIX")
            .unwrap_or_else(|_| ALLOWED_PREFIX.into());
        Self::with(
            data,
            token,
            CURRENT.map(str::to_owned),
            manifest,
            prefix,
            trusted_keys(),
        )
    }

    pub fn with(
        data: &Path,
        token: String,
        current: Option<String>,
        manifest_url: String,
        artifact_prefix: String,
        trusted: Vec<VerifyingKey>,
    ) -> Arc<Self> {
        let dir = data.join("update");
        let settings = std::fs::read(dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Settings {
                automatic: true,
                highest_seen: None,
            });
        Arc::new(Updater {
            dir,
            token,
            current,
            manifest_url,
            artifact_prefix,
            trusted,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("http client"),
            state: Mutex::new(UpdateState {
                settings,
                ..UpdateState::default()
            }),
        })
    }

    /// Background checks: shortly after launch, then every [`CHECK_EVERY`].
    pub fn start(self: &Arc<Self>) {
        install::finish();
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(20)).await;
            loop {
                let automatic = me.state.lock().await.settings.automatic;
                if automatic && me.current.is_some() && !me.trusted.is_empty() {
                    let _ = me.check().await;
                }
                tokio::time::sleep(CHECK_EVERY).await;
            }
        });
    }

    fn save(&self, settings: &Settings) {
        let _ = std::fs::create_dir_all(&self.dir);
        let temp = self.dir.join("settings.json.tmp");
        if std::fs::write(
            &temp,
            serde_json::to_vec_pretty(settings).expect("plain struct"),
        )
        .is_ok()
        {
            let _ = std::fs::rename(&temp, self.dir.join("settings.json"));
        }
    }

    async fn fetch(&self, url: &str, limit: usize) -> Result<Vec<u8>, String> {
        let mut response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| "The update server cannot be reached.".to_string())?;
        if !response.status().is_success() {
            return Err("No update information is published yet.".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "The update download was interrupted.".to_string())?
        {
            body.extend_from_slice(&chunk);
            if body.len() > limit {
                return Err("The update file is larger than announced.".into());
            }
        }
        Ok(body)
    }

    /// Checks the channel; downloads a newer build when automatic download is on.
    pub async fn check(&self) -> Result<(), String> {
        {
            let mut s = self.state.lock().await;
            if s.checking {
                return Ok(());
            }
            s.checking = true;
            s.error = None;
        }
        let result = self.check_inner().await;
        let mut s = self.state.lock().await;
        s.checking = false;
        s.checked_at = Some(now());
        if let Err(e) = &result {
            s.error = Some(e.clone());
        }
        result
    }

    async fn check_inner(&self) -> Result<(), String> {
        let bytes = self
            .fetch(&self.manifest_url, manifest::MAX_MANIFEST_BYTES)
            .await?;
        let signature = self
            .fetch(&format!("{}.sig", self.manifest_url), 1024)
            .await?;
        let signature = String::from_utf8(signature)
            .map_err(|_| "The update signature is invalid.".to_string())?;
        let m = manifest::verify_with_prefix(
            &bytes,
            &signature,
            &self.trusted,
            now(),
            &self.artifact_prefix,
        )
        .map_err(|e| match e {
            manifest::ManifestError::Expired => {
                "The published update information has expired.".to_string()
            }
            _ => "The update information is not signed by ORVEXA. It was ignored.".to_string(),
        })?;
        let mut s = self.state.lock().await;
        if let Some(highest) = &s.settings.highest_seen
            && compare(&m.release, highest) == Some(Ordering::Less)
        {
            return Err(
                "The published update is older than one already seen. It was ignored.".into(),
            );
        }
        let newer = self
            .current
            .as_deref()
            .is_some_and(|c| compare(&m.release, c) == Some(Ordering::Greater));
        let mut settings = s.settings.clone();
        settings.highest_seen = Some(m.release.clone());
        self.save(&settings);
        s.settings = settings;
        if !newer {
            s.available = None;
            return Ok(());
        }
        let auto = s.settings.automatic;
        let changed = s.available.as_ref().is_none_or(|a| a.release != m.release);
        if changed {
            s.staged = None;
        }
        s.available = Some(m);
        drop(s);
        if auto { self.download().await } else { Ok(()) }
    }

    /// Downloads and verifies the build for this platform.
    pub async fn download(&self) -> Result<(), String> {
        let (release, file) = {
            let s = self.state.lock().await;
            let m = s.available.as_ref().ok_or("No update is available.")?;
            if s.staged.is_some() {
                return Ok(());
            }
            let file = m
                .files
                .get(platform())
                .cloned()
                .ok_or("This update has no build for this computer.")?;
            (m.release.clone(), file)
        };
        let staged_dir = self.dir.join("staged");
        std::fs::create_dir_all(&staged_dir).map_err(|e| e.to_string())?;
        let name = file
            .url
            .rsplit('/')
            .next()
            .filter(|n| !n.is_empty() && !n.contains('\\'))
            .ok_or("Bad file name.")?;
        let target = staged_dir.join(format!("{release}-{name}"));
        let body = self.fetch(&file.url, file.bytes as usize).await?;
        if body.len() as u64 != file.bytes || hex::encode(Sha256::digest(&body)) != file.sha256 {
            return Err(
                "The downloaded update does not match its signed checksum. It was discarded."
                    .into(),
            );
        }
        let temp = target.with_extension("part");
        std::fs::write(&temp, &body).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, &target).map_err(|e| e.to_string())?;
        self.state.lock().await.staged = Some(target);
        Ok(())
    }

    pub async fn view(&self) -> StateView {
        let s = self.state.lock().await;
        let packaged = install::installed_path().is_ok();
        StateView {
            current: self.current.clone(),
            platform: platform(),
            automatic: s.settings.automatic,
            checking: s.checking,
            checked_at: s.checked_at,
            error: s.error.clone(),
            available: s.available.as_ref().map(|m| Available {
                release: m.release.clone(),
                notes_ru: m.notes_ru.clone(),
                notes_en: m.notes_en.clone(),
                bytes: m.files.get(platform()).map_or(0, |f| f.bytes),
            }),
            ready: s.staged.is_some(),
            can_install: s.staged.is_some() && self.current.is_some() && packaged,
        }
    }

    /// Re-checks the staged file and swaps it in. The caller exits afterwards.
    pub async fn install(&self) -> Result<PathBuf, String> {
        let s = self.state.lock().await;
        let (m, staged) = match (&s.available, &s.staged) {
            (Some(m), Some(p)) => (m.clone(), p.clone()),
            _ => return Err("Download the update first.".into()),
        };
        drop(s);
        let file = m
            .files
            .get(platform())
            .ok_or("This update has no build for this computer.")?;
        let body = std::fs::read(&staged).map_err(|e| e.to_string())?;
        if hex::encode(Sha256::digest(&body)) != file.sha256 {
            return Err(
                "The downloaded update does not match its signed checksum. It was discarded."
                    .into(),
            );
        }
        let installed = install::installed_path().map_err(|e| e.0)?;
        let done = if cfg!(target_os = "macos") {
            install::install_macos(&staged, &installed, &m.release)
        } else {
            install::install_windows(&staged, &installed)
        };
        done.map_err(|e| format!("The update could not be installed: {}", e.0))?;
        install::relaunch(&installed).map_err(|e| e.0)?;
        let _ = std::fs::remove_file(&staged);
        Ok(installed)
    }

    pub fn router(self: &Arc<Self>) -> Router {
        Router::new()
            .route("/desktop/update/state", get(state))
            .route("/desktop/update/check", post(check))
            .route("/desktop/update/settings", post(settings))
            .route("/desktop/update/install", post(install_now))
            .route_layer(middleware::from_fn_with_state(
                self.clone(),
                require_session,
            ))
            .with_state(self.clone())
    }
}

async fn require_session(State(u): State<Arc<Updater>>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(SESSION_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    if supplied.len() != u.token.len() || !bool::from(supplied.ct_eq(u.token.as_bytes())) {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Updates can only be managed from the ORVEXA app window." }))).into_response();
    }
    next.run(request).await
}

async fn state(State(u): State<Arc<Updater>>) -> Json<StateView> {
    Json(u.view().await)
}

async fn check(State(u): State<Arc<Updater>>) -> Response {
    let _ = u.check().await;
    Json(u.view().await).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsRequest {
    automatic: bool,
}

async fn settings(
    State(u): State<Arc<Updater>>,
    Json(req): Json<SettingsRequest>,
) -> Json<StateView> {
    {
        let mut s = u.state.lock().await;
        s.settings.automatic = req.automatic;
        u.save(&s.settings);
    }
    Json(u.view().await)
}

async fn install_now(State(u): State<Arc<Updater>>) -> Response {
    match u.install().await {
        Ok(_) => {
            // Let the response reach the window, then hand over to the new build.
            tokio::spawn(async {
                tokio::time::sleep(Duration::from_millis(600)).await;
                std::process::exit(0);
            });
            Json(serde_json::json!({ "restarting": true })).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}
