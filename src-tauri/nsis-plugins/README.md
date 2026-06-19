# Vendored NSIS 插件：EnVar

本目录随仓库内置（vendored）了 NSIS 的 **EnVar** 插件，供 `../installer-hooks.nsh`
在安装/卸载时安全地维护用户 PATH。

| 项 | 值 |
|---|---|
| 来源 | https://nsis.sourceforge.io/EnVar_plug-in |
| 下载包 | https://nsis.sourceforge.io/mediawiki/images/7/7f/EnVar_plugin.zip |
| 作者 | Jason Ross (JasonFriday13) · MouseHelmet Software |
| 许可 | zlib 式，见 `LICENSE.txt`（允许商业使用与重分发，须保留版权声明） |
| `x86-unicode/EnVar.dll` | SHA-256 `DBB0040CD73C83AAC965319EAAFE81A962154668EB2E7773D79A6A8040B446B0`，11264 bytes |

## 为什么要 vendored 这个插件

Tauri 自带的 NSIS（makensis v3.11）以 **`NSIS_MAX_STRLEN=1024`** 构建。NSIS 原生
`ReadRegStr` 读取**超过 1024 字符**的注册表值时会返回**空字符串**，旧的 hook 实现据此
把用户 PATH 误判为空，并用 `WriteRegExpandStr` 整段覆盖——开发者的长 PATH（nvm/pnpm/
go/JetBrains…）几乎必中，导致用户 PATH 被清空只剩安装目录。

EnVar 直接走注册表 API，不受该长度限制：

- `AddValueEx` 以 `REG_EXPAND_SZ` 追加并**保留既有内容**（含 `%VAR%`），已存在则幂等；
- `DeleteValue` 卸载时**精确移除**安装目录那一条，不动其他条目。

## 集成方式

NSIS 安装器/卸载器 stub 始终是 32 位 Unicode，故只需 `x86-unicode` 变体。
`installer-hooks.nsh` 通过

```nsis
!addplugindir /x86-unicode "${__FILEDIR__}\nsis-plugins\x86-unicode"
```

加载，`${__FILEDIR__}` 解析为 hook 文件自身所在目录，保证在 CI/任意机器上可移植。

## 升级插件

重新从上方「下载包」获取 zip，替换 `x86-unicode/EnVar.dll` 与 `LICENSE.txt`，
并更新本文件中的 SHA-256 与大小。
