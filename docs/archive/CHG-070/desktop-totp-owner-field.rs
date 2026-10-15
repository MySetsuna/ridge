    /// Remote Control auth — shared TOTP generator. Created on app startup;
    /// the same secret persists for the process lifetime.
    pub remote_auth: Arc<RemoteAuth>,
