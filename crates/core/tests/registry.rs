use std::fs;
use wplocal_core::list_sites_in;

const REGISTRY: &str = r#"{
  "AAA111": {
    "id": "AAA111", "name": "Demo", "path": "~/Local Sites/demo", "domain": "demo.local",
    "mysql": {"database": "local", "user": "root", "password": "x"},
    "services": {
      "mailpit": {"name": "mailpit", "version": "1.24.1", "ports": {"WEB": [10000], "SMTP": [10001]}},
      "php":     {"name": "php", "version": "8.4.10", "ports": {"cgi": [10002]}},
      "mysql":   {"name": "mysql", "version": "8.0.35", "ports": {"MYSQL": [10003]}},
      "nginx":   {"name": "nginx", "version": "1.26.1", "ports": {"HTTP": [10004]}}
    }
  },
  "BBB222": {
    "id": "BBB222", "name": "Shop", "path": "~/Local Sites/shop", "domain": "shop.local",
    "services": {
      "php":   {"name": "php", "version": "8.2.27", "ports": {"cgi": [10007]}},
      "nginx": {"name": "nginx", "version": "1.26.1", "ports": {"HTTP": [10008]}}
    }
  }
}"#;

fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
    let local = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(local.path().join("sites.json"), REGISTRY).unwrap();
    for svc in ["nginx-1.26.1+3", "php-8.4.10+0", "mysql-8.0.35+4"] {
        fs::create_dir_all(local.path().join("lightning-services").join(svc)).unwrap();
    }
    fs::create_dir_all(home.path().join("Local Sites/demo/app/public")).unwrap();
    fs::create_dir_all(local.path().join("run/AAA111/mysql/data")).unwrap();
    (local, home)
}

#[test]
fn complete_site_has_nothing_missing() {
    let (local, home) = setup();
    let overview = list_sites_in(local.path(), home.path(), false);
    assert!(overview.error.is_none());
    let demo = overview.sites.iter().find(|s| s.domain == "demo.local").unwrap();
    assert_eq!(demo.php.as_deref(), Some("8.4.10"));
    assert_eq!(demo.http_port, Some(10004));
    assert!(demo.missing.is_empty(), "unexpected: {:?}", demo.missing);
    assert!(demo.path.ends_with("Local Sites/demo"));
    assert!(!demo.path.starts_with('~'));
}

#[test]
fn incomplete_site_lists_what_is_missing() {
    let (local, home) = setup();
    let overview = list_sites_in(local.path(), home.path(), false);
    let shop = overview.sites.iter().find(|s| s.domain == "shop.local").unwrap();
    let text = shop.missing.join(" | ");
    assert!(text.contains("php 8.2.27"), "{text}");
    assert!(text.contains("mysql: not listed"), "{text}");
    assert!(text.contains("site files"), "{text}");
    assert!(text.contains("database folder"), "{text}");
}

#[test]
fn sites_are_sorted_by_domain() {
    let (local, home) = setup();
    let overview = list_sites_in(local.path(), home.path(), false);
    let domains: Vec<_> = overview.sites.iter().map(|s| s.domain.as_str()).collect();
    assert_eq!(domains, ["demo.local", "shop.local"]);
}

#[test]
fn missing_registry_is_reported_not_fatal() {
    let local = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let overview = list_sites_in(local.path(), home.path(), false);
    assert!(overview.sites.is_empty());
    assert!(overview.error.is_some());
}

#[test]
fn garbage_registry_is_reported_not_fatal() {
    let local = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::write(local.path().join("sites.json"), "[1,2,3]").unwrap();
    let overview = list_sites_in(local.path(), home.path(), false);
    assert!(overview.sites.is_empty());
    assert!(overview.error.unwrap().contains("expected format"));
}
