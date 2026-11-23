use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use wplocal_core::runner::{plan_site, Ready};
use wplocal_core::Runner;

const FAKE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/support/fake_service.py");

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn script(path: &Path, role: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\nexec python3 -u {FAKE} {role} \"$@\"\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    local: tempfile::TempDir,
    home: tempfile::TempDir,
    logs: tempfile::TempDir,
    ports: [u16; 5], // mysql, nginx, mail web, mail smtp, router
}

fn fixture(mysql_role: &str) -> Fixture {
    let local = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let logs = tempfile::tempdir().unwrap();
    let ports = [free_port(), free_port(), free_port(), free_port(), free_port()];
    let l = local.path();

    fs::write(
        l.join("sites.json"),
        format!(
            r#"{{"AAA111": {{"id":"AAA111","name":"Demo","path":"~/Local Sites/demo","domain":"demo.local",
            "services": {{
              "mailpit": {{"version":"1.24.1","ports":{{"WEB":[{}],"SMTP":[{}]}}}},
              "php":     {{"version":"8.4.10","ports":{{"cgi":[1]}}}},
              "mysql":   {{"version":"8.0.35","ports":{{"MYSQL":[{}]}}}},
              "nginx":   {{"version":"1.26.1","ports":{{"HTTP":[{}]}}}} }} }}}}"#,
            ports[2], ports[3], ports[0], ports[1]
        ),
    )
    .unwrap();

    let svc = l.join("lightning-services");
    script(&svc.join("mysql-8.0.35+4/bin/darwin-arm64/bin/mysqld"), mysql_role);
    script(&svc.join("php-8.4.10+0/bin/darwin-arm64/sbin/php-fpm"), "php-fpm");
    script(&svc.join("nginx-1.26.1+3/bin/darwin-arm64/sbin/nginx"), "nginx");
    script(&svc.join("mailpit-1.24.1+0/bin/darwin-arm64/mailpit"), "mailpit");

    let run = l.join("run/AAA111");
    fs::create_dir_all(run.join("conf/nginx")).unwrap();
    fs::create_dir_all(run.join("conf/mysql")).unwrap();
    fs::create_dir_all(run.join("conf/php")).unwrap();
    fs::write(run.join("conf/nginx/nginx.conf"), format!("port={}", ports[1])).unwrap();
    fs::write(run.join("conf/mysql/my.cnf"), format!("[mysqld]\nport = {}\n", ports[0])).unwrap();
    fs::write(
        run.join("conf/php/php-fpm.conf"),
        format!("listen={}", run.join("php/php-fpm.socket").display()),
    )
    .unwrap();
    fs::create_dir_all(l.join("run/router/nginx/conf")).unwrap();
    fs::write(l.join("run/router/nginx/conf/nginx.conf"), format!("port={}", ports[4])).unwrap();

    Fixture { local, home, logs, ports }
}

fn runner(f: &Fixture) -> Runner {
    Runner::with_dirs(f.local.path().to_path_buf(), f.home.path().to_path_buf(), f.logs.path().to_path_buf())
        .with_router_port(f.ports[4])
}

fn open(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

#[test]
fn plan_lists_services_in_start_order_with_local_settings_files() {
    let f = fixture("mysqld");
    let plan = plan_site(f.local.path(), "AAA111").unwrap();
    let names: Vec<_> = plan.services.iter().map(|s| s.name).collect();
    assert_eq!(names, ["mysql", "php", "nginx", "mailpit"]);
    assert_eq!(plan.domain, "demo.local");
    assert_eq!(plan.services[0].ready, Ready::Port(f.ports[0]));
    let mysql_arg = plan.services[0].args[0].to_string_lossy().into_owned();
    assert!(mysql_arg.starts_with("--defaults-file=") && mysql_arg.ends_with("run/AAA111/conf/mysql/my.cnf"));
    assert!(plan.services[3].optional);
    assert_eq!(plan.ports.len(), 4);
}

#[test]
fn plan_explains_a_site_local_never_started() {
    let f = fixture("mysqld");
    fs::remove_file(f.local.path().join("run/AAA111/conf/nginx/nginx.conf")).unwrap();
    let err = plan_site(f.local.path(), "AAA111").unwrap_err();
    assert!(err.contains("never started"), "{err}");
}

#[test]
fn plan_explains_a_missing_program() {
    let f = fixture("mysqld");
    fs::remove_dir_all(f.local.path().join("lightning-services/php-8.4.10+0")).unwrap();
    let err = plan_site(f.local.path(), "AAA111").unwrap_err();
    assert!(err.contains("php 8.4.10"), "{err}");
}

#[test]
fn start_then_stop_runs_and_cleans_up_everything() {
    let f = fixture("mysqld");
    let r = runner(&f);
    r.start_site("AAA111").expect("start");
    for p in [f.ports[0], f.ports[1], f.ports[2], f.ports[4]] {
        assert!(open(p), "port {p} should be open while running");
    }
    let overview = r.overview();
    assert!(overview.sites[0].running && overview.sites[0].managed);

    r.stop_site("AAA111").expect("stop");
    for p in [f.ports[0], f.ports[1], f.ports[2], f.ports[4]] {
        assert!(!open(p), "port {p} should be closed after stopping");
    }
    assert!(!r.overview().sites[0].managed);
}

#[test]
fn starting_twice_is_harmless() {
    let f = fixture("mysqld");
    let r = runner(&f);
    r.start_site("AAA111").unwrap();
    r.start_site("AAA111").unwrap();
    r.stop_all();
    assert!(!open(f.ports[1]));
}

#[test]
fn refuses_when_a_port_is_taken() {
    let f = fixture("mysqld");
    let _taken = TcpListener::bind(("127.0.0.1", f.ports[1])).unwrap();
    let err = runner(&f).start_site("AAA111").unwrap_err();
    assert!(err.contains("already in use"), "{err}");
}

#[test]
fn a_failing_program_is_reported_with_its_log_and_nothing_is_left_running() {
    let f = fixture("crash");
    let err = runner(&f).start_site("AAA111").unwrap_err();
    assert!(err.contains("mysql did not start"), "{err}");
    assert!(err.contains("fake failure"), "{err}");
    assert!(!open(f.ports[1]));
}

#[test]
fn cannot_stop_a_site_this_app_did_not_start() {
    let f = fixture("mysqld");
    let err = runner(&f).stop_site("AAA111").unwrap_err();
    assert!(err.contains("did not start this site"), "{err}");
    let _unused: PathBuf = f.logs.path().to_path_buf();
}
