    // The native `/api/v1/tmux/*` surface (everything except GUI-only summon) is
    // the SAME shared router the headless `ridge-cli` host mounts — one source of
    // truth in `ridge_tmux::http`. The desktop supplies a GUI-backed session
    // source so default-socket find-target still folds in visible workspaces.
    let native_ctx = ridge_tmux::http::NativeHttpCtx {
        token: ctx.token.clone(),
        gui: Arc::new(DesktopGuiSessions {
            state: ctx.state.clone(),
        }),
        mcp_state: super::mcp::desktop_hub_state(),
    };

