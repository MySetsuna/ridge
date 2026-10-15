    if response["error"]["code"] == -32601 && matches!(method,"git_stage"|"git_unstage"|"git_commit") {
        let mut request = json!({"operation":method.trim_start_matches("git_"),"repo_root":args["repoRoot"]});
        if method=="git_commit" {request["message"]=args["message"].clone();request["amend"]=args["amend"].clone();}
        else {request["paths"]=args["paths"].clone();}
        serde_json::from_value::<crate::domain::GitMutationRequest>(request.clone()).map_err(|error|internal(format!("invalid Git mutation: {error}")))?;
        return crate::client::mutate_domain_git(&host.current_endpoint(),&request).map(|_|Value::Null).map_err(internal);
    }
