//! Existing UI commands share the typed Git executors used by the private domain API.
use ridge_core::{CoreError, CoreResult, Ctx, sandbox::RootScope};
use serde_json::{json, Value};

pub(crate) async fn dispatch(method: &str, args: &Value, scope: &RootScope, ctx: &Ctx) -> Option<CoreResult<Value>> {
    let read = match method {
        "git_get_commit_files" => Some("commit-files"),
        "git_get_file_versions_at_commit" => Some("file-versions-at-commit"),
        "git_get_file_versions_between" => Some("file-versions-between"),
        "git_compare_commits" => Some("compare-commits"),
        _ => None,
    };
    if read.is_none() && !matches!(method,
        "git_stage" | "git_unstage" | "git_discard" | "git_clean_untracked" | "git_commit"
        | "git_checkout" | "git_push" | "git_push_branch" | "git_merge_branch" | "git_delete_branch"
        | "git_rename_branch" | "git_rebase" | "git_delete_tag" | "git_push_tag" | "git_stash_push"
        | "git_stash_apply" | "git_stash_pop" | "git_stash_drop" | "git_stash_branch" | "git_fetch"
        | "git_pull" | "git_sync" | "git_cherry_pick_abort" | "git_revert_abort" | "git_cherry_pick"
        | "git_revert" | "git_create_tag" | "git_reset") { return None; }
    Some(async {
        let repo_root = args["repoRoot"].as_str().ok_or_else(|| CoreError::InvalidArgs("missing repoRoot".into()))?;
        // A permitted subdirectory must not authorize writes to its enclosing repository.
        if !scope.is_unrestricted() || !ctx.capabilities().root_scope().is_unrestricted() {
            let detect_path = repo_root.to_owned();
            if let Some(actual) = tokio::task::spawn_blocking(move || ridge_core::commands::git::find_git_repo_root(detect_path))
                .await.map_err(CoreError::internal)? {
                if !scope.is_allowed(&actual) || !ctx.capabilities().root_scope().is_allowed(&actual) { return Err(CoreError::OutsideSandbox); }
            }
        }
        let mut request = args.as_object().cloned().ok_or_else(|| CoreError::InvalidArgs("Git arguments must be an object".into()))?;
        for (camel, snake) in [("repoRoot", "repo_root"), ("setUpstream", "set_upstream"),
            ("oldName", "old_name"), ("newName", "new_name"), ("includeUntracked", "include_untracked")] {
            if let Some(value) = request.remove(camel) { request.insert(snake.into(), value); }
        }
        request.insert("repo_root".into(), json!(repo_root));
        request.insert("operation".into(), json!(read.map(str::to_owned).unwrap_or_else(|| method.trim_start_matches("git_").replace('_', "-"))));
        let response = if read.is_some() {
            let typed = serde_json::from_value(Value::Object(request)).map_err(|error| CoreError::InvalidArgs(format!("invalid Git read: {error}")))?;
            crate::domain::execute_git_read(typed).await
        } else {
            let typed = serde_json::from_value(Value::Object(request)).map_err(|error| CoreError::InvalidArgs(format!("invalid Git mutation: {error}")))?;
            crate::domain::execute_git_mutation(typed).await
        }.map_err(|_| CoreError::internal("Git executor unavailable"))?.0;
        if response["ok"] != true { return Err(CoreError::internal(response["error"].as_str().unwrap_or("Git operation failed"))); }
        if response["is_repo"] != true { return Err(CoreError::internal(format!("Not a git repo: {}", response["repo_root"].as_str().unwrap_or_default()))); }
        Ok(response[if read.is_some() { "value" } else { "output" }].clone())
    }.await)
}
