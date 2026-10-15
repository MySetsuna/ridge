    let mcp_state =
        ridge_mcp::server::McpSessionState::with_sqlite(ridge_kernel::registry::agent_hub_path())
            .map_err(anyhow::Error::msg)
            .map(std::sync::Arc::new)
            .context("open persistent Agent Hub")?;
    let ctx = ridge_tmux::http::NativeHttpCtx::headless(token).with_mcp_state(mcp_state);
    ridge_tmux::http::serve(listener, ctx)
        .await
        .context("tmux engine server stopped")?;
