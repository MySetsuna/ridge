// `embed-ui` feature 用 rust-embed 把 `remote-dist` 统一产物编进二进制。rust-embed 在目录
// **不存在**时会编译期报错——而 `cargo build` 完全可能在前端产物之前跑（纯 Rust
// 开发轨、CI 的 cargo-only 步骤）。这里预建空目录把「没构建前端」从**编译失败**
// 降级为**运行期回落磁盘探测**（与 embed 关闭时同行为）。
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let remote = root.join("remote-dist");
    let _ = std::fs::create_dir_all(remote.join("desktop"));
    let _ = std::fs::create_dir_all(remote.join("mobile"));
    println!("cargo:rerun-if-changed={}", remote.display());
}
