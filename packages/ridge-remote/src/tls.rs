//! TLS material for the remote-control HTTPS server.
//!
//! Browsers only expose WebGPU (`navigator.gpu`) in a **secure context**
//! (https / wss / localhost / file). A phone or desktop browser reaching the
//! remote UI over `http://<lan-ip>:9527` is *not* a secure context, so the
//! terminal renderer silently falls back to Canvas2D. Serving the same page
//! over TLS turns the LAN origin into a secure context and unlocks the
//! WebGPU render path.
//!
//! There is no public CA that will issue a cert for a private LAN IP, so we
//! run our own. On first run we generate a long-lived **local root CA** and
//! a short-lived **leaf** cert (signed by that CA) with the LAN IP + hostname
//! baked in as SANs. The server presents the leaf; the CA is offered for
//! download on the verification page (`/ridge-ca.crt`). A device that trusts
//! the CA **once** stops warning for every present and future leaf — even
//! after the LAN IP changes and the leaf rotates — because the trust anchor
//! is the CA, not the leaf. This is the "mkcert" model.
//!
//! Apple platforms refuse TLS server (leaf) certs whose validity exceeds
//! ~398 days, so the leaf is issued for [`LEAF_VALID_DAYS`] and proactively
//! rotated once it passes [`LEAF_RENEW_DAYS`]. The CA, being a manually
//! trusted anchor rather than a server cert, is long-lived ([`CA_VALID_DAYS`]).
//!
//! Material under `%LOCALAPPDATA%\ridge\remote-tls\`:
//! - `ca-key.pem`  — root CA private key (generated once, reused forever).
//! - `ca.pem` / `ca.der` — root CA cert (stable; what the user downloads/trusts).
//! - `cert.pem` / `key.pem` — current server leaf cert + key.
//! - `meta.txt`   — `lan_ip\nhostname\n<created_unix>`; tracks what the leaf
//!   SANs were built for and when, driving rotation.
//!
//! User-provided cert escape hatch (unchanged): if `cert.pem` + `key.pem`
//! exist with **no** `meta.txt` and **no** `ca-key.pem`, they are treated as a
//! user-supplied cert (mkcert / corporate CA) and reused verbatim, never
//! overwritten.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use axum_server::tls_rustls::RustlsConfig;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType, SerialNumber,
};
use time::{Duration as TimeDuration, OffsetDateTime};

/// Root CA validity — long, because it is a manually trusted anchor, not a
/// server cert subject to the ~398-day platform ceiling.
const CA_VALID_DAYS: i64 = 3650;
/// Leaf validity — kept under Apple's ~398-day TLS server-cert limit.
const LEAF_VALID_DAYS: i64 = 397;
/// Rotate the leaf once it is older than this (well before expiry).
const LEAF_RENEW_DAYS: u64 = 350;

/// Directory holding the remote server's TLS material.
pub fn tls_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
        .join("remote-tls")
}

/// Root CA certificate in DER form, for the `/ridge-ca.crt` download endpoint
/// (iOS / Android / Windows install the CA as a trust anchor in this form).
///
/// Returns `None` before the server has generated a CA, or when the install is
/// running a user-provided cert with no Ridge CA.
pub fn ca_cert_der() -> Option<Vec<u8>> {
    let dir = tls_dir();
    if let Ok(der) = std::fs::read(dir.join("ca.der")) {
        if !der.is_empty() {
            return Some(der);
        }
    }
    // Fallback for installs predating ca.der: decode the PEM body.
    let pem = std::fs::read_to_string(dir.join("ca.pem")).ok()?;
    pem_to_der(&pem)
}

/// Root CA certificate in PEM form, for desktop trust-store import.
pub fn ca_cert_pem() -> Option<String> {
    let pem = std::fs::read_to_string(tls_dir().join("ca.pem")).ok()?;
    if pem.trim().is_empty() {
        return None;
    }
    Some(pem)
}

/// Resolve a `RustlsConfig` for the remote server, ensuring a local CA exists
/// and minting a fresh CA-signed leaf for `lan_ip` / `hostname` whenever the
/// cached leaf is missing, stale (LAN IP / hostname changed), or aged out.
///
/// Returns `None` (caller falls back to plain HTTP) if cert material can't be
/// produced or parsed — TLS is best-effort, the server must still come up.
pub async fn resolve_config(lan_ip: &str, hostname: &str) -> Option<RustlsConfig> {
    // rustls 0.23 has no compiled-in crypto provider under
    // `tls-rustls-no-provider`; install ring once for the whole process.
    // `install_default` errors only if one is already set — harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let dir = tls_dir();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let meta_path = dir.join("meta.txt");
    let ca_key_path = dir.join("ca-key.pem");

    // User-provided cert: cert+key present, but no meta and no Ridge CA → treat
    // as a user-supplied cert (mkcert / corporate). Reuse verbatim.
    let have_leaf = cert_path.exists() && key_path.exists();
    if have_leaf && !meta_path.exists() && !ca_key_path.exists() {
        if let (Ok(c), Ok(k)) = (std::fs::read(&cert_path), std::fs::read(&key_path)) {
            return build_config(c, k).await;
        }
    }

    // Ridge-managed CA + leaf.
    if let Some((cert_pem, key_pem)) = ensure_ca_and_leaf(&dir, lan_ip, hostname) {
        return build_config(cert_pem.into_bytes(), key_pem.into_bytes()).await;
    }

    // Last-ditch fallback: serve whatever cert/key pair is on disk.
    match (std::fs::read(&cert_path), std::fs::read(&key_path)) {
        (Ok(c), Ok(k)) => build_config(c, k).await,
        _ => None,
    }
}

async fn build_config(cert_pem: Vec<u8>, key_pem: Vec<u8>) -> Option<RustlsConfig> {
    match RustlsConfig::from_pem(cert_pem, key_pem).await {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote TLS: failed to load cert/key");
            None
        }
    }
}

/// Ensure a local CA exists, then return a (leaf cert PEM, leaf key PEM) pair —
/// reusing the cached leaf when still valid for the current address, otherwise
/// minting and persisting a fresh CA-signed leaf. Returns `None` on any
/// unrecoverable generation error.
fn ensure_ca_and_leaf(dir: &Path, lan_ip: &str, hostname: &str) -> Option<(String, String)> {
    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::error!(target: "ridge::remote", error = %e, "remote TLS: mkdir failed");
        // Persistence will fail, but we can still mint an in-memory leaf below.
    }

    let (ca_cert, ca_key) = load_or_create_ca(dir)?;

    // Reuse the cached leaf when its SANs still match and it is not aged out.
    if leaf_should_reuse(dir, lan_ip, hostname) {
        if let (Ok(cert), Ok(key)) = (
            std::fs::read_to_string(dir.join("cert.pem")),
            std::fs::read_to_string(dir.join("key.pem")),
        ) {
            return Some((cert, key));
        }
    }

    let (leaf_pem, leaf_key_pem) = match generate_leaf(lan_ip, hostname, &ca_cert, &ca_key) {
        Some(pair) => pair,
        None => {
            tracing::error!(target: "ridge::remote", "remote TLS: leaf generation failed");
            return None;
        }
    };

    let _ = std::fs::write(dir.join("cert.pem"), &leaf_pem);
    let _ = std::fs::write(dir.join("key.pem"), &leaf_key_pem);
    let _ = std::fs::write(dir.join("meta.txt"), leaf_meta(lan_ip, hostname));
    tracing::info!(target: "ridge::remote", lan_ip, hostname, "remote TLS: issued CA-signed leaf cert");

    Some((leaf_pem, leaf_key_pem))
}

/// Load the persisted root CA, or create and persist one on first run.
///
/// The CA private key is the durable trust root: it is generated once and
/// reused forever. The CA *certificate* is regenerated in memory each call
/// from that key with a fixed distinguished name, so it can act as the
/// `signed_by` issuer; the downloadable `ca.pem` / `ca.der` are written only
/// once (on first creation) and stay byte-stable for the device's trust store.
fn load_or_create_ca(dir: &Path) -> Option<(Certificate, KeyPair)> {
    let ca_key_path = dir.join("ca-key.pem");

    let ca_key = if ca_key_path.exists() {
        match std::fs::read_to_string(&ca_key_path).ok().and_then(|p| KeyPair::from_pem(&p).ok()) {
            Some(kp) => kp,
            None => {
                tracing::error!(target: "ridge::remote", "remote TLS: CA key unreadable; regenerating");
                let kp = KeyPair::generate().ok()?;
                let _ = std::fs::write(&ca_key_path, kp.serialize_pem());
                // Force a fresh anchor so the new key's cert is the one served.
                let _ = std::fs::remove_file(dir.join("ca.pem"));
                let _ = std::fs::remove_file(dir.join("ca.der"));
                kp
            }
        }
    } else {
        let kp = KeyPair::generate().ok()?;
        let _ = std::fs::write(&ca_key_path, kp.serialize_pem());
        kp
    };

    let ca_cert = ca_params()?.self_signed(&ca_key).ok()?;

    // Persist the downloadable anchor exactly once so it stays byte-stable.
    let ca_pem_path = dir.join("ca.pem");
    if !ca_pem_path.exists() {
        let _ = std::fs::write(&ca_pem_path, ca_cert.pem());
    }
    let ca_der_path = dir.join("ca.der");
    if !ca_der_path.exists() {
        let _ = std::fs::write(&ca_der_path, ca_cert.der().as_ref());
    }

    Some((ca_cert, ca_key))
}

/// Parameters for the local root CA.
fn ca_params() -> Option<CertificateParams> {
    let mut params = CertificateParams::new(Vec::new()).ok()?;
    params
        .distinguished_name
        .push(DnType::CommonName, "Ridge Remote Local CA");
    params
        .distinguished_name
        .push(DnType::OrganizationName, "Ridge");
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - TimeDuration::days(1);
    params.not_after = now + TimeDuration::days(CA_VALID_DAYS);
    params.serial_number = Some(SerialNumber::from(1u64));
    Some(params)
}

/// Mint a leaf cert signed by the local CA.
fn generate_leaf(
    lan_ip: &str,
    hostname: &str,
    ca_cert: &Certificate,
    ca_key: &KeyPair,
) -> Option<(String, String)> {
    let mut dns: Vec<String> = vec!["localhost".to_string(), "ridge-local.local".to_string()];
    if !hostname.is_empty() && hostname != "localhost" {
        dns.push(hostname.to_string());
    }
    let mut params = CertificateParams::new(dns).ok()?;
    params
        .distinguished_name
        .push(DnType::CommonName, "Ridge Remote Control");
    params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    if let Ok(ip) = lan_ip.parse::<IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }
    params.is_ca = IsCa::NoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    let now = OffsetDateTime::now_utc();
    params.not_before = now - TimeDuration::days(1);
    params.not_after = now + TimeDuration::days(LEAF_VALID_DAYS);

    let leaf_key = KeyPair::generate().ok()?;
    let leaf_cert = params.signed_by(&leaf_key, ca_cert, ca_key).ok()?;
    Some((leaf_cert.pem(), leaf_key.serialize_pem()))
}

fn leaf_meta(lan_ip: &str, hostname: &str) -> String {
    format!("{lan_ip}\n{hostname}\n{}", now_unix())
}

fn leaf_should_reuse(dir: &Path, lan_ip: &str, hostname: &str) -> bool {
    let Ok(meta) = std::fs::read_to_string(dir.join("meta.txt")) else {
        return false;
    };
    let lines: Vec<&str> = meta.lines().collect();
    if lines.len() < 3 || lines[0] != lan_ip || lines[1] != hostname {
        return false;
    }
    let created: u64 = lines[2].trim().parse().unwrap_or(0);
    let now = now_unix();
    if created == 0 || now < created {
        return false;
    }
    let age_days = (now - created) / 86_400;
    age_days < LEAF_RENEW_DAYS && dir.join("cert.pem").exists() && dir.join("key.pem").exists()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn pem_to_der(pem: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let body: String = pem
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END"))
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .ok()
}
