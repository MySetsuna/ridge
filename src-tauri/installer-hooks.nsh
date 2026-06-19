; Ridge NSIS 安装器 hooks
;
; 用 EnVar 插件（vendored 于 nsis-plugins/，见该目录 README）维护【用户】PATH，
; 而非 NSIS 原生 ReadRegStr/WriteRegStr：Tauri 自带的 makensis 以
; NSIS_MAX_STRLEN=1024 构建，ReadRegStr 读取超过 1024 字符的 PATH 会返回空串，
; 旧实现据此把用户 PATH 误判为空并整段覆盖——开发者的长 PATH 几乎必中。
; EnVar 走注册表 API，无此长度限制：AddValueEx 以 REG_EXPAND_SZ 追加并保留既有
; 内容（含 %VAR%）、已存在则幂等；DeleteValue 卸载时精确移除安装目录条目。
;
; ${__FILEDIR__} = 本文件所在目录，保证插件路径在 CI/任意机器可移植。
!addplugindir /x86-unicode "${__FILEDIR__}\nsis-plugins\x86-unicode"

; 覆盖 INSTDIR，使安装路径为 ...\ridge\<version>，而非 Tauri 默认的
; ...\ridge <version>（含空格）（如 C:\Program Files\ridge\0.1.0）
!macro NSIS_HOOK_INIT
  Push $0
  ${GetParent} $INSTDIR $0
  StrCpy $INSTDIR "$0\ridge\${VERSION}"
  Pop $0
!macroend

; 把安装目录加入用户 PATH。安装目录内同时含 ridge.exe（桌面 GUI）与 rdg.exe
; （无头 CLI，作为 bundle 资源随包安装），故安装后 `ridge` 与 `rdg` 两命令均可直接调用。
!macro NSIS_HOOK_POSTINSTALL
  Push $0
  EnVar::SetHKCU
  EnVar::AddValueEx "PATH" "$INSTDIR"
  Pop $0
  DetailPrint "Ridge: EnVar::AddValueEx PATH ($INSTDIR) -> $0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

; 卸载对称：从用户 PATH 精确移除安装目录条目（DeleteValue 递归删所有匹配）。
!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  EnVar::SetHKCU
  EnVar::DeleteValue "PATH" "$INSTDIR"
  Pop $0
  DetailPrint "Ridge: EnVar::DeleteValue PATH ($INSTDIR) -> $0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend
