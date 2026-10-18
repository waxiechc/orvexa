//! Local-only desktop control plane. No wallet/password/seed or signing API.
use crate::{Node, SharedNode, locked, p2p};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use futures_util::FutureExt;
use orvexa_core::Genesis;
use orvexa_crypto::{Hash32, random_bytes};
use orvexa_rpc::{PeerStatus, RuntimeStatus, Status};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::{sync::Mutex, task::JoinHandle};
use tower::ServiceExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub profile: String,
    pub chain_id: Hash32,
    pub seeds: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub genesis: Genesis,
    pub network: Manifest,
    #[serde(default)]
    pub allow_private_peers: bool,
    #[serde(default = "default_peer_port")]
    pub peer_port: u16,
    #[serde(default)]
    pub resume_on_launch: bool,
}
fn default_peer_port() -> u16 {
    4243
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        self.genesis.validate()?;
        ensure!(
            self.network.profile == "orvexa-p2p-v1",
            "use a native ORVEXA P2P manifest"
        );
        ensure!(
            self.genesis.chain_id() == self.network.chain_id,
            "genesis does not match the supplied chain ID"
        );
        ensure!(
            !self.network.seeds.is_empty() && self.network.seeds.len() <= 32,
            "one to 32 real seed addresses required"
        );
        ensure!(self.peer_port >= 1024, "peer port must be 1024..65535");
        for (index, address) in self.network.seeds.iter().enumerate() {
            p2p::peer_address(address, self.allow_private_peers)?;
            ensure!(
                !self.network.seeds[..index].contains(address),
                "duplicate seed"
            );
        }
        ensure!(
            serde_json::to_vec(self)?.len() <= 32768,
            "network configuration too large"
        );
        Ok(())
    }
}
struct Managed {
    profile: Option<Profile>,
    node: Option<SharedNode>,
    task: Option<JoinHandle<()>>,
    phase: String,
    problem: Option<String>,
    restarts: u32,
}
pub struct Controller {
    root: PathBuf,
    managed: Mutex<Managed>,
    api: RwLock<Option<Router>>,
    _lock: File,
}
#[derive(Serialize)]
pub struct DesktopStatus {
    pub application: &'static str,
    pub phase: String,
    pub configured: bool,
    pub chain_id: Option<Hash32>,
    pub seeds: Vec<String>,
    pub peer_port: u16,
    pub allow_private_peers: bool,
    pub resume_on_launch: bool,
    pub restarts: u32,
    pub problem: Option<String>,
    pub runtime: Option<RuntimeStatus>,
    pub ledger: Option<Status>,
    pub validator_active: bool,
    pub rewards_supported: bool,
}
impl Controller {
    pub fn open(root: &Path) -> Result<Arc<Self>> {
        std::fs::create_dir_all(root)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("application.lock"))?;
        lock.try_lock()
            .context("another ORVEXA app is using this data directory")?;
        let path = root.join("profile.json");
        let (profile, problem) = if path.try_exists()? {
            let read = || -> Result<Profile> {
                ensure!(
                    std::fs::metadata(&path)?.len() <= 32768,
                    "configuration file too large"
                );
                let profile: Profile = serde_json::from_slice(&std::fs::read(&path)?)?;
                profile.validate()?;
                Ok(profile)
            };
            match read() {
                Ok(p) => (Some(p), None),
                Err(e) => (
                    None,
                    Some(format!(
                        "Cannot load network settings: {e}. Import the verified public files again."
                    )),
                ),
            }
        } else {
            (None, None)
        };
        Ok(Arc::new(Self {
            root: root.into(),
            managed: Mutex::new(Managed {
                profile,
                node: None,
                task: None,
                phase: "stopped".into(),
                problem,
                restarts: 0,
            }),
            api: RwLock::new(None),
            _lock: lock,
        }))
    }
    pub async fn status(&self) -> Result<DesktopStatus> {
        let state = self.managed.lock().await;
        let (runtime, ledger) = if let Some(node) = &state.node {
            let n = locked(node)?;
            (Some(n.runtime.clone()), Some(n.status()))
        } else {
            (None, None)
        };
        let phase = if runtime.as_ref().is_some_and(|r| r.halted) {
            "quarantined".into()
        } else {
            state.phase.clone()
        };
        Ok(DesktopStatus {
            application: "ORVEXA Desktop",
            phase,
            configured: state.profile.is_some(),
            chain_id: state.profile.as_ref().map(|p| p.network.chain_id),
            seeds: state
                .profile
                .as_ref()
                .map(|p| p.network.seeds.clone())
                .unwrap_or_default(),
            peer_port: state.profile.as_ref().map_or(4243, |p| p.peer_port),
            allow_private_peers: state
                .profile
                .as_ref()
                .is_some_and(|p| p.allow_private_peers),
            resume_on_launch: state.profile.as_ref().is_some_and(|p| p.resume_on_launch),
            restarts: state.restarts,
            problem: state.problem.clone(),
            runtime,
            ledger,
            validator_active: false,
            rewards_supported: false,
        })
    }
    pub async fn configure(&self, profile: Profile) -> Result<()> {
        profile.validate()?;
        let mut state = self.managed.lock().await;
        ensure!(
            state.node.is_none(),
            "stop the node before changing its network"
        );
        let name = format!("profile-{}.tmp", Hash32(random_bytes()?));
        let temp = self.root.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&serde_json::to_vec_pretty(&profile)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, self.root.join("profile.json"))?;
        #[cfg(unix)]
        File::open(&self.root)?.sync_all()?;
        state.profile = Some(profile);
        state.problem = None;
        state.phase = "stopped".into();
        Ok(())
    }
    fn config(&self, profile: &Profile) -> Result<p2p::Config> {
        Ok(p2p::Config {
            identity: p2p::load_identity(&self.root.join("identity.peerkey"))?,
            listen: format!("/ip4/0.0.0.0/tcp/{}", profile.peer_port).parse()?,
            seeds: profile.network.seeds.clone(),
            announce: vec![],
            allow_private: profile.allow_private_peers,
            expected_chain: profile.network.chain_id,
            evidence_dir: self
                .root
                .join(format!("evidence-{}", profile.network.chain_id)),
            authority: None,
            block_every: Duration::from_secs(5),
            sync_every: Duration::from_secs(2),
        })
    }
    pub async fn start(self: &Arc<Self>) -> Result<()> {
        let mut state = self.managed.lock().await;
        ensure!(
            state.node.is_none(),
            "node is already started; stop it before restarting"
        );
        let profile = state
            .profile
            .clone()
            .context("import the common public network files first")?;
        let db = self
            .root
            .join(format!("chain-{}.sqlite", profile.network.chain_id));
        let genesis = profile.genesis.clone();
        let opened = tokio::task::spawn_blocking(move || {
            if db.try_exists()? {
                Node::open(&db)
            } else {
                Node::create(&db, genesis)
            }
        })
        .await?;
        let node = match opened {
            Ok(n) => n.shared(),
            Err(e) => {
                state.problem = Some(format!("Cannot open the verified chain: {e}"));
                state.phase = "failed".into();
                return Err(e);
            }
        };
        let worker = match p2p::Network::new(node.clone(), self.config(&profile)?) {
            Ok(w) => w,
            Err(e) => {
                state.problem = Some(e.to_string());
                state.phase = "failed".into();
                return Err(e);
            }
        };
        *self
            .api
            .write()
            .map_err(|_| anyhow::anyhow!("desktop routing lock poisoned"))? =
            Some(crate::api_router(node.clone()));
        state.node = Some(node.clone());
        state.phase = "running".into();
        state.problem = None;
        state.restarts = 0;
        let control = self.clone();
        state.task = Some(tokio::spawn(async move {
            let mut next = Some(worker);
            for attempt in 0..=5 {
                let worker =
                    match next.take().map(Ok).unwrap_or_else(|| {
                        p2p::Network::new(node.clone(), control.config(&profile)?)
                    }) {
                        Ok(w) => w,
                        Err(e) => {
                            let mut s = control.managed.lock().await;
                            s.phase = "failed".into();
                            s.problem = Some(e.to_string());
                            break;
                        }
                    };
                {
                    let mut s = control.managed.lock().await;
                    s.phase = "running".into();
                }
                let result = AssertUnwindSafe(worker.run()).catch_unwind().await;
                let problem = match result {
                    Ok(Err(e)) => e.to_string(),
                    Ok(Ok(())) => "peer worker stopped".into(),
                    Err(_) => "peer worker failed".into(),
                };
                let mut s = control.managed.lock().await;
                s.problem = Some(problem);
                let halted = locked(&node).map_or(true, |n| n.runtime.halted);
                if halted || attempt == 5 {
                    s.phase = if halted { "quarantined" } else { "failed" }.into();
                    break;
                }
                s.phase = "restarting".into();
                s.restarts += 1;
                drop(s);
                tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
            }
        }));
        Ok(())
    }
    pub async fn stop(&self) -> Result<()> {
        let mut state = self.managed.lock().await;
        if let Some(task) = state.task.take() {
            task.abort();
            let _ = task.await;
        }
        *self
            .api
            .write()
            .map_err(|_| anyhow::anyhow!("desktop routing lock poisoned"))? = None;
        state.node = None;
        state.phase = "stopped".into();
        state.problem = None;
        Ok(())
    }
    pub async fn resume(self: &Arc<Self>) {
        let enabled = self
            .managed
            .lock()
            .await
            .profile
            .as_ref()
            .is_some_and(|p| p.resume_on_launch);
        if enabled && let Err(e) = self.start().await {
            let mut s = self.managed.lock().await;
            s.phase = "failed".into();
            s.problem = Some(format!("Resume failed: {e}"));
        }
    }
    pub fn router(self: &Arc<Self>, directory: &Path, origin: &str) -> Result<Router> {
        let api = Router::new()
            .route("/v1/desktop/status", get(status))
            .route(
                "/v1/desktop/configure",
                post(configure).layer(DefaultBodyLimit::max(32768)),
            )
            .route("/v1/desktop/start", post(start))
            .route("/v1/desktop/stop", post(stop))
            .route("/v1/{*path}", any(proxy))
            .with_state(self.clone());
        crate::web::host(api, directory, origin)
    }
}
fn failure(error: anyhow::Error) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(orvexa_rpc::ApiError {
            error: error.to_string(),
        }),
    )
        .into_response()
}
async fn status(State(c): State<Arc<Controller>>) -> Response {
    match c.status().await {
        Ok(s) => Json(s).into_response(),
        Err(e) => failure(e),
    }
}
async fn configure(State(c): State<Arc<Controller>>, Json(profile): Json<Profile>) -> Response {
    match c.configure(profile).await {
        Ok(()) => status(State(c)).await,
        Err(e) => failure(e),
    }
}
async fn start(State(c): State<Arc<Controller>>) -> Response {
    match c.start().await {
        Ok(()) => status(State(c)).await,
        Err(e) => failure(e),
    }
}
async fn stop(State(c): State<Arc<Controller>>) -> Response {
    match c.stop().await {
        Ok(()) => status(State(c)).await,
        Err(e) => failure(e),
    }
}
async fn proxy(State(c): State<Arc<Controller>>, request: Request) -> Response {
    if c.managed.lock().await.phase != "running" {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(orvexa_rpc::ApiError {
                error: "Node is not running. Check Node controls before signing.".into(),
            }),
        )
            .into_response();
    }
    let api = match c.api.read() {
        Ok(api) => api.clone(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    match api {
        Some(api) => {
            // Do not leak the outer wildcard capture into typed inner Path
            // extractors. Re-match headers/URI/body with clean routing metadata.
            let (mut parts,body)=request.into_parts();
            parts.extensions.clear();
            match api.oneshot(Request::from_parts(parts,body)).await {
                Ok(response)=>response,
                Err(error)=>match error {},
            }
        },
        None=>(StatusCode::SERVICE_UNAVAILABLE,Json(orvexa_rpc::ApiError {
            error:"Node is stopped. Open Node and start it after importing the common network files.".into()
        })).into_response(),
    }
}

/// Public status helper for a future native tray; never contains signing keys.
pub fn connected_peers(status: &DesktopStatus) -> &[PeerStatus] {
    status.runtime.as_ref().map_or(&[], |r| r.peers.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::identity;
    use orvexa_core::{Transaction, VERSION};
    use orvexa_crypto::SecretKey;
    fn key(n: u8) -> SecretKey {
        SecretKey::from_bytes(&[n; 32])
    }
    fn genesis() -> Genesis {
        Genesis::new(key(1).address(), key(2).address(), Hash32([67; 32]))
    }
    fn port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
    fn profile() -> Profile {
        let peer = identity::Keypair::generate_ed25519().public().to_peer_id();
        Profile {
            genesis: genesis(),
            network: Manifest {
                profile: "orvexa-p2p-v1".into(),
                chain_id: genesis().chain_id(),
                seeds: vec![format!("/ip4/127.0.0.1/tcp/9/p2p/{peer}")],
            },
            allow_private_peers: true,
            peer_port: port(),
            resume_on_launch: true,
        }
    }
    #[tokio::test]
    async fn explicit_profile_pin_lock_and_persistent_resume() {
        let dir = tempfile::tempdir().unwrap();
        let c = Controller::open(dir.path()).unwrap();
        assert!(!c.status().await.unwrap().configured);
        assert!(c.start().await.is_err());
        assert!(Controller::open(dir.path()).is_err());
        let mut bad = profile();
        bad.network.chain_id = Hash32::ZERO;
        assert!(c.configure(bad).await.is_err());
        let mut bad = profile();
        bad.allow_private_peers = false;
        assert!(c.configure(bad).await.is_err());
        let p = profile();
        c.configure(p.clone()).await.unwrap();
        c.start().await.unwrap();
        assert!(c.configure(p).await.is_err());
        assert!(c.start().await.is_err());
        let s = c.status().await.unwrap();
        assert_eq!(s.phase, "running");
        assert!(!s.validator_active);
        assert!(!s.rewards_supported);
        c.stop().await.unwrap();
        drop(c);
        let c = Controller::open(dir.path()).unwrap();
        c.resume().await;
        let s = c.status().await.unwrap();
        assert_eq!(s.phase, "running");
        assert_eq!(s.chain_id, Some(genesis().chain_id()));
        c.stop().await.unwrap();
    }
    #[tokio::test]
    async fn control_surface_is_same_origin_and_rejects_secret_commands() {
        let dir = tempfile::tempdir().unwrap();
        let c = Controller::open(dir.path()).unwrap();
        let web = dir.path().join("web");
        std::fs::create_dir(&web).unwrap();
        std::fs::write(web.join("index.html"), "ORVEXA").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let app = c.router(&web, &origin).unwrap();
        let server = tokio::spawn(async {
            axum::serve(listener, app).await.unwrap();
        });
        let http = reqwest::Client::new();
        assert_eq!(
            http.get(format!("{origin}/v1/desktop/status"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        for route in ["start", "stop", "configure"] {
            let r = http
                .post(format!("{origin}/v1/desktop/{route}"))
                .header("origin", "https://evil.example")
                .json(&profile())
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
        }
        assert_eq!(
            http.get(format!("{origin}/v1/desktop/status"))
                .header("host", "evil.example")
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let mut p = serde_json::to_value(profile()).unwrap();
        p["seed_phrase"] = serde_json::json!("not accepted");
        assert_eq!(
            http.post(format!("{origin}/v1/desktop/configure"))
                .json(&p)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            http.post(format!("{origin}/v1/desktop/configure"))
                .header("content-type", "application/json")
                .body(" ".repeat(32769))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        assert_eq!(
            http.get(format!("{origin}/v1/status"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert!(!c.status().await.unwrap().configured);
        let configured = http
            .post(format!("{origin}/v1/desktop/configure"))
            .json(&profile())
            .send()
            .await
            .unwrap();
        assert_eq!(configured.status(), StatusCode::OK);
        assert_eq!(
            http.post(format!("{origin}/v1/desktop/start"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        let balance = http
            .get(format!("{origin}/v1/balance/{}", key(2).address()))
            .send()
            .await
            .unwrap();
        assert_eq!(balance.status(), StatusCode::OK);
        assert_eq!(
            balance.json::<serde_json::Value>().await.unwrap()["confirmed"]["balance"],
            1_000_000
        );
        let hash = Hash32::ZERO;
        assert_eq!(
            http.get(format!("{origin}/v1/transactions/{hash}"))
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        c.stop().await.unwrap();
        server.abort();
        let _ = server.await;
    }
    #[tokio::test]
    async fn full_node_sync_survives_stop_and_app_restart() {
        let dir = tempfile::tempdir().unwrap();
        let authority = Node::create(&dir.path().join("authority.sqlite"), genesis())
            .unwrap()
            .shared();
        let config = p2p::Config {
            identity: identity::Keypair::generate_ed25519(),
            listen: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            seeds: vec![],
            announce: vec![],
            allow_private: true,
            expected_chain: genesis().chain_id(),
            evidence_dir: dir.path().join("authority-evidence"),
            authority: Some(key(1)),
            block_every: Duration::from_secs(1),
            sync_every: Duration::from_secs(1),
        };
        let network = p2p::Network::new(authority.clone(), config).unwrap();
        let worker = tokio::spawn(network.run());
        let address = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(a) = locked(&authority)
                    .unwrap()
                    .runtime
                    .listen_addresses
                    .first()
                    .cloned()
                {
                    break a;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let root = dir.path().join("app");
        let c = Controller::open(&root).unwrap();
        let mut p = profile();
        p.network.seeds = vec![address];
        c.configure(p).await.unwrap();
        c.start().await.unwrap();
        let node = c.managed.lock().await.node.clone().unwrap();
        let tx = Transaction {
            version: VERSION,
            chain_id: genesis().chain_id(),
            sender: key(2).address(),
            recipient: key(3).address(),
            amount: 25,
            fee: 1,
            nonce: 0,
        }
        .sign(&key(2))
        .unwrap();
        locked(&node).unwrap().submit(tx).unwrap();
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if locked(&node).unwrap().ledger.height() == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap();
        let tip = locked(&node).unwrap().ledger.tip();
        drop(node);
        c.stop().await.unwrap();
        drop(c);
        let c = Controller::open(&root).unwrap();
        c.resume().await;
        let node = c.managed.lock().await.node.clone().unwrap();
        assert_eq!(locked(&node).unwrap().ledger.tip(), tip);
        assert_eq!(
            locked(&node)
                .unwrap()
                .ledger
                .account(key(3).address())
                .balance,
            25
        );
        drop(node);
        c.stop().await.unwrap();
        worker.abort();
        let _ = worker.await;
    }
    #[tokio::test]
    async fn quarantine_survives_stop_and_resume_and_corrupt_config_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let c = Controller::open(dir.path()).unwrap();
        c.configure(profile()).await.unwrap();
        let evidence = dir
            .path()
            .join(format!("evidence-{}", genesis().chain_id()));
        std::fs::create_dir(&evidence).unwrap();
        std::fs::write(
            evidence.join("authority-conflict.json"),
            "preserved test marker",
        )
        .unwrap();
        c.start().await.unwrap();
        assert_eq!(c.status().await.unwrap().phase, "quarantined");
        c.stop().await.unwrap();
        drop(c);
        let c = Controller::open(dir.path()).unwrap();
        c.resume().await;
        assert_eq!(c.status().await.unwrap().phase, "quarantined");
        c.stop().await.unwrap();
        drop(c);
        std::fs::write(dir.path().join("profile.json"), "corrupt").unwrap();
        let c = Controller::open(dir.path()).unwrap();
        assert!(!c.status().await.unwrap().configured);
        assert!(c.start().await.is_err());
        assert!(c.status().await.unwrap().problem.is_some());
    }
}
