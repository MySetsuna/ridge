//! Runtime-owned TypeScript/JavaScript and Rust language servers over bounded stdio JSON-RPC.
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use crate::business::BusinessEvents;
use ridge_core::EventSink;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::sync::{oneshot, Mutex};

pub(crate) fn owns_method(method: &str) -> bool {
    matches!(method, "lsp_did_open" | "lsp_did_change" | "lsp_definition" | "lsp_hover" | "lsp_references")
}

pub(crate) async fn dispatch(state: &crate::server::AppState, method: &str, args: &Value, native: bool, ctx: &ridge_core::Ctx) -> Result<Value, String> {
    if !native { return Err("LSP requires native trust".into()); }
    let string = |key: &str| args[key].as_str().map(str::to_owned).ok_or_else(|| format!("missing {key}"));
    let root = std::path::PathBuf::from(string("workspaceRoot")?).canonicalize().map_err(|error| error.to_string())?;
    if !root.is_dir() || !state.fs_scope.is_allowed(&root.to_string_lossy()) || !ctx.capabilities().root_scope().is_allowed(&root.to_string_lossy()) {
        return Err("LSP workspace outside sandbox".into());
    }
    let uri = string("uri")?;
    let parsed = url::Url::parse(&uri).map_err(|_| "invalid LSP document URI")?;
    if parsed.query().is_some() || parsed.fragment().is_some() { return Err("invalid LSP document URI".into()); }
    let file = parsed.to_file_path().map_err(|_| "LSP document must use a local file URI")?.canonicalize().map_err(|error| error.to_string())?;
    if !file.starts_with(&root) || !file.is_file() || !state.fs_scope.is_allowed(&file.to_string_lossy()) { return Err("LSP document outside workspace".into()); }
    let root = root.to_string_lossy().into_owned();
    match method {
        "lsp_did_open" | "lsp_did_change" => {
            let text = string("text")?;
            if text.len() > 8 * 1024 * 1024 { return Err("LSP document too large".into()); }
            if method == "lsp_did_open" {
                lsp_did_open(root, uri, string("languageId")?, text).await?;
            } else {
                lsp_did_change(root, uri, args["version"].as_i64().ok_or("invalid LSP document version")?, text).await?;
            }
            Ok(Value::Null)
        }
        _ => {
            let position = |key: &str| args[key].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("invalid LSP {key}"));
            let line = position("line")?;
            let character = position("character")?;
            match method {
                "lsp_definition" => lsp_definition(root, uri, line, character).await,
                "lsp_hover" => lsp_hover(root, uri, line, character).await,
                "lsp_references" => lsp_references(root, uri, line, character).await,
                _ => Err("unknown LSP method".into()),
            }
        }
    }
}

/// 全局 LSP 管理器（进程级；语言服务器是 app 级资源）。
static MANAGER: OnceLock<LspManager> = OnceLock::new();

pub fn manager() -> &'static LspManager {
    MANAGER.get_or_init(LspManager::new)
}

static EVENTS: OnceLock<BusinessEvents> = OnceLock::new();

pub(crate) fn set_events(events: BusinessEvents) { let _ = EVENTS.set(events); }

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Value>>>>;

struct PendingRequest { pending: Pending, id: i64 }
impl Drop for PendingRequest {
    fn drop(&mut self) {
        let pending = self.pending.clone();
        let id = self.id;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move { pending.lock().await.remove(&id); });
        }
    }
}

pub struct LspManager {
    /// key = workspace_root（每个工作区一个 TS server）。
    servers: Mutex<HashMap<String, Arc<Server>>>,
}

impl LspManager {
    fn new() -> Self {
        Self {
            servers: Mutex::new(HashMap::new()),
        }
    }

    /// 取（或惰性起 + initialize）某 (语言, 工作区) 的 server。
    async fn ensure(&self, root: &str, kind: ServerKind) -> Result<Arc<Server>, String> {
        let map_key = format!("{}\u{1f}{}", kind.key(), root);
        {
            let mut servers = self.servers.lock().await;
            if let Some(s) = servers.get(&map_key) {
                if !s.closed.load(Ordering::Acquire) { return Ok(s.clone()); }
            }
            servers.remove(&map_key);
        }
        // 起进程（锁外做，spawn + initialize 可能耗时；并发首次请求可能重复起一个，
        // 用末尾的 entry 去重——后到的丢弃自己起的那个）。
        let server = Arc::new(Server::spawn(root, kind).await?);
        let mut servers = self.servers.lock().await;
        if let Some(existing) = servers.get(&map_key) {
            return Ok(existing.clone());
        }
        servers.insert(map_key, server.clone());
        Ok(server)
    }

    /// 进程死亡时从表里剔除（下次请求重起）。
    async fn drop_server(&self, root: &str, kind: ServerKind) {
        self.servers
            .lock()
            .await
            .remove(&format!("{}\u{1f}{}", kind.key(), root));
    }
}

struct Server {
    stdin: Mutex<ChildStdin>,
    pending: Pending,
    next_id: AtomicI64,
    closed: Arc<AtomicBool>,
    _child: tokio::process::Child,
    process: ridge_core::process_identity::ProcessInstance,
}

impl Drop for Server {
    fn drop(&mut self) {
        if ridge_core::process_identity::validate(&self.process).is_ok() {
            ridge_core::process_guard::kill_process_tree(self.process.pid);
        }
    }
}

pub(crate) async fn shutdown() {
    let servers = std::mem::take(&mut *manager().servers.lock().await);
    let _ = tokio::task::spawn_blocking(move || drop(servers)).await;
}

impl Server {
    async fn spawn(root: &str, kind: ServerKind) -> Result<Server, String> {
        let mut cmd = kind.command();
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()).kill_on_drop(true).current_dir(root);
        #[cfg(windows)]
        {
            // 不弹出 cmd 黑窗（CREATE_NO_WINDOW）。tokio::process::Command 在 Windows
            // 上自带 creation_flags（委托内部 std Command），无需额外 trait import。
            cmd.creation_flags(0x0800_0000);
        }
        #[cfg(unix)] { use std::os::unix::process::CommandExt; cmd.as_std_mut().process_group(0); }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动语言服务器失败（{}）：{e}", kind.install_hint()))?;
        let process = ridge_core::process_identity::instance(child.id().ok_or("LSP process already exited")?).map_err(|error| error.to_string())?;
        let stdin = child.stdin.take().ok_or("LSP: 无法获取 stdin")?;
        let stdout = child.stdout.take().ok_or("LSP: 无法获取 stdout")?;
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let closed = Arc::new(AtomicBool::new(false));
        tokio::spawn(read_loop(stdout, pending.clone(), closed.clone()));
        let server = Server {
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicI64::new(1),
            closed,
            _child: child,
            process,
        };
        server.initialize(root).await?;
        Ok(server)
    }

    async fn initialize(&self, root: &str) -> Result<(), String> {
        let root_uri = url::Url::from_directory_path(root).map_err(|_| "invalid LSP root URI")?.to_string();
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "workspaceFolders": [{ "uri": root_uri, "name": "root" }],
            "capabilities": {
                "textDocument": {
                    "definition": { "linkSupport": true },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "publishDiagnostics": { "relatedInformation": false },
                    "synchronization": { "didSave": false, "dynamicRegistration": false }
                }
            }
        });
        self.request("initialize", params).await?;
        self.notify("initialized", json!({})).await?;
        Ok(())
    }

    async fn request(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.len() >= 256 { return Err("LSP pending request limit reached".into()); }
            pending.insert(id, tx);
        }
        let _pending = PendingRequest { pending: self.pending.clone(), id };
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.write_frame(&msg).await {
            self.pending.lock().await.remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_secs(15), rx).await {
            Ok(Ok(v)) => if let Some(error) = v.get("__lsp_error") { Err(format!("LSP server rejected request: {error}")) } else { Ok(v) },
            Ok(Err(_)) => Err("LSP 响应通道关闭（服务器可能已退出）".into()),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err("LSP 请求超时".into())
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.write_frame(&msg).await
    }

    async fn write_frame(&self, msg: &Value) -> Result<(), String> {
        let body = serde_json::to_string(msg).map_err(|e| e.to_string())?;
        let frame = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(frame.as_bytes())
            .await
            .map_err(|e| format!("LSP 写入失败: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("LSP flush 失败: {e}"))?;
        Ok(())
    }
}

async fn read_lsp_headers(reader: &mut BufReader<ChildStdout>) -> Result<usize, ()> {
    let mut content_length = 0;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        match reader.take(8193).read_line(&mut line).await {
            Ok(0) | Err(_) => return Err(()),
            Ok(_) => {}
        }
        header_bytes += line.len();
        if line.len() > 8192 || header_bytes > 16384 { return Err(()); }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return Ok(content_length);
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
    }
}

async fn read_lsp_message(reader: &mut BufReader<ChildStdout>) -> Result<Option<Value>, ()> {
    let content_length = read_lsp_headers(reader).await?;
    if content_length == 0 {
        return Ok(None);
    }
    if content_length > 8 * 1024 * 1024 { return Err(()); }
    let mut buf = vec![0u8; content_length];
    reader.read_exact(&mut buf).await.map_err(|_| ())?;
    Ok(serde_json::from_slice::<Value>(&buf).ok())
}

async fn route_lsp_message(msg: Value, pending: &Pending) {
    let id = msg.get("id").and_then(Value::as_i64);
    let method = msg.get("method").and_then(Value::as_str);
    match (id, method) {
        // 响应（有 id、无 method）→ 路由回对应 pending oneshot。
        (Some(id), None) => {
            let result = msg
                .get("result")
                .cloned()
                .or_else(|| msg.get("error").map(|e| json!({ "__lsp_error": e })))
                .unwrap_or(Value::Null);
            if let Some(tx) = pending.lock().await.remove(&id) {
                let _ = tx.send(result);
            }
        }
        // 通知（有 method、无 id）→ P2：转发诊断给前端（其余忽略）。
        (None, Some("textDocument/publishDiagnostics")) => {
            if let Some(events) = EVENTS.get() {
                events.broadcast(
                    "lsp://diagnostics",
                    msg.get("params").cloned().unwrap_or(Value::Null),
                );
            }
        }
        // 服务器→客户端请求（id+method，如 workspace/configuration）/ 其它通知：P1/P2 忽略。
        _ => {}
    }
}

/// stdout 读循环：解析 `Content-Length` 分帧，把响应（有 id 无 method）路由回
/// 对应 pending oneshot；通知 / 服务器→客户端请求（P1）暂忽略。EOF/错误即退出
/// （服务器死亡）。
async fn read_loop(stdout: ChildStdout, pending: Pending, closed: Arc<AtomicBool>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let message = match read_lsp_message(&mut reader).await {
            Ok(Some(message)) => message,
            Ok(None) => continue,
            Err(()) => { closed.store(true, Ordering::Release); pending.lock().await.clear(); return; },
        };
        route_lsp_message(message, &pending).await;
    }
}

/// 支持的语言服务器种类（按文件扩展名路由）。P3 多语言：TS/JS + Rust。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ServerKind {
    TypeScript,
    Rust,
}

impl ServerKind {
    /// 由文件 URI/路径扩展名判定语言服务器；不支持的语言返回 None。
    fn from_uri(uri: &str) -> Option<Self> {
        let name = uri
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(uri)
            .to_ascii_lowercase();
        const TS: [&str; 8] = [".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".mjs", ".cjs"];
        if TS.iter().any(|e| name.ends_with(e)) {
            Some(Self::TypeScript)
        } else if name.ends_with(".rs") {
            Some(Self::Rust)
        } else {
            None
        }
    }

    fn key(&self) -> &'static str {
        match self {
            Self::TypeScript => "ts",
            Self::Rust => "rust",
        }
    }

    /// 起服务器的命令。TS 经 npm 全局 bin（Windows `.cmd` shim → `cmd /c`，因 Rust
    /// CreateProcess 不查 PATHEXT）；rust-analyzer 是原生 exe，直接调（Command 在
    /// Windows 自动补 `.exe` + 查 PATH）。
    fn command(&self) -> Command {
        match self {
            Self::TypeScript => {
                if cfg!(windows) {
                    let mut c = Command::new("cmd");
                    c.args(["/c", "typescript-language-server", "--stdio"]);
                    c
                } else {
                    let mut c = Command::new("typescript-language-server");
                    c.arg("--stdio");
                    c
                }
            }
            Self::Rust => Command::new("rust-analyzer"),
        }
    }

    /// 起进程失败时的安装提示（供给检测，P3）。
    fn install_hint(&self) -> &'static str {
        match self {
            Self::TypeScript => "请全局安装：npm i -g typescript-language-server typescript",
            Self::Rust => {
                "请安装 rust-analyzer（rustup component add rust-analyzer 或下载二进制并加入 PATH）"
            }
        }
    }
}

/// 文件路径 → `file://` URI（LSP 用）。Windows `C:\a\b` → `file:///C:/a/b`。
#[cfg(test)]
fn path_to_uri(path: &str) -> String {
    let p = path.replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

// ── Runtime 文档操作 ───────────────────────────────────────────────────────────────

/// 文档打开：把当前编辑器文件内容同步给 LSP（definition 解析需要打开缓冲区）。
pub async fn lsp_did_open(
    workspace_root: String,
    uri: String,
    language_id: String,
    text: String,
) -> Result<(), String> {
    let Some(kind) = ServerKind::from_uri(&uri) else {
        return Ok(());
    };
    let server = manager().ensure(&workspace_root, kind).await?;
    let r = server
        .notify(
            "textDocument/didOpen",
            json!({
                "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text }
            }),
        )
        .await;
    if r.is_err() {
        manager().drop_server(&workspace_root, kind).await;
    }
    r
}

/// 文档变更（全量同步）：编辑后把新内容推给 LSP，保证 definition 位置准确。
pub async fn lsp_did_change(
    workspace_root: String,
    uri: String,
    version: i64,
    text: String,
) -> Result<(), String> {
    let Some(kind) = ServerKind::from_uri(&uri) else {
        return Ok(());
    };
    let server = manager().ensure(&workspace_root, kind).await?;
    let r = server
        .notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }]
            }),
        )
        .await;
    if r.is_err() {
        manager().drop_server(&workspace_root, kind).await;
    }
    r
}

/// go-to-definition：返回原始 LSP 结果（Location | Location[] | LocationLink[] |
/// null）。前端 lspClient 负责解析为 {path,line,col} 并 openFile。
pub async fn lsp_definition(
    workspace_root: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<Value, String> {
    let Some(kind) = ServerKind::from_uri(&uri) else {
        return Ok(Value::Null);
    };
    let server = manager().ensure(&workspace_root, kind).await?;
    server
        .request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
}

/// hover：返回原始 LSP Hover（{ contents, range? } | null）。前端解析 contents
/// （MarkupContent | MarkedString | MarkedString[]）为 Markdown。
pub async fn lsp_hover(
    workspace_root: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<Value, String> {
    let Some(kind) = ServerKind::from_uri(&uri) else {
        return Ok(Value::Null);
    };
    let server = manager().ensure(&workspace_root, kind).await?;
    server
        .request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
}

/// references（Find All References）：返回 Location[]。前端 `parseDefinition` 同样
/// 适用（Location[] → LspTarget[]）。`includeDeclaration` 一并返回声明处。
pub async fn lsp_references(
    workspace_root: String,
    uri: String,
    line: u32,
    character: u32,
) -> Result<Value, String> {
    let Some(kind) = ServerKind::from_uri(&uri) else {
        return Ok(Value::Null);
    };
    let server = manager().ensure(&workspace_root, kind).await?;
    server
        .request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true }
            }),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_to_uri_windows_and_posix() {
        assert_eq!(path_to_uri("C:\\a\\b.ts"), "file:///C:/a/b.ts");
        assert_eq!(path_to_uri("/home/u/a.ts"), "file:///home/u/a.ts");
    }

    #[test]
    fn server_kind_routes_by_extension() {
        assert!(matches!(
            ServerKind::from_uri("file:///c:/a/b.ts"),
            Some(ServerKind::TypeScript)
        ));
        assert!(matches!(
            ServerKind::from_uri("file:///c:/a/b.tsx"),
            Some(ServerKind::TypeScript)
        ));
        assert!(matches!(
            ServerKind::from_uri("file:///c:/a/main.rs"),
            Some(ServerKind::Rust)
        ));
        assert!(ServerKind::from_uri("file:///c:/a/readme.md").is_none());
        assert!(ServerKind::from_uri("file:///c:/a/x.svelte").is_none());
    }
}
