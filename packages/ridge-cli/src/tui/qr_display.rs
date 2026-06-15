//! 终端二维码渲染：将 `otpauth://totp/...` URI 渲染为 Unicode 半块字符（Dense1x2）
//! 二维码，用户可在手机上用 Authenticator 应用扫描。

use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;

/// 将 URI 渲染为 Unicode 字符二维码字符串。
/// Dense1x2 用 ▄▀█ 实现 2× 垂直密度，输出比全⣿更紧凑。
pub fn render_qr(uri: &str) -> String {
    match QrCode::new(uri.as_bytes()) {
        Ok(code) => code.render::<Dense1x2>().module_dimensions(1, 1).build(),
        Err(e) => format!("(QR 生成失败: {e})"),
    }
}
