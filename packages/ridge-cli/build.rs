// 让 cargo 在 `RIDGE_BASE_DOMAIN` 编译期环境变量变化时重编 ——
// config.rs 用 `option_env!("RIDGE_BASE_DOMAIN")` 把 base zone 烘焙进二进制
// （debug 包指向 localhost:5173 等本地 cloud）。没有这行，cargo 不追踪
// option_env! 依赖的 env，改了地址重建会命中缓存、烘焙不进去。
fn main() {
    println!("cargo:rerun-if-env-changed=RIDGE_BASE_DOMAIN");
}
