use std::{
    fs,
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const KITSU_NOT_FOUND: &str = r#"{"errors":[{"title":"Record not found","status":"404","code":"404"}]}"#;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meta")
}

struct DataDir(PathBuf);

impl DataDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("kitsu-addon-{}-{name}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::copy(fixtures().join("records.jsonl"), dir.join("search_titles.jsonl")).unwrap();
        fs::copy(fixtures().join("mapping_rows.json"), dir.join("imdb_mapping.json")).unwrap();
        fs::write(dir.join("search_aliases.json"), "{}").unwrap();
        Self(dir)
    }
}

impl Drop for DataDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct SlowKitsu {
    port: u16,
    requests: Arc<AtomicUsize>,
}

async fn slow_kitsu(delay: Duration) -> SlowKitsu {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let requests = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&requests);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).await.unwrap_or(0);
                if String::from_utf8_lossy(&buffer[..read]).contains("/anime/") {
                    seen.fetch_add(1, Ordering::SeqCst);
                }
                tokio::time::sleep(delay).await;
                let reply = format!(
                    "HTTP/1.1 404 Not Found\r\ncontent-type: application/vnd.api+json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{KITSU_NOT_FOUND}",
                    KITSU_NOT_FOUND.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    SlowKitsu { port, requests }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port()
}

struct Addon {
    child: Child,
    port: u16,
}

impl Addon {
    async fn start(data: &DataDir, kitsu_port: Option<u16>) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_kitsu-addon"));
        command
            .env_clear()
            .env("PORT", port.to_string())
            .env("NO_CACHE", "true")
            .env("FANART_APIKEY", "test")
            .env("DATA_DIR", &data.0)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        command.env("KITSU_URL", format!("http://127.0.0.1:{}", kitsu_port.unwrap_or_else(free_port)));
        let mut addon = Self { child: command.spawn().unwrap(), port };
        addon.wait_until_serving().await;
        addon
    }

    async fn wait_until_serving(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            assert!(self.child.try_wait().unwrap().is_none(), "the addon exited while starting");
            if TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the addon did not start");
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    fn signal(&self, name: &str) {
        let status = Command::new("kill").arg(format!("-{name}")).arg(self.child.id().to_string()).status().unwrap();
        assert!(status.success());
    }

    async fn exit(&mut self, within: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        None
    }
}

impl Drop for Addon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn get(url: String) -> (u16, String) {
    let response = reqwest::get(url).await.unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

#[tokio::test]
async fn the_binary_serves_the_manifest_and_searches_the_data_dir() {
    let data = DataDir::new("smoke");
    let addon = Addon::start(&data, None).await;

    let (status, body) = get(addon.url("/manifest.json")).await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], "community.anime.kitsu");

    let (status, body) = get(addon.url("/search/naruto.json")).await;
    assert_eq!(status, 200);
    let ids = serde_json::from_str::<Value>(&body).unwrap()["ids"].clone();
    assert_eq!(ids[0], "11");
}

#[tokio::test]
async fn the_binary_fails_to_start_without_the_data_files() {
    let data = DataDir::new("missing");
    fs::remove_file(data.0.join("search_titles.jsonl")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_kitsu-addon"))
        .env_clear()
        .env("PORT", free_port().to_string())
        .env("NO_CACHE", "true")
        .env("DATA_DIR", &data.0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
}

#[tokio::test]
async fn sigterm_stops_an_idle_addon_with_exit_code_0() {
    let data = DataDir::new("idle");
    let mut addon = Addon::start(&data, None).await;
    addon.signal("TERM");
    assert_eq!(addon.exit(Duration::from_secs(5)).await.and_then(|status| status.code()), Some(0));
}

#[tokio::test]
async fn sigint_stops_the_addon_with_exit_code_0() {
    let data = DataDir::new("interrupt");
    let mut addon = Addon::start(&data, None).await;
    addon.signal("INT");
    assert_eq!(addon.exit(Duration::from_secs(5)).await.and_then(|status| status.code()), Some(0));
}

#[tokio::test]
async fn sigterm_lets_an_in_flight_request_finish_before_exiting_0() {
    let data = DataDir::new("in-flight");
    let kitsu = slow_kitsu(Duration::from_millis(1500)).await;
    let mut addon = Addon::start(&data, Some(kitsu.port)).await;

    let request = tokio::spawn(get(addon.url("/meta/anime/kitsu:1.json")));
    while kitsu.requests.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    addon.signal("TERM");

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(addon.child.try_wait().unwrap().is_none(), "the addon exited with a request in flight");
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, addon.port)).await.is_err(),
        "the addon still accepts connections"
    );

    let (status, body) = request.await.unwrap();
    assert_eq!((status, body.as_str()), (404, r#"{"err":"not found"}"#));
    assert_eq!(addon.exit(Duration::from_secs(5)).await.and_then(|status| status.code()), Some(0));
}

#[tokio::test]
async fn a_second_sigterm_exits_immediately_with_exit_code_0() {
    let data = DataDir::new("second");
    let kitsu = slow_kitsu(Duration::from_secs(60)).await;
    let mut addon = Addon::start(&data, Some(kitsu.port)).await;

    let request = tokio::spawn(get(addon.url("/meta/anime/kitsu:1.json")));
    while kitsu.requests.load(Ordering::SeqCst) == 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    addon.signal("TERM");
    assert!(addon.exit(Duration::from_millis(500)).await.is_none(), "the addon should wait for the request");
    addon.signal("TERM");
    assert_eq!(addon.exit(Duration::from_secs(2)).await.and_then(|status| status.code()), Some(0));
    request.abort();
}
