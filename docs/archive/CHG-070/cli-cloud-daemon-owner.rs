pub async fn run(shell: Option<String>, cwd: Option<String>, root: Option<String>) -> Result<()> {
    let _status_guard = StatusGuard;
    publish_status("starting", "正在读取设备凭据");
    let auth = match config::load_auth()
        .context("failed to load credentials")?
        .context("本机尚未激活云端设备：先跑 `ridge login`（或 `ridge login --browser`）绑定本机，再启动公网远控")
    {
        Ok(auth) => auth,
        Err(error) => {
            publish_status("error", &format!("{error:#}"));
            return Err(error);
        }
    };

    // WebRTC is a detached shell adapter over the long-lived kernel.  Ensure
    // the kernel exists before a controller can create its first PTY; this
    // prevents a startup race from falling back to a daemon-local PTY.
    if let Err(error) = crate::kernel_ctl::ensure_kernel_running() {
        tracing::warn!(target: "ridge_cli::daemon", %error, "kernel bootstrap unavailable; session may retry");
    }

    tracing::info!(
        target: "ridge_cli::daemon",
        device = %auth.device_name,
        username = %auth.username,
        entry = %auth.public_entry(),
        "starting ridge-cli daemon"
    );

    // Runtime holds the device key; the daemon retains only public identity and a signer client.
    let endpoint = crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
    let mut device_identity = DeviceBindingClient::connect(endpoint).map_err(anyhow::Error::msg)?;
    tracing::info!(
        target: "ridge_cli::daemon",
        fingerprint = %device_identity.fingerprint(),
        "device identity ready (Ed25519, zero-trust #2)"
    );

    let http = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;

    let mut backoff = MIN_BACKOFF;
    loop {
        let endpoint = crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
        let refreshed = DeviceBindingClient::connect(endpoint).map_err(anyhow::Error::msg)?;
        if refreshed.public_bytes() != device_identity.public_bytes() {
            anyhow::bail!("Runtime device identity changed; reconnect requires explicit trust review");
        }
        device_identity = refreshed;
        publish_status("connecting", "正在连接信令中继");
        match serve_once(
            &http,
            &auth,
            &device_identity,
            shell.clone(),
            cwd.clone(),
            root.clone(),
        )
        .await
        {
            Ok(()) => {
                // 信令正常断开：重置退避后立即重连。
                backoff = MIN_BACKOFF;
            }
            Err(e) => {
                if e.downcast_ref::<FatalSignalError>().is_some() {
                    return Err(e);
                }
                publish_status(
                    "connecting",
                    &format!("连接失败，{} 秒后重试：{e:#}", backoff.as_secs()),
                );
                tracing::warn!(
                    target: "ridge_cli::daemon",
                    error = %e,
                    backoff_secs = backoff.as_secs(),
                    "session loop error; reconnecting"
                );
                // 可见性（iter-62）：连不上中继时旧实现只 warn 进日志再静默退避，前台
                // `ridge remote` 一片空白，用户只能从控制端的「远程主机当前不在线」反推。
                // 把失败原因与下次重试间隔直接打到 stderr（TUI 模式下 stderr 已重定向到
                // 日志文件，不会糊屏）。
                eprintln!(
                    "! 连接信令中继失败（{}s 后重试）：{e:#}\n  入口：{}",
                    backoff.as_secs(),
                    auth.public_entry()
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// 连一次信令，处理其上的所有会话直到信令断开。
async fn serve_once(
    http: &reqwest::Client,
    auth: &AuthFile,
    device_identity: &dyn DeviceBindingSigner,
    shell: Option<String>,
    cwd: Option<String>,
    root: Option<String>,
) -> Result<()> {
    let ice_servers = ice::fetch_ice_servers(http, &auth.token).await;
    let mut signaling = Signaling::connect(&auth.signaling_ws_url())
        .await
        .context("signaling connect failed")?;
    let sender = signaling.sender();
    let peer = WebRtcHost;

    tracing::info!(target: "ridge_cli::daemon", "signaling socket connected; waiting for relay welcome");
    // 可见性（iter-61）：daemon 是长驻前台进程，用户只有 stdout 可看。日志默认进
    // 文件/stderr，「连上了没有」全靠猜——这里把关键节点直接打到 stdout。
    loop {
        let ev = match signaling.incoming.recv().await {
            Some(ev) => ev,
            None => {
                tracing::info!(target: "ridge_cli::daemon", "signaling closed");
                return Ok(());
            }
        };

        // controller 在场 → 起会话（host 作 answerer）。
        // `..` 忽略共享 schema 新增的 cid 字段：此处只判定「是否有 controller」，cid 的
        // 捕获/回盖在 RemoteSession 内进行（见 session.rs，从入站 offer 取 cid）。
        let controller_present = match ev {
            SignalMsg::Welcome { peer_present, .. } => {
                publish_status("online", "已连接信令中继，等待控制端接入");
                println!(
                    "✓ 已连接信令中继：{}（等待控制端接入…）",
                    auth.public_entry()
                );
                peer_present
            }
            SignalMsg::PeerJoin { ref role, .. } => *role == Role::Controller,
            SignalMsg::Error { code, message } => {
                tracing::warn!(target: "ridge_cli::daemon", %code, %message, "signaling error");
                if FATAL_SIGNAL_CODES.contains(&code.as_str()) {
                    // 终态：重连必然重蹈覆辙。打印可读指引并让 daemon 退出（非零码），
                    // 而非静默热重连——否则 controller 侧只能看到永远的「正在连接」。
                    eprintln!("✗ 云端拒绝本设备接入（{code}）：{message}");
                    eprintln!("  {}", fatal_hint(&code));
                    let detail = fatal_hint(&code).to_string();
                    publish_status("error", &format!("{code}: {detail}"));
                    return Err(FatalSignalError { code, detail }.into());
                }
                eprintln!("! 信令错误（{code}）：{message}");
                continue;
            }
            _ => false,
        };

        if controller_present {
            tracing::info!(target: "ridge_cli::daemon", "controller present; starting session");
            println!("→ 控制端已接入，正在建立加密会话…");
            // 会话借用 incoming 读 offer/ICE，并用 cheap-clone 的 sender 回 answer/ICE。
            // 零信任 #2：注入设备身份签名材料（host 握手发 0x02）。
            let endpoint=crate::kernel_ctl::ensure_kernel_running().map_err(anyhow::Error::msg)?;
            let remote_auth=ridge_kernel::auth_client::remote_auth(endpoint);
            remote_auth.call("remote_set_totp_identity",serde_json::json!({"username":auth.username})).map_err(anyhow::Error::msg)?;
            if let Err(e) = RemoteSession::run(
                &peer,
                ice_servers.clone(),
                &sender,
                &mut signaling.incoming,
                shell.clone(),
                cwd.clone(),
                root.clone(),
                HostIdentity {
                    device_identity,
                    device_name: &auth.device_name,
                    username: &auth.username,
                },
                &remote_auth,
            )
            .await
            {
                tracing::warn!(target: "ridge_cli::daemon", error = %e, "session ended with error");
            }
            tracing::info!(target: "ridge_cli::daemon", "session ended; waiting for next controller");
        }
    }
}
