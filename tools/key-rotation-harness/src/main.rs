//! A stand-in for an installed SketchCraft's updater, to rehearse the 0.3.6 key rotation.
//!
//! It repeats, step for step, what tauri-plugin-updater 2.14.1 does with a static manifest:
//! fetch the endpoint, parse `version`, ask the app's own comparator (`release.version > current`
//! and every download under that version's own release address: copied from
//! app/src-tauri/src/main.rs), pick the platform's entry, download, and check the signature with
//! `minisign_verify` exactly as the plugin's `verify_signature` does (allow-legacy, then the
//! signed version in the trusted comment against the announced one). A "client" is a version plus
//! the public key and endpoint that version shipped with:
//!   <= 0.3.5: the OLD public key, .../releases/latest/download/latest.json
//!   >= 0.3.6: the NEW public key, the endpoint in the repository's tauri.conf.json
//! Network is a local HTTP server standing in for github.com (URLs are rewritten to it).
//!
//! usage: harness --base http://127.0.0.1:PORT/state --current 0.3.5 --old-pub F --new-pub F --conf tauri.conf.json [--follow]
use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use std::io::{Read, Write};
use std::net::TcpStream;

const RELEASES: &str = "https://github.com/jasonsutter87/sketchcraft/releases/";
const OLD_ENDPOINT: &str = "https://github.com/jasonsutter87/sketchcraft/releases/latest/download/latest.json";
const PLATFORM: &str = "darwin-aarch64";

// ---- copied from app/src-tauri/src/main.rs ----
const RELEASE_BASE: &str = "https://github.com/jasonsutter87/sketchcraft/releases/download/";
fn release_url_ok(version: &str, url: &str) -> bool {
    url.starts_with(&format!("{RELEASE_BASE}v{version}/"))
}

fn get(base: &str, url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix(RELEASES).ok_or("url is not a github releases url")?;
    let local = format!("{base}/releases/{rest}");
    let hostport = local.strip_prefix("http://").ok_or("base must be http")?;
    let (host, path) = hostport.split_once('/').ok_or("bad base")?;
    if !host.starts_with("127.0.0.1:") { return Err("refusing a non-local host".into()); }
    let mut s = TcpStream::connect(host).map_err(|e| e.to_string())?;
    write!(s, "GET /{path} HTTP/1.0\r\nHost: {host}\r\n\r\n").map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no http body")? + 4;
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    if !head.starts_with("HTTP/1.0 200") { return Err(format!("http error: {}", head.lines().next().unwrap_or(""))); }
    Ok(buf[split..].to_vec())
}

fn b64s(s: &str) -> Result<String, String> {
    let d = base64::engine::general_purpose::STANDARD.decode(s.trim()).map_err(|e| e.to_string())?;
    String::from_utf8(d).map_err(|_| "not utf8".into())
}

// ---- the plugin's verify_signature (2.14.1), with require_signed_version off like the app ----
fn verify_signature(data: &[u8], release_signature: &str, pub_key: &str, announced: &str) -> Result<(), String> {
    let public_key = PublicKey::decode(&b64s(pub_key)?).map_err(|e| e.to_string())?;
    let signature = Signature::decode(&b64s(release_signature)?).map_err(|e| e.to_string())?;
    public_key.verify(data, &signature, true).map_err(|e| format!("signature check failed: {e}"))?;
    if let Some(sv) = signature.trusted_comment().split('\t').find_map(|f| f.strip_prefix("version:")) {
        let same = match (Version::parse(sv.trim_start_matches('v')), Version::parse(announced.trim_start_matches('v'))) {
            (Ok(x), Ok(y)) => x == y,
            _ => sv == announced,
        };
        if !same {
            return Err(format!("signed for {sv}, announced {announced}"));
        }
    }
    Ok(())
}

enum Outcome { NoUpdate, Accepted(Version), Rejected(String) }

fn poll(base: &str, current: &Version, old_pub: &str, new_pub: &str, conf_endpoint: &str) -> Outcome {
    let (pubkey, endpoint) = if *current < Version::new(0, 3, 6) { (old_pub, OLD_ENDPOINT) } else { (new_pub, conf_endpoint) };
    let body = match get(base, endpoint) { Ok(b) => b, Err(e) => return Outcome::Rejected(format!("fetch {endpoint}: {e}")) };
    let m: serde_json::Value = match serde_json::from_slice(&body) { Ok(v) => v, Err(e) => return Outcome::Rejected(format!("manifest: {e}")) };
    let ver = match m["version"].as_str().and_then(|v| Version::parse(v.trim_start_matches('v')).ok()) { Some(v) => v, None => return Outcome::Rejected("manifest has no version".into()) };
    // the app's comparator: newer, and every download from its own release
    let platforms = match m["platforms"].as_object() { Some(p) if !p.is_empty() => p, _ => return Outcome::Rejected("no platforms".into()) };
    if ver <= *current { return Outcome::NoUpdate; }
    if !platforms.values().all(|p| release_url_ok(&ver.to_string(), p["url"].as_str().unwrap_or(""))) { return Outcome::NoUpdate; }
    let p = match platforms.get(PLATFORM) { Some(p) => p, None => return Outcome::Rejected("no entry for this platform".into()) };
    let bundle = match get(base, p["url"].as_str().unwrap()) { Ok(b) => b, Err(e) => return Outcome::Rejected(format!("download: {e}")) };
    match verify_signature(&bundle, p["signature"].as_str().unwrap_or(""), pubkey, &ver.to_string()) {
        Ok(()) => Outcome::Accepted(ver),
        Err(e) => Outcome::Rejected(e),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let arg = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned().unwrap_or_else(|| { eprintln!("missing {k}"); std::process::exit(2) });
    let rd = |p: String| std::fs::read_to_string(p).unwrap().trim().to_string();
    let base = arg("--base");
    let mut current = Version::parse(&arg("--current")).unwrap();
    let (old_pub, new_pub) = (rd(arg("--old-pub")), rd(arg("--new-pub")));
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(arg("--conf")).unwrap()).unwrap();
    let endpoint = conf["plugins"]["updater"]["endpoints"][0].as_str().unwrap().to_string();
    let follow = a.iter().any(|x| x == "--follow");
    for _ in 0..5 {
        match poll(&base, &current, &old_pub, &new_pub, &endpoint) {
            Outcome::NoUpdate => { println!("{current}: no update"); return; }
            Outcome::Accepted(v) => { println!("{current}: ACCEPTED {v}"); current = v; if !follow { return; } }
            Outcome::Rejected(why) => { println!("{current}: REJECTED ({why})"); return; }
        }
    }
}
