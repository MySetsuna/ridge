//! 账号密码登录 + 自助激活（替代设备码浏览器回环）。
//!
//! `rdg login` 引导流程：
//!   1. 读邮箱 + 密码（密码不回显），`POST /auth/login` 取 **user JWT**。
//!   2. 若账号尚无用户名（试用用户也可设）→ 提示设置 → `POST /auth/set-username`。
//!   3. 读设备名 → `POST /device/bind`（Bearer user）→ 直接下发 **device JWT** + 公网入口。
//!   4. 把 device JWT 持久化到 `~/.config/ridge/auth.json`（与设备码流同一 [`AuthFile`]）。
//!
//! 设计取向：与 [`crate::device_flow`] 同款极客 ANSI 控制台体验，纯 stdin 引导（无需
//! 浏览器）。接入与激活解耦：免费/试用账号也能登录、设用户名、绑定设备使主机上线；
//! 建立控制通道仍由云端按订阅门控（见 ridge-cloud ws/handler.rs）。

use std::io::{self, Write};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::json;

use crate::config::{self, AuthFile};
use crate::envelope::parse_envelope;

/// 单步交互最大重试次数（登录 / 设用户名 / 绑定设备）。
const MAX_ATTEMPTS: u32 = 3;

// ── 响应 DTO（仅取所需字段）─────────────────────────────────────────────

/// `/auth/login` 与 `/auth/set-username` 返回 `{token, user}`（token 为 user JWT）。
#[derive(Debug, Deserialize)]
struct TokenUser {
    token: String,
    user: UserBrief,
}

#[derive(Debug, Deserialize)]
struct UserBrief {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: String,
    #[serde(rename = "premiumActive", default)]
    premium_active: bool,
    #[serde(rename = "isTrial", default)]
    is_trial: bool,
}

/// `/device/bind` 返回 `{token, device_name, username, public_entry}`（token 为 device JWT）。
#[derive(Debug, Deserialize)]
struct DeviceBind {
    token: String,
    device_name: String,
    username: String,
    public_entry: String,
}

/// 跑完整「登录 → 设用户名 → 绑定设备」流程，返回已持久化的设备凭据。
pub async fn run_login(client: &reqwest::Client) -> Result<AuthFile> {
    let api = config::api_base();
    print_login_banner();

    // 1. 账号密码登录（最多 MAX_ATTEMPTS 次）。
    let mut session = login_loop(client, &api).await?;

    // 2. 设置用户名（若未设；试用用户也可设）。
    if session.user.username.as_deref().unwrap_or("").is_empty() {
        session = set_username_loop(client, &api, &session.token).await?;
    }
    let username = session.user.username.clone().unwrap_or_default();
    print_logged_in(&session.user, &username);

    // 3. 绑定设备 → device JWT。
    let bound = bind_device_loop(client, &api, &session.token).await?;

    // 4. 持久化（与设备码流同一文件 / 形状）。
    let auth = AuthFile {
        token: bound.token,
        device_name: bound.device_name,
        username: bound.username,
    };
    config::save_auth(&auth).context("failed to persist device credentials")?;
    print_bound_banner(&auth, &bound.public_entry, session.user.premium_active);
    Ok(auth)
}

// ── 分步循环 ──────────────────────────────────────────────────────────

async fn login_loop(client: &reqwest::Client, api: &str) -> Result<TokenUser> {
    for attempt in 1..=MAX_ATTEMPTS {
        let email = prompt_line("  邮箱: ")?;
        let password = prompt_password("  密码: ")?;
        if email.trim().is_empty() || password.is_empty() {
            print_err("邮箱和密码不能为空");
            continue;
        }

        let body = client
            .post(format!("{api}/auth/login"))
            .json(&json!({ "email": email.trim(), "password": password }))
            .send()
            .await
            .context("POST /auth/login request failed")?
            .text()
            .await
            .context("reading /auth/login body failed")?;

        match parse_envelope::<TokenUser>(&body) {
            Ok(session) => return Ok(session),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("EMAIL_NOT_VERIFIED") {
                    print_err("邮箱尚未验证：请先在浏览器完成邮箱验证后再登录。");
                    bail!("email not verified");
                }
                // 凭据错误等：剩余次数则重试。
                if attempt < MAX_ATTEMPTS {
                    print_err("邮箱或密码错误，请重试。");
                } else {
                    bail!("login failed after {MAX_ATTEMPTS} attempts: {msg}");
                }
            }
        }
    }
    unreachable!("login_loop exits via return/bail")
}

async fn set_username_loop(
    client: &reqwest::Client,
    api: &str,
    user_token: &str,
) -> Result<TokenUser> {
    eprintln!();
    eprintln!("  {DIM}该账号尚未设置用户名（公网子域前缀，设定后不可更改）。{RESET}");
    for attempt in 1..=MAX_ATTEMPTS {
        let username = prompt_line("  用户名 (小写字母+数字, 3–20 位): ")?;
        if username.trim().is_empty() {
            print_err("用户名不能为空");
            continue;
        }

        let body = client
            .post(format!("{api}/auth/set-username"))
            .bearer_auth(user_token)
            .json(&json!({ "username": username.trim() }))
            .send()
            .await
            .context("POST /auth/set-username request failed")?
            .text()
            .await
            .context("reading /auth/set-username body failed")?;

        match parse_envelope::<TokenUser>(&body) {
            Ok(session) => return Ok(session),
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    print_err(&friendly(&e.to_string()));
                } else {
                    bail!("set username failed: {e}");
                }
            }
        }
    }
    unreachable!("set_username_loop exits via return/bail")
}

async fn bind_device_loop(
    client: &reqwest::Client,
    api: &str,
    user_token: &str,
) -> Result<DeviceBind> {
    eprintln!();
    eprintln!("  {DIM}为本机起一个设备名，生成专属公网入口。{RESET}");
    for attempt in 1..=MAX_ATTEMPTS {
        let device = prompt_line("  设备名 (小写字母+数字+连字符, 3–30 位): ")?;
        if device.trim().is_empty() {
            print_err("设备名不能为空");
            continue;
        }

        let body = client
            .post(format!("{api}/device/bind"))
            .bearer_auth(user_token)
            .json(&json!({ "device_name": device.trim() }))
            .send()
            .await
            .context("POST /device/bind request failed")?
            .text()
            .await
            .context("reading /device/bind body failed")?;

        match parse_envelope::<DeviceBind>(&body) {
            Ok(bound) => return Ok(bound),
            Err(e) => {
                if attempt < MAX_ATTEMPTS {
                    print_err(&friendly(&e.to_string()));
                } else {
                    bail!("device bind failed: {e}");
                }
            }
        }
    }
    unreachable!("bind_device_loop exits via return/bail")
}

// ── 输入助手 ──────────────────────────────────────────────────────────

/// 打印提示并读一行（去尾换行）。提示写 stdout 并 flush，保证可见。
fn prompt_line(label: &str) -> Result<String> {
    let mut stdout = io::stdout();
    write!(stdout, "{label}").context("write prompt failed")?;
    stdout.flush().ok();
    let mut line = String::new();
    let n = io::stdin()
        .read_line(&mut line)
        .context("read stdin failed")?;
    if n == 0 {
        bail!("input stream closed (EOF)");
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// 读密码（不回显）。非 TTY（管道）场景回退为普通读行。
fn prompt_password(label: &str) -> Result<String> {
    match rpassword::prompt_password(label) {
        Ok(p) => Ok(p),
        // 非交互/无 TTY：退回明文读行（CI / 管道）。
        Err(_) => prompt_line(label),
    }
}

/// 把后端错误码翻成对用户友好的中文提示（仅常见几种，其余原样回显）。
fn friendly(err: &str) -> String {
    if err.contains("USERNAME_TAKEN") {
        "该用户名已被占用，换一个试试。".into()
    } else if err.contains("DEVICE_NAME_TAKEN") {
        "该设备名已存在，换一个名字。".into()
    } else if err.contains("DEVICE_LIMIT_REACHED") {
        "设备数量已达上限，请到控制台删除旧设备后再试。".into()
    } else if err.contains("INVALID_INPUT") {
        "格式不合法，请按提示重新输入。".into()
    } else if err.contains("USERNAME_REQUIRED") {
        "请先设置用户名。".into()
    } else {
        err.to_string()
    }
}

// ── ANSI 极客风输出（与 device_flow 同款）────────────────────────────────

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";

fn print_login_banner() {
    eprintln!();
    eprintln!("{CYAN}{BOLD}  ╔══════════════════════════════════════════════╗{RESET}");
    eprintln!("{CYAN}{BOLD}  ║          RIDGE · ACCOUNT LOGIN                ║{RESET}");
    eprintln!("{CYAN}{BOLD}  ╚══════════════════════════════════════════════╝{RESET}");
    eprintln!();
    eprintln!("  {DIM}用 Ridge 账号邮箱 + 密码登录，随后直接绑定本机为远控设备。{RESET}");
    eprintln!();
}

fn print_logged_in(user: &UserBrief, username: &str) {
    let plan = if user.premium_active {
        if user.is_trial {
            "TRIAL"
        } else {
            "PREMIUM"
        }
    } else {
        "FREE"
    };
    eprintln!();
    eprintln!("  {GREEN}{BOLD}✓ 已登录{RESET}  {DIM}{}{RESET}", user.email);
    eprintln!("    username : {BOLD}{username}{RESET}   plan: {BOLD}{plan}{RESET}");
}

fn print_bound_banner(auth: &AuthFile, public_entry: &str, premium_active: bool) {
    eprintln!();
    eprintln!("  {GREEN}{BOLD}✓ 设备已激活{RESET}");
    eprintln!("    device   : {BOLD}{}{RESET}", auth.device_name);
    eprintln!("    公网入口 : {CYAN}{public_entry}{RESET}");
    eprintln!("    凭据已写入: {DIM}~/.config/ridge/auth.json{RESET}");
    eprintln!();
    if !premium_active {
        eprintln!(
            "  {DIM}提示：当前为免费/试用，主机可上线展示为「接入」，建立控制通道需订阅。{RESET}"
        );
    }
    eprintln!("  下一步：运行 {BOLD}rdg remote --daemon{RESET} 开始守护并等待控制端接入。");
    eprintln!();
}

fn print_err(msg: &str) {
    eprintln!("  {RED}✗ {msg}{RESET}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::parse_envelope;

    #[test]
    fn token_user_parses_username_and_plan_fields() {
        let body = r#"{"ok":true,"data":{"token":"JWT","user":{
            "email":"a@b.com","username":"alice","premiumActive":true,"isTrial":true}}}"#;
        let session: TokenUser = parse_envelope(body).unwrap();
        assert_eq!(session.token, "JWT");
        assert_eq!(session.user.email, "a@b.com");
        assert_eq!(session.user.username.as_deref(), Some("alice"));
        assert!(session.user.premium_active);
        assert!(session.user.is_trial);
    }

    #[test]
    fn token_user_tolerates_missing_optional_user_fields() {
        // username 缺省 → None；premiumActive / isTrial 缺省 → false。
        let body = r#"{"ok":true,"data":{"token":"JWT","user":{"email":"x@y.com"}}}"#;
        let session: TokenUser = parse_envelope(body).unwrap();
        assert!(session.user.username.is_none());
        assert!(!session.user.premium_active);
        assert!(!session.user.is_trial);
    }

    #[test]
    fn device_bind_parses_all_fields() {
        let body = r#"{"ok":true,"data":{"token":"DEVJWT","device_name":"vps",
            "username":"alice","public_entry":"https://vps-alice.example"}}"#;
        let bound: DeviceBind = parse_envelope(body).unwrap();
        assert_eq!(bound.token, "DEVJWT");
        assert_eq!(bound.device_name, "vps");
        assert_eq!(bound.username, "alice");
        assert_eq!(bound.public_entry, "https://vps-alice.example");
    }

    #[test]
    fn friendly_maps_known_error_codes_to_chinese() {
        assert!(friendly("API error [USERNAME_TAKEN]: ...").contains("用户名已被占用"));
        assert!(friendly("API error [DEVICE_NAME_TAKEN]: ...").contains("设备名已存在"));
        assert!(friendly("API error [DEVICE_LIMIT_REACHED]: ...").contains("上限"));
        assert!(friendly("API error [INVALID_INPUT]: ...").contains("格式不合法"));
    }

    #[test]
    fn friendly_passes_through_unknown_errors() {
        let raw = "API error [SOMETHING_ELSE]: boom";
        assert_eq!(friendly(raw), raw);
    }
}
