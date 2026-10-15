//! Runtime ICE implementation; CLI supplies its existing configured API root.
pub use ridge_kernel::cloud::ice::IceServerConfig;
pub async fn fetch_ice_servers(client:&reqwest::Client,token:&str)->Vec<IceServerConfig> {
    ridge_kernel::cloud::ice::fetch_ice_servers(client,token,&crate::config::api_base()).await
}
