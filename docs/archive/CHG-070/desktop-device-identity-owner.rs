    /// 零信任 #2：Ed25519 长期**设备身份**。私钥留 Rust（DPAPI/0600），仅经 invoke
    /// 暴露公钥/签名能力，**绝不进 JS/localStorage**。进程启动 `load_or_create`。
    pub device_identity: Arc<ridge_core::DeviceIdentity>,
            device_identity: Arc::new(if crate::utils::cdp::isolated() {
                ridge_core::DeviceIdentity::load_or_create_at(&crate::utils::cdp::local_data_dir().join("device_identity.key"))
                    .expect("CDP identity must initialize in its owned namespace")
            } else {ridge_core::DeviceIdentity::load_or_create()}),
