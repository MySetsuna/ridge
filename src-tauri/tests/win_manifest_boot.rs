//! G1（iteration 7）：测试宿主可启动性冒烟。链接 ridge_lib rlib 即拉入完整静态
//! 依赖面（含 comctl32 v6 独有的 TaskDialogIndirect 导入）；本目标能启动并跑过
//! 一条断言，即证明 build.rs 注入的 common-controls v6 manifest 依赖生效。
use ridge_lib as _;

#[test]
fn test_harness_boots_with_v6_common_controls() {
    assert_eq!(1 + 1, 2);
}
