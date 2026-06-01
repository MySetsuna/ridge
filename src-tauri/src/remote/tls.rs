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
//! auto-generate a long-lived **self-signed** cert on first run (rcgen, ring
//! backend) with the LAN IP + hostname baked in as SANs. Browsers show a
//! one-time "not private" warning per device (encrypted, just untrusted);
//! after the user proceeds once, the exception is remembered.
//!
//! Cert lifecycle (under `%LOCALAPPDATA%\ridge\remote-tls\`):
//! - `cert.pem` + `key.pem` + `meta.txt` present, `meta.txt` matches the
//!   current `lan_ip\nhostname` → reuse (our auto cert, still valid).
//! - `meta.txt` *missing* but cert+key present → treat as a **user-provided**
//!   cert (e.g. mkcert / corporate CA); reuse verbatim, never overwrite.
//! - `meta.txt` present but stale (LAN IP changed) or files missing →
//!   regenerate so the SAN keeps matching the address clients connect to.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use axum_server::tls_rustls::RustlsConfig;

/// Directory holding the remote server's TLS material.
fn tls_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge")
        .join("remote-tls")
}

/// Resolve a `RustlsConfig` for the remote server, generating a self-signed
/// cert for `lan_ip` / `hostname` if one isn't already cached (or if the
/// cached auto cert no longer matches the current LAN IP).
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
    let meta_now = format!("{lan_ip}\n{hostname}");

    let have_pair = cert_path.exists() && key_path.exists();
    let user_provided = have_pair && !meta_path.exists();
    let auto_fresh = have_pair
        && std::fs::read_to_string(&meta_path)
            .map(|m| m.trim() == meta_now.trim())
            .unwrap_or(false);

    let (cert_pem, key_pem) = if user_provided || auto_fresh {
        match (std::fs::read(&cert_path), std::fs::read(&key_path)) {
            (Ok(c), Ok(k)) => (c, k),
            _ => generate_and_persist(&dir, lan_ip, hostname, &meta_now)?,
        }
    } else {
        generate_and_persist(&dir, lan_ip, hostname, &meta_now)?
    };

    match RustlsConfig::from_pem(cert_pem, key_pem).await {
        Ok(cfg) => Some(cfg),
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote TLS: failed to load cert/key");
            None
        }
    }
}

/// Generate a fresh self-signed cert+key, persist them (plus the SAN meta) to
/// `dir`, and return the PEM bytes. Returns `None` on any generation/IO error.
fn generate_and_persist(
    dir: &std::path::Path,
    lan_ip: &str,
    hostname: &str,
    meta_now: &str,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let (cert_pem, key_pem) = match generate_self_signed(lan_ip, hostname) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!(target: "ridge::remote", error = %e, "remote TLS: cert generation failed");
            return None;
        }
    };

    if let Err(e) = std::fs::create_dir_all(dir) {
        tracing::error!(target: "ridge::remote", error = %e, "remote TLS: mkdir failed");
        return None;
    }
    // Best-effort persistence: if writing fails we still return the in-memory
    // PEM so the server can come up over TLS this session.
    let _ = std::fs::write(dir.join("cert.pem"), &cert_pem);
    let _ = std::fs::write(dir.join("key.pem"), &key_pem);
    let _ = std::fs::write(dir.join("meta.txt"), meta_now);
    tracing::info!(target: "ridge::remote", lan_ip, hostname, "remote TLS: generated self-signed cert");

    Some((cert_pem.into_bytes(), key_pem.into_bytes()))
}

/// Build a self-signed cert valid for the LAN IP, loopback, localhost, the
/// machine hostname, and the mDNS `ridge-local.local` target.
fn generate_self_signed(lan_ip: &str, hostname: &str) -> Result<(String, String), rcgen::Error> {
    use rcgen::{CertificateParams, DnType, KeyPair, SanType};

    // DNS-style SANs go through `new`; IP SANs are pushed explicitly so they
    // land as `SanType::IpAddress` rather than being misread as DNS names.
    let mut dns: Vec<String> = vec!["localhost".to_string(), "ridge-local.local".to_string()];
    if !hostname.is_empty() && hostname != "localhost" {
        dns.push(hostname.to_string());
    }
    let mut params = CertificateParams::new(dns)?;
    params
        .distinguished_name
        .push(DnType::CommonName, "Ridge Remote Control");

    params
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    if let Ok(ip) = lan_ip.parse::<IpAddr>() {
        params.subject_alt_names.push(SanType::IpAddress(ip));
    }

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;
    Ok((cert.pem(), key_pair.serialize_pem()))
}
