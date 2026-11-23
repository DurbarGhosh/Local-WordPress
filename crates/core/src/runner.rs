//! Starts and stops a site's programs.
//!
//! This reuses the settings files Local already generated for each site
//! (`run/<site id>/conf/...`), so the programs run exactly as Local runs them.
//! A site is refused if any of its ports are already taken, which is how the
//! app avoids clashing with Local.

use crate::{find_service_dir, home_dir, list_sites_in, local_dir, port_open, Overview};
use serde::Deserialize;
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// How the app knows a program is ready to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ready {
    Port(u16),
    Socket(PathBuf),
    /// Nothing to wait for; only checks the program did not exit at once.
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    pub name: &'static str,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    /// A failure of an optional program does not stop the site.
    pub optional: bool,
    pub ready: Ready,
    pub wait_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePlan {
    pub id: String,
    pub domain: String,
    /// In start order: MySQL, PHP, nginx, Mailpit.
    pub services: Vec<ServiceSpec>,
    /// Every port the site needs to be free before it starts.
    pub ports: Vec<u16>,
    pub router: ServiceSpec,
}

#[derive(Deserialize)]
struct RegSite {
    #[serde(default)]
    domain: String,
    #[serde(default)]
    services: HashMap<String, RegService>,
}

#[derive(Deserialize)]
struct RegService {
    #[serde(default)]
    version: String,
    #[serde(default)]
    ports: HashMap<String, Vec<u16>>,
}

fn port_of(site: &RegSite, service: &str, key: &str) -> Option<u16> {
    site.services.get(service)?.ports.get(key)?.first().copied()
}

fn platform_dir(svc: &Path) -> Option<PathBuf> {
    fs::read_dir(svc.join("bin"))
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("darwin")))
}

fn find_exe(dir: &Path, name: &str, depth: u8) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    if let Some(found) = entries.iter().find(|p| p.is_file() && p.file_name().is_some_and(|n| n == name)) {
        return Some(found.clone());
    }
    if depth == 0 {
        return None;
    }
    entries.iter().filter(|p| p.is_dir()).find_map(|p| find_exe(p, name, depth - 1))
}

/// Finds the program `exe` inside the installed service `service`.
fn locate(local: &Path, site: &RegSite, service: &str, exe: &str) -> Result<PathBuf, String> {
    let version = site
        .services
        .get(service)
        .map(|s| s.version.as_str())
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{service} is not listed for this site in Local"))?;
    let dir = find_service_dir(local, service, version)
        .ok_or_else(|| format!("{service} {version} is not installed in Local's folder"))?;
    let platform = platform_dir(&dir).ok_or_else(|| format!("{service} {version} has no Mac program folder"))?;
    find_exe(&platform, exe, 3).ok_or_else(|| format!("Could not find {exe} inside {service} {version}"))
}

/// Works out the exact commands for a site without running anything.
pub fn plan_site(local: &Path, id: &str) -> Result<SitePlan, String> {
    let text = fs::read_to_string(local.join("sites.json"))
        .map_err(|e| format!("Cannot read Local's site list: {e}"))?;
    let map: HashMap<String, RegSite> =
        serde_json::from_str(&text).map_err(|e| format!("Local's site list has an unexpected format: {e}"))?;
    let site = map.get(id).ok_or_else(|| format!("Site {id} is not in Local's site list"))?;

    let run = local.join("run").join(id);
    let conf = run.join("conf");
    if !conf.join("nginx/nginx.conf").is_file() {
        return Err("Local has never started this site, so its settings files do not exist yet. Start it once in Local first.".into());
    }
    let router_conf = local.join("run/router/nginx/conf/nginx.conf");
    if !router_conf.is_file() {
        return Err("Local's router settings were not found. Open Local once, then try again.".into());
    }

    let mysql_port = port_of(site, "mysql", "MYSQL").ok_or("No MySQL port in Local's site list")?;
    let http_port = port_of(site, "nginx", "HTTP").ok_or("No web server port in Local's site list")?;
    let mail_web = port_of(site, "mailpit", "WEB");
    let mail_smtp = port_of(site, "mailpit", "SMTP");

    let mysqld = locate(local, site, "mysql", "mysqld")?;
    let php_fpm = locate(local, site, "php", "php-fpm")?;
    let nginx = locate(local, site, "nginx", "nginx")?;
    let mailpit = locate(local, site, "mailpit", "mailpit").ok();

    let mut php_args: Vec<OsString> = vec!["-y".into(), conf.join("php/php-fpm.conf").into_os_string()];
    if conf.join("php/php.ini").is_file() {
        php_args.push("-c".into());
        php_args.push(conf.join("php/php.ini").into_os_string());
    }
    // php-fpm resolves `include=php-fpm.d/*.conf` relative to its prefix, which
    // otherwise points at the folder it was compiled in on the vendor's machine.
    php_args.push("-p".into());
    php_args.push(conf.join("php").into_os_string());
    php_args.push("-F".into());

    let mut services = vec![
        ServiceSpec {
            name: "mysql",
            program: mysqld,
            args: vec![format!("--defaults-file={}", conf.join("mysql/my.cnf").display()).into()],
            cwd: run.clone(),
            optional: false,
            ready: Ready::Port(mysql_port),
            wait_secs: 90,
        },
        ServiceSpec {
            name: "php",
            program: php_fpm,
            args: php_args,
            cwd: run.clone(),
            optional: false,
            ready: Ready::Socket(run.join("php/php-fpm.socket")),
            wait_secs: 20,
        },
        ServiceSpec {
            name: "nginx",
            program: nginx.clone(),
            args: vec![
                "-g".into(),
                "daemon off;".into(),
                "-c".into(),
                conf.join("nginx/nginx.conf").into_os_string(),
                "-p".into(),
                run.join("nginx").into_os_string(),
            ],
            cwd: run.clone(),
            optional: false,
            ready: Ready::Port(http_port),
            wait_secs: 20,
        },
    ];
    let mut ports = vec![mysql_port, http_port];

    if let (Some(program), Some(web), Some(smtp)) = (mailpit, mail_web, mail_smtp) {
        services.push(ServiceSpec {
            name: "mailpit",
            program,
            args: vec![
                "--listen".into(),
                format!("127.0.0.1:{web}").into(),
                "--smtp".into(),
                format!("127.0.0.1:{smtp}").into(),
                "--database".into(),
                run.join("mailpit/mailpit.db").into_os_string(),
            ],
            cwd: run.clone(),
            optional: true,
            ready: Ready::Nothing,
            wait_secs: 0,
        });
        ports.push(web);
        ports.push(smtp);
    }

    let router_dir = local.join("run/router/nginx");
    let router = ServiceSpec {
        name: "router",
        program: nginx,
        args: vec!["-c".into(), router_conf.into_os_string(), "-p".into(), router_dir.clone().into_os_string()],
        cwd: router_dir,
        optional: false,
        ready: Ready::Port(80), // replaced by the runner's router port
        wait_secs: 15,
    };

    Ok(SitePlan { id: id.to_string(), domain: site.domain.clone(), services, ports, router })
}

struct Running {
    name: &'static str,
    child: Child,
}

#[derive(Default)]
struct State {
    sites: HashMap<String, Vec<Running>>,
    /// Set only when this app started the router itself.
    router: Option<Running>,
}

/// Starts and stops sites. One lives for the whole app session.
pub struct Runner {
    local: PathBuf,
    home: PathBuf,
    log_dir: PathBuf,
    router_port: u16,
    state: Mutex<State>,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner {
    pub fn new() -> Self {
        let home = home_dir();
        let log_dir = home.join("Library/Logs/WP Local Lite");
        Self::with_dirs(local_dir(), home, log_dir)
    }

    pub fn with_dirs(local: PathBuf, home: PathBuf, log_dir: PathBuf) -> Self {
        Self { local, home, log_dir, router_port: 80, state: Mutex::new(State::default()) }
    }

    /// Only for tests, which cannot use port 80.
    pub fn with_router_port(mut self, port: u16) -> Self {
        self.router_port = port;
        self
    }

    /// The site list, marked with which sites this app can stop.
    pub fn overview(&self) -> Overview {
        let mut overview = list_sites_in(&self.local, &self.home, true);
        let state = self.state.lock().unwrap();
        for site in &mut overview.sites {
            site.managed = state.sites.contains_key(&site.id);
        }
        overview
    }

    pub fn start_site(&self, id: &str) -> Result<(), String> {
        if self.state.lock().unwrap().sites.contains_key(id) {
            return Ok(());
        }
        let plan = plan_site(&self.local, id)?;

        for port in &plan.ports {
            if port_open(*port) {
                return Err(format!(
                    "Port {port} is already in use. If Local is running this site, stop it there first."
                ));
            }
        }
        fs::create_dir_all(&self.log_dir).map_err(|e| format!("Cannot create the log folder: {e}"))?;
        let run = self.local.join("run").join(id);
        for dir in ["php", "mailpit", "nginx/logs"] {
            let _ = fs::create_dir_all(run.join(dir));
        }
        let _ = fs::remove_file(run.join("php/php-fpm.socket")); // stale leftover

        let mut started: Vec<Running> = Vec::new();
        for spec in &plan.services {
            let log_name = format!("{id}-{}", spec.name);
            let child = match self.spawn(&log_name, spec) {
                Ok(c) => c,
                Err(e) if spec.optional => {
                    let _ = e;
                    continue;
                }
                Err(e) => {
                    terminate_all(started);
                    return Err(e);
                }
            };
            started.push(Running { name: spec.name, child });
            let last = started.last_mut().unwrap();
            if let Err(why) = wait_ready(&mut last.child, &spec.ready, spec.wait_secs) {
                if spec.optional {
                    continue;
                }
                let tail = self.log_tail(&log_name);
                terminate_all(started);
                return Err(format!("{} did not start: {why}{tail}", spec.name));
            }
        }

        if let Err(e) = self.ensure_router(&plan.router) {
            terminate_all(started);
            return Err(e);
        }
        self.state.lock().unwrap().sites.insert(id.to_string(), started);
        Ok(())
    }

    pub fn stop_site(&self, id: &str) -> Result<(), String> {
        let (running, router) = {
            let mut state = self.state.lock().unwrap();
            let running = state.sites.remove(id);
            let router = if state.sites.is_empty() { state.router.take() } else { None };
            (running, router)
        };
        let Some(running) = running else {
            return Err("This app did not start this site, so it cannot stop it. Stop it where you started it.".into());
        };
        terminate_all(running);
        if let Some(router) = router {
            terminate_all(vec![router]);
        }
        Ok(())
    }

    /// Stops everything this app started. Called when the app quits.
    pub fn stop_all(&self) {
        let (sites, router) = {
            let mut state = self.state.lock().unwrap();
            (std::mem::take(&mut state.sites), state.router.take())
        };
        for (_, running) in sites {
            terminate_all(running);
        }
        if let Some(router) = router {
            terminate_all(vec![router]);
        }
    }

    fn ensure_router(&self, spec: &ServiceSpec) -> Result<(), String> {
        if port_open(self.router_port) {
            return Ok(()); // Local's router, or ours from an earlier site
        }
        let mut spec = spec.clone();
        spec.ready = Ready::Port(self.router_port);
        let child = self.spawn("router", &spec)?;
        let mut running = Running { name: "router", child };
        if let Err(why) = wait_ready(&mut running.child, &spec.ready, spec.wait_secs) {
            let tail = self.log_tail("router");
            terminate_all(vec![running]);
            return Err(format!("The router did not start: {why}{tail}"));
        }
        self.state.lock().unwrap().router = Some(running);
        Ok(())
    }

    fn spawn(&self, log_name: &str, spec: &ServiceSpec) -> Result<Child, String> {
        let path = self.log_dir.join(format!("{log_name}.log"));
        let log = File::create(&path).map_err(|e| format!("Cannot create {}: {e}", path.display()))?;
        let log_err = log.try_clone().map_err(|e| e.to_string())?;
        Command::new(&spec.program)
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .spawn()
            .map_err(|e| format!("Could not start {} ({}): {e}", spec.name, spec.program.display()))
    }

    fn log_tail(&self, log_name: &str) -> String {
        let path = self.log_dir.join(format!("{log_name}.log"));
        let Ok(text) = fs::read_to_string(&path) else { return String::new() };
        let mut lines: Vec<&str> = text.lines().rev().take(12).collect();
        if lines.is_empty() {
            return String::new();
        }
        lines.reverse();
        format!("\n\nLast log lines ({}):\n{}", path.display(), lines.join("\n"))
    }
}

fn is_ready(ready: &Ready) -> bool {
    match ready {
        Ready::Port(p) => port_open(*p),
        Ready::Socket(path) => UnixStream::connect(path).is_ok(),
        Ready::Nothing => true,
    }
}

fn wait_ready(child: &mut Child, ready: &Ready, secs: u64) -> Result<(), String> {
    if *ready == Ready::Nothing {
        sleep(Duration::from_millis(400));
        return match child.try_wait() {
            Ok(Some(status)) => Err(format!("exited at once ({status})")),
            _ => Ok(()),
        };
    }
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("stopped unexpectedly ({status})"));
        }
        if is_ready(ready) {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("not ready after {secs} seconds"));
        }
        sleep(Duration::from_millis(250));
    }
}

/// Stops programs in the reverse of their start order.
fn terminate_all(mut running: Vec<Running>) {
    while let Some(mut r) = running.pop() {
        terminate(&mut r);
    }
}

/// Asks politely first. MySQL is never force-killed, so a database is not damaged.
fn terminate(r: &mut Running) {
    unsafe {
        libc::kill(r.child.id() as i32, libc::SIGTERM);
    }
    let grace = if r.name == "mysql" { 120 } else { 10 };
    let deadline = Instant::now() + Duration::from_secs(grace);
    loop {
        match r.child.try_wait() {
            Ok(None) => {}
            _ => return,
        }
        if Instant::now() > deadline {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    if r.name != "mysql" {
        let _ = r.child.kill();
        let _ = r.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminate_stops_a_running_program() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let mut r = Running { name: "nginx", child };
        let started = Instant::now();
        terminate(&mut r);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(r.child.try_wait().unwrap().is_some());
    }
}
