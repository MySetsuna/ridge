#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneOriginDto {
    Headless {
        host_id: String,
        host_label: String,
        session_id: String,
    },
    Remote {
        host_id: String,
        host_label: String,
        session_id: String,
    },
    Rdg {
        host_id: String,
        host_label: String,
        session_id: String,
    },
}

