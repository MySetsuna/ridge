fn main() {
    // Windows 测试宿主载败修复（iteration 7 G1）：依赖树引入
    // `comctl32!TaskDialogIndirect`（仅 common-controls **v6** 导出）。tauri 主 exe 由
    // tauri-build 嵌入声明 v6 依赖的 manifest；而 cargo 的 lib 单测宿主没有 manifest，
    // 加载器把 comctl32 绑到 WinSxS 5.82 → 进程启动即 STATUS_ENTRYPOINT_NOT_FOUND
    // (0xc0000139)。修法：把 comctl32 改为**延迟加载**——绑定推迟到首次真正调用
    // （测试从不弹对话框 ⇒ 永不绑定；主 exe 首调时其嵌入 manifest 的 v6 激活上下文
    // 已生效）。不能用 /MANIFEST:EMBED（会与 tauri-build 的 RT_MANIFEST 资源冲突），
    // `rustc-link-arg-tests` 又不覆盖 lib 单测宿主，故全局注入。非 MSVC 不注入。
    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    {
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg=/DEFAULTLIB:delayimp.lib");
    }
    tauri_build::build()
}
