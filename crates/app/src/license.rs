//! 라이선스: a key bought on the sales site (site/) is registered for this computer, and the
//! server's answer — a token signed with Ed25519 — is kept and checked offline at every start.
//!
//! Token: `<payload>.<signature>`, both base64url; the signature covers the ASCII bytes of
//! `<payload>` (see site/functions/src/tokens.js). The public key and the site's address come from
//! `assets/license.json`; while they are empty the feature is off (the settings section is hidden).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

static CONFIG_JSON: &str = include_str!("../assets/license.json");

/// Where the sales site is, and the key its tokens are signed with.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LicenseConfig {
    #[serde(default)]
    pub server: String,
    #[serde(default, rename = "publicKey")]
    pub public_key: String,
}

impl LicenseConfig {
    /// The built-in configuration — in debug builds the environment can point it at a local
    /// server (`PHOTO_ORGANIZE_LICENSE_SERVER`, `PHOTO_ORGANIZE_LICENSE_PUBLIC_KEY`). `None` while it
    /// is not set up.
    pub fn get() -> Option<LicenseConfig> {
        let mut c: LicenseConfig = serde_json::from_str(CONFIG_JSON).unwrap_or_default();
        if cfg!(debug_assertions) {
            if let Ok(v) = std::env::var("PHOTO_ORGANIZE_LICENSE_SERVER") {
                c.server = v;
            }
            if let Ok(v) = std::env::var("PHOTO_ORGANIZE_LICENSE_PUBLIC_KEY") {
                c.public_key = v;
            }
        }
        c.server = c.server.trim().trim_end_matches('/').to_string();
        (!c.server.is_empty() && public_key_bytes(&c.public_key).is_some()).then_some(c)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server)
    }
}

/// What the server vouches for (the token's payload).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Claims {
    pub v: u32,
    /// License id.
    pub lid: String,
    pub key: String,
    pub product: String,
    pub email: String,
    /// The computer it was issued for ([`machine_id`]).
    pub mid: String,
    /// Computers allowed.
    pub max: u32,
    /// New versions are free until this day (YYYY-MM-DD).
    pub upd: String,
    /// Issued at (Unix seconds).
    pub iat: i64,
}

/// Kept in the config folder (`license.json`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stored {
    pub key: String,
    pub token: String,
    /// Last time the server confirmed the license (Unix seconds).
    #[serde(default)]
    pub checked_at: i64,
}

/// A license registered for this computer, its token verified.
#[derive(Clone, Debug)]
pub struct License {
    pub stored: Stored,
    pub claims: Claims,
}

impl License {
    /// `PO-VR7D0-•••••-•••••-H8C36`
    pub fn masked_key(&self) -> String {
        let parts: Vec<&str> = self.claims.key.split('-').collect();
        if parts.len() != 5 {
            return self.claims.key.clone();
        }
        format!("{}-{}-•••••-•••••-{}", parts[0], parts[1], parts[4])
    }

    /// The server has not been asked for a while (to notice refunds); `now` in Unix seconds.
    pub fn due_for_check(&self, now: i64) -> bool {
        now - self.stored.checked_at.max(self.claims.iat) > 7 * 24 * 3600
    }
}

#[derive(Debug)]
pub enum LicenseError {
    /// No answer (offline, server down).
    Network(String),
    /// The server said no.
    Server { code: String, message: String },
    /// The answer could not be trusted.
    BadToken,
}

impl LicenseError {
    /// The license itself is gone (refunded, revoked, unknown) — not just unreachable.
    pub fn is_revoked(&self) -> bool {
        matches!(self, LicenseError::Server { code, .. } if code == "REVOKED" || code == "INVALID_KEY")
    }
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LicenseError::Network(detail) => {
                write!(f, "라이선스 서버에 연결하지 못했습니다. 인터넷 연결을 확인해 주세요. ({detail})")
            }
            LicenseError::Server { message, .. } => write!(f, "{message}"),
            LicenseError::BadToken => write!(f, "라이선스 서버의 응답을 확인하지 못했습니다. 앱을 업데이트해 주세요."),
        }
    }
}

// ---- keys ----

const ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A key as typed or pasted (spaces, lower case, I/L for 1, O for 0, no dashes) in its canonical
/// form `PO-XXXXX-XXXXX-XXXXX-XXXXX`, or `None` when it cannot be one.
pub fn normalize_key(input: &str) -> Option<String> {
    let mut s: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect::<String>().to_uppercase();
    if let Some(rest) = s.strip_prefix("PO") {
        s = rest.to_string();
    }
    let s: String = s
        .chars()
        .map(|c| match c {
            'I' | 'L' => '1',
            'O' => '0',
            c => c,
        })
        .collect();
    if s.chars().count() != 20 || !s.chars().all(|c| ALPHABET.contains(c)) {
        return None;
    }
    Some(format!("PO-{}-{}-{}-{}", &s[0..5], &s[5..10], &s[10..15], &s[15..20]))
}

// ---- base64 (standard or url-safe, padding optional) ----

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The 32-byte Ed25519 public key from base64(url) raw bytes or a PEM `PUBLIC KEY` block.
pub fn public_key_bytes(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let body: String = s.lines().filter(|l| !l.starts_with("-----")).collect();
    let bytes = base64_decode(&body)?;
    // SPKI DER for Ed25519 is a 12-byte header and the key.
    const SPKI: [u8; 12] = [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00];
    let raw = match bytes.len() {
        32 => &bytes[..],
        44 if bytes[..12] == SPKI => &bytes[12..],
        _ => return None,
    };
    raw.try_into().ok()
}

/// The token's claims if its signature is valid.
pub fn verify(token: &str, public_key: &str) -> Option<Claims> {
    let (body, sig) = token.split_once('.')?;
    let sig = base64_decode(sig)?;
    let key = public_key_bytes(public_key)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key).verify(body.as_bytes(), &sig).ok()?;
    serde_json::from_slice(&base64_decode(body)?).ok()
}

// ---- this computer ----

fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("photo-organize"))
}

/// A stable, anonymous id for this computer: a hash of the OS's machine id (never the id itself).
pub fn machine_id() -> String {
    let raw = machine_uid::get().unwrap_or_else(|_| fallback_machine_id());
    blake3::hash(format!("photo-organize/license/{}", raw.trim()).as_bytes()).to_hex()[..32].to_string()
}

/// When the OS has no machine id: one made up once and kept in the config folder.
fn fallback_machine_id() -> String {
    let path = config_dir().map(|d| d.join("machine-id"));
    if let Some(id) = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()) {
        if !id.trim().is_empty() {
            return id.trim().to_string();
        }
    }
    let seed = format!("{:?}{}{:p}", std::time::SystemTime::now(), std::process::id(), &path);
    let id = blake3::hash(seed.as_bytes()).to_hex()[..32].to_string();
    if let Some(p) = path {
        let _ = p.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(p, &id);
    }
    id
}

/// The computer's name as its owner knows it (shown on the website's device list).
pub fn machine_name() -> String {
    let name = if cfg!(target_os = "macos") {
        std::process::Command::new("scutil")
            .args(["--get", "ComputerName"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
    } else if cfg!(windows) {
        std::env::var("COMPUTERNAME").ok()
    } else {
        std::fs::read_to_string("/etc/hostname").ok().or_else(|| std::env::var("HOSTNAME").ok())
    };
    name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| "이 컴퓨터".into())
}

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    }
}

// ---- storage ----

fn stored_path() -> Option<PathBuf> {
    Some(config_dir()?.join("license.json"))
}

fn save(s: &Stored) -> Result<()> {
    let path = stored_path().context("설정 폴더를 찾지 못했습니다")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

fn forget() {
    if let Some(p) = stored_path() {
        let _ = std::fs::remove_file(p);
    }
}

/// The license registered for this computer, if there is one and its token checks out.
pub fn load(cfg: &LicenseConfig) -> Option<License> {
    let stored: Stored = serde_json::from_slice(&std::fs::read(stored_path()?).ok()?).ok()?;
    check(cfg, stored).ok()
}

fn check(cfg: &LicenseConfig, stored: Stored) -> Result<License, LicenseError> {
    let claims = verify(&stored.token, &cfg.public_key).ok_or(LicenseError::BadToken)?;
    if claims.mid != machine_id() || normalize_key(&stored.key).as_deref() != Some(claims.key.as_str()) {
        return Err(LicenseError::BadToken);
    }
    Ok(License { stored, claims })
}

// ---- the server ----

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn post(cfg: &LicenseConfig, path: &str, body: &serde_json::Value) -> Result<serde_json::Value, LicenseError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut res = agent.post(cfg.url(path)).send_json(body).map_err(|e| LicenseError::Network(e.to_string()))?;
    let status = res.status().as_u16();
    let value: serde_json::Value =
        res.body_mut().read_json().map_err(|e| LicenseError::Network(format!("HTTP {status}: {e}")))?;
    if (200..300).contains(&status) {
        return Ok(value);
    }
    let err = &value["error"];
    Err(LicenseError::Server {
        code: err["code"].as_str().unwrap_or("ERROR").to_string(),
        message: err["message"].as_str().map(str::to_string).unwrap_or_else(|| format!("서버 오류 ({status})")),
    })
}

/// Register this computer for the license `key` (again: refresh it) and keep the token.
pub fn activate(cfg: &LicenseConfig, key: &str) -> Result<License, LicenseError> {
    let key = normalize_key(key).ok_or_else(|| LicenseError::Server {
        code: "INVALID_KEY".into(),
        message: "라이선스 키 형식이 아닙니다 (PO-XXXXX-XXXXX-XXXXX-XXXXX).".into(),
    })?;
    let body = serde_json::json!({
        "key": key,
        "machineId": machine_id(),
        "machineName": machine_name(),
        "os": os_name(),
        "appVersion": env!("CARGO_PKG_VERSION"),
    });
    let res = post(cfg, "/api/licenses/activate", &body)?;
    let token = res["token"].as_str().ok_or(LicenseError::BadToken)?;
    let lic = check(cfg, Stored { key, token: token.to_string(), checked_at: now() })?;
    save(&lic.stored).map_err(|e| LicenseError::Network(format!("{e:#}")))?;
    Ok(lic)
}

/// Ask the server again (refreshes the token; a refunded license is removed from this computer).
pub fn refresh(cfg: &LicenseConfig, lic: &License) -> Result<License, LicenseError> {
    match activate(cfg, &lic.stored.key) {
        Err(e) if e.is_revoked() => {
            forget();
            Err(e)
        }
        r => r,
    }
}

/// Free this computer's seat and forget the license here.
pub fn deactivate(cfg: &LicenseConfig, lic: &License) -> Result<(), LicenseError> {
    let body = serde_json::json!({ "key": lic.claims.key, "machineId": machine_id() });
    match post(cfg, "/api/licenses/deactivate", &body) {
        Ok(_) => {}
        // Already gone on the server: nothing to free.
        Err(e) if e.is_revoked() => {}
        Err(e) => return Err(e),
    }
    forget();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    fn b64url(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().fold(0u32, |a, b| (a << 8) | *b as u32) << (8 * (3 - chunk.len()));
            for i in 0..chunk.len() + 1 {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize] as char);
            }
        }
        out
    }

    fn keypair() -> (Ed25519KeyPair, String) {
        let rng = ring::rand::SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        let kp = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public = b64url(kp.public_key().as_ref());
        (kp, public)
    }

    fn token(kp: &Ed25519KeyPair, claims: &serde_json::Value) -> String {
        let body = b64url(serde_json::to_string(claims).unwrap().as_bytes());
        format!("{body}.{}", b64url(kp.sign(body.as_bytes()).as_ref()))
    }

    fn claims(mid: &str) -> serde_json::Value {
        serde_json::json!({
            "v": 1, "lid": "L1", "key": "PO-VR7D0-HWEV1-TGZAM-H8C36", "product": "license",
            "email": "a@b.c", "mid": mid, "max": 3, "upd": "2027-11-05", "iat": 1_793_858_126
        })
    }

    #[test]
    fn keys_are_normalised() {
        assert_eq!(normalize_key("PO-VR7D0-HWEV1-TGZAM-H8C36").as_deref(), Some("PO-VR7D0-HWEV1-TGZAM-H8C36"));
        assert_eq!(normalize_key(" po vr7do hwevl tgzam h8c36 ").as_deref(), Some("PO-VR7D0-HWEV1-TGZAM-H8C36"));
        assert_eq!(normalize_key("VR7D0HWEV1TGZAMH8C36").as_deref(), Some("PO-VR7D0-HWEV1-TGZAM-H8C36"));
        assert_eq!(normalize_key("PO-1234"), None);
        assert_eq!(normalize_key("PO-UUUUU-UUUUU-UUUUU-UUUUU"), None);
        assert_eq!(normalize_key("가나다"), None);
    }

    #[test]
    fn base64_both_alphabets() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ").unwrap(), b"hello world");
        assert_eq!(base64_decode("-_8").unwrap(), [0xfb, 0xff]);
        assert_eq!(base64_decode("+/8=").unwrap(), [0xfb, 0xff]);
        assert!(base64_decode("not base64!").is_none());
    }

    #[test]
    fn public_key_raw_or_pem() {
        // The PEM the site's keygen prints, and the same key as raw base64url.
        let pem = "-----BEGIN PUBLIC KEY-----\nMCowBQYDK2VwAyEAv2flYy0nRikNHynbAl8kT7cn0IDbxIUn/ok2fx1fbDo=\n-----END PUBLIC KEY-----\n";
        let from_pem = public_key_bytes(pem).unwrap();
        assert_eq!(public_key_bytes(&b64url(&from_pem)), Some(from_pem));
        assert_eq!(public_key_bytes(""), None);
        assert_eq!(public_key_bytes("AAAA"), None);
    }

    #[test]
    fn tokens_verify_and_forgeries_do_not() {
        let (kp, public) = keypair();
        let t = token(&kp, &claims("m1"));
        let c = verify(&t, &public).expect("valid token");
        assert_eq!(c.mid, "m1");
        assert_eq!(c.upd, "2027-11-05");
        assert_eq!(c.max, 3);

        // Changed payload, old signature.
        let (_, sig) = t.split_once('.').unwrap();
        let forged = format!("{}.{sig}", b64url(serde_json::to_string(&claims("m2")).unwrap().as_bytes()));
        assert!(verify(&forged, &public).is_none());
        // Someone else's key.
        let (other, _) = keypair();
        assert!(verify(&token(&other, &claims("m1")), &public).is_none());
        assert!(verify("garbage", &public).is_none());
    }

    #[test]
    fn license_checks_machine_and_key() {
        let (kp, public) = keypair();
        let cfg = LicenseConfig { server: "http://x".into(), public_key: public };
        let here = machine_id();
        let ok = check(
            &cfg,
            Stored { key: "po-vr7d0-hwev1-tgzam-h8c36".into(), token: token(&kp, &claims(&here)), checked_at: 0 },
        );
        let lic = ok.expect("this computer, this key");
        assert_eq!(lic.masked_key(), "PO-VR7D0-•••••-•••••-H8C36");
        assert!(lic.due_for_check(1_793_858_126 + 8 * 24 * 3600));
        assert!(!lic.due_for_check(1_793_858_126 + 24 * 3600));
        // Copied from another computer.
        assert!(check(
            &cfg,
            Stored { key: "PO-VR7D0-HWEV1-TGZAM-H8C36".into(), token: token(&kp, &claims("elsewhere")), checked_at: 0 }
        )
        .is_err());
        // A token for a different key.
        assert!(check(
            &cfg,
            Stored { key: "PO-00000-00000-00000-00000".into(), token: token(&kp, &claims(&here)), checked_at: 0 }
        )
        .is_err());
    }

    #[test]
    fn only_a_gone_license_counts_as_revoked() {
        let server = |code: &str| LicenseError::Server { code: code.into(), message: String::new() };
        assert!(server("REVOKED").is_revoked());
        assert!(server("INVALID_KEY").is_revoked());
        assert!(!server("DEVICE_LIMIT").is_revoked());
        assert!(!LicenseError::Network("offline".into()).is_revoked());
        assert!(!LicenseError::BadToken.is_revoked());
    }

    #[test]
    fn config_needs_a_server_and_a_key() {
        // The shipped file is empty until the sales site is set up: licensing stays off.
        let c: LicenseConfig = serde_json::from_str(CONFIG_JSON).unwrap();
        assert!(c.server.is_empty() || public_key_bytes(&c.public_key).is_some());
    }

    #[test]
    fn machine_id_is_stable_and_anonymous() {
        let a = machine_id();
        assert_eq!(a, machine_id());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        if let Ok(raw) = machine_uid::get() {
            assert!(!a.contains(raw.trim()));
        }
    }
}
