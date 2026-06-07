//! 设备码流（契约 §4.4，Device Code Flow，给无头 ridge-cli 用）。
//!
//! 1. `POST /device/code` 取配对码 `{pairing_code, poll_token, expires_in}`。
//! 2. 控制台用极客 ANSI 风格打印配对码，引导用户访问 `https://{base}/activate` 输入。
//! 3. 异步长轮询 `POST /device/poll {poll_token}` 直到 `status="bound"`。
//! 4. 拿到 device JWT 持久化到 `~/.config/ridge/auth.json`。

use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::config::{self, AuthFile};
use crate::envelope::parse_envelope;

/// 轮询间隔（契约建议 2s）。
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// `POST /device/code` 的 data。
#[derive(Debug, Clone, Deserialize)]
struct DeviceCodeResp {
    pairing_code: String,
    poll_token: String,
    expires_in: u64,
}

/// `POST /device/poll` 的 data（按 `status` 分支）。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum PollResp {
    Pending,
    Expired,
    Bound {
        token: String,
        device_name: String,
        username: String,
    },
}

/// 跑完整设备码流并返回（已持久化的）凭据。
pub async fn run_enable(client: &reqwest::Client) -> Result<AuthFile> {
    let api = config::api_base();

    // 1. 取配对码。
    let body = client
        .post(format!("{api}/device/code"))
        .json(&json!({}))
        .send()
        .await
        .context("POST /device/code request failed")?
        .text()
        .await
        .context("reading /device/code body failed")?;
    let code: DeviceCodeResp =
        parse_envelope(&body).context("parsing /device/code response failed")?;

    // 2. 极客风提示。
    print_pairing_banner(&code.pairing_code, &config::activate_url(), code.expires_in);

    // 3. 长轮询直至 bound / expired / 超时。
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    loop {
        if Instant::now() >= deadline {
            bail!(
                "pairing timed out after {}s — re-run `ridge-cli remote --enable`",
                code.expires_in
            );
        }

        let resp_body = client
            .post(format!("{api}/device/poll"))
            .json(&json!({ "poll_token": code.poll_token }))
            .send()
            .await
            .context("POST /device/poll request failed")?
            .text()
            .await
            .context("reading /device/poll body failed")?;

        let poll: PollResp =
            parse_envelope(&resp_body).context("parsing /device/poll response failed")?;

        match poll {
            PollResp::Pending => {
                tokio::time::sleep(POLL_INTERVAL).await;
            }
            PollResp::Expired => {
                bail!("pairing code expired — re-run `ridge-cli remote --enable`");
            }
            PollResp::Bound {
                token,
                device_name,
                username,
            } => {
                let auth = AuthFile {
                    token,
                    device_name,
                    username,
                };
                config::save_auth(&auth).context("failed to persist device credentials")?;
                print_bound_banner(&auth);
                return Ok(auth);
            }
        }
    }
}

// ── ANSI 极客风控制台输出 ──────────────────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";

fn print_pairing_banner(pairing_code: &str, activate_url: &str, expires_in: u64) {
    let minutes = expires_in / 60;
    eprintln!();
    eprintln!("{CYAN}{BOLD}  ╔══════════════════════════════════════════════╗{RESET}");
    eprintln!("{CYAN}{BOLD}  ║          RIDGE · DEVICE PAIRING               ║{RESET}");
    eprintln!("{CYAN}{BOLD}  ╚══════════════════════════════════════════════╝{RESET}");
    eprintln!();
    eprintln!("  {DIM}1.{RESET} 在已登录的浏览器打开:  {GREEN}{BOLD}{activate_url}{RESET}");
    eprintln!("  {DIM}2.{RESET} 输入下面的配对码 (≈{minutes} 分钟内有效):");
    eprintln!();
    eprintln!("        {YELLOW}{BOLD}▎ {pairing_code} ▎{RESET}");
    eprintln!();
    eprintln!("  {DIM}等待绑定中… (Ctrl-C 取消){RESET}");
    eprintln!();
}

fn print_bound_banner(auth: &AuthFile) {
    eprintln!();
    eprintln!("  {GREEN}{BOLD}✓ 设备已绑定{RESET}");
    eprintln!("    device   : {BOLD}{}{RESET}", auth.device_name);
    eprintln!("    username : {BOLD}{}{RESET}", auth.username);
    eprintln!("    公网入口 : {CYAN}{}{RESET}", auth.public_entry());
    eprintln!("    凭据已写入: {DIM}~/.config/ridge/auth.json{RESET}");
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_resp_deserializes_each_status() {
        let pending: PollResp = serde_json::from_str(r#"{"status":"pending"}"#).unwrap();
        assert!(matches!(pending, PollResp::Pending));

        let expired: PollResp = serde_json::from_str(r#"{"status":"expired"}"#).unwrap();
        assert!(matches!(expired, PollResp::Expired));

        let bound: PollResp = serde_json::from_str(
            r#"{"status":"bound","token":"JWT","device_name":"vps","username":"bob"}"#,
        )
        .unwrap();
        match bound {
            PollResp::Bound {
                token,
                device_name,
                username,
            } => {
                assert_eq!(token, "JWT");
                assert_eq!(device_name, "vps");
                assert_eq!(username, "bob");
            }
            _ => panic!("expected Bound"),
        }
    }

    #[test]
    fn device_code_envelope_parses() {
        let body = r#"{"ok":true,"data":{"pairing_code":"XA4B-97RE","poll_token":"opaque","expires_in":600}}"#;
        let code: DeviceCodeResp = parse_envelope(body).unwrap();
        assert_eq!(code.pairing_code, "XA4B-97RE");
        assert_eq!(code.poll_token, "opaque");
        assert_eq!(code.expires_in, 600);
    }

    #[test]
    fn error_envelope_is_surfaced() {
        let body = r#"{"ok":false,"error":{"code":"RATE_LIMITED","message":"slow down"}}"#;
        let res: Result<DeviceCodeResp> = parse_envelope(body);
        let err = res.unwrap_err().to_string();
        assert!(err.contains("RATE_LIMITED"), "got: {err}");
    }
}
