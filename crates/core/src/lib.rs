//! Core logic, kept free of any UI code so it can be tested anywhere.
//!
//! Milestone 1 is strictly read-only: list the sites Local knows about and
//! report whether everything needed to run them is present on disk.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One site as shown in the app.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Site {
    pub id: String,
    pub name: String,
    pub domain: String,
    /// Absolute path of the site folder (the `~` in Local's registry is expanded).
    pub path: String,
    pub php: Option<String>,
    pub mysql: Option<String>,
    pub nginx: Option<String>,
    /// Port of the site's own nginx (Local's router forwards to it).
    pub http_port: Option<u16>,
    /// True when something answers on `http_port` right now.
    pub running: bool,
    /// Things the app would need but cannot find. Empty means adoptable.
    pub missing: Vec<String>,
}

/// Everything `list_sites` can say about the machine.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Overview {
    pub local_dir: String,
    pub local_found: bool,
    pub sites: Vec<Site>,
    /// Human-readable problem, if the registry could not be read.
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawSite {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    services: HashMap<String, RawService>,
}

#[derive(Debug, Deserialize)]
struct RawService {
    #[serde(default)]
    version: String,
    #[serde(default)]
    ports: HashMap<String, Vec<u16>>,
}

/// Where Local keeps its data. `WPLOCAL_LOCAL_DIR` overrides it (used by tests).
pub fn local_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("WPLOCAL_LOCAL_DIR") {
        return PathBuf::from(custom);
    }
    home_dir().join("Library/Application Support/Local")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn expand_tilde(path: &str, home: &Path) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home.to_path_buf(),
        None => PathBuf::from(path),
    }
}

/// Finds `lightning-services/<name>-<version>+<build>` for a service.
fn find_service_dir(local: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let prefix = format!("{name}-{version}+");
    let entries = fs::read_dir(local.join("lightning-services")).ok()?;
    entries
        .flatten()
        .map(|e| e.path())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix)))
}

fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(150)).is_ok()
}

fn first_port(service: &RawService, key: &str) -> Option<u16> {
    service.ports.get(key).and_then(|p| p.first().copied())
}

fn build_site(raw: RawSite, local: &Path, home: &Path, check_ports: bool) -> Site {
    let version_of = |name: &str| raw.services.get(name).map(|s| s.version.clone()).filter(|v| !v.is_empty());
    let http_port = raw.services.get("nginx").and_then(|s| first_port(s, "HTTP"));

    let mut missing = Vec::new();
    for name in ["nginx", "php", "mysql"] {
        match version_of(name) {
            None => missing.push(format!("{name}: not listed in Local's registry")),
            Some(v) => {
                if find_service_dir(local, name, &v).is_none() {
                    missing.push(format!("{name} {v}: program not found"));
                }
            }
        }
    }

    let site_path = expand_tilde(&raw.path, home);
    if !site_path.join("app/public").is_dir() {
        missing.push("site files (app/public) not found".to_string());
    }
    if !local.join("run").join(&raw.id).join("mysql/data").is_dir() {
        missing.push("database folder not found".to_string());
    }

    let running = check_ports && http_port.is_some_and(port_open);

    Site {
        id: raw.id,
        name: raw.name,
        domain: raw.domain,
        path: site_path.to_string_lossy().into_owned(),
        php: version_of("php"),
        mysql: version_of("mysql"),
        nginx: version_of("nginx"),
        http_port,
        running,
        missing,
    }
}

/// Reads Local's registry. Never writes anything.
pub fn list_sites() -> Overview {
    list_sites_in(&local_dir(), &home_dir(), true)
}

pub fn list_sites_in(local: &Path, home: &Path, check_ports: bool) -> Overview {
    let local_dir = local.to_string_lossy().into_owned();
    let registry = local.join("sites.json");

    let text = match fs::read_to_string(&registry) {
        Ok(t) => t,
        Err(_) => {
            return Overview {
                local_dir,
                local_found: local.is_dir(),
                sites: vec![],
                error: Some(format!("Could not read {}", registry.display())),
            }
        }
    };

    match serde_json::from_str::<HashMap<String, RawSite>>(&text) {
        Ok(map) => {
            let mut sites: Vec<Site> = map
                .into_iter()
                .map(|(key, mut raw)| {
                    if raw.id.is_empty() {
                        raw.id = key;
                    }
                    build_site(raw, local, home, check_ports)
                })
                .collect();
            sites.sort_by(|a, b| a.domain.cmp(&b.domain));
            Overview { local_dir, local_found: true, sites, error: None }
        }
        Err(e) => Overview {
            local_dir,
            local_found: true,
            sites: vec![],
            error: Some(format!("sites.json is not in the expected format: {e}")),
        },
    }
}
