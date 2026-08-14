; Ridge NSIS installer hooks.
;
; Ridge and rdg are separate products. This installer adds only the Ridge
; desktop directory to PATH; the optional rdg CLI is released separately.
!addplugindir /x86-unicode "${__FILEDIR__}\nsis-plugins\x86-unicode"

; Install to ...\ridge\<version>, rather than Tauri's default
; ...\ridge\<version> (with a duplicated version directory).
!macro NSIS_HOOK_INIT
  Push $0
  ${GetParent} $INSTDIR $0
  StrCpy $INSTDIR "$0\ridge\${VERSION}"
  Pop $0
!macroend

; Add only ridge.exe's installation directory to the user's PATH.
!macro NSIS_HOOK_POSTINSTALL
  Push $0
  EnVar::SetHKCU
  EnVar::AddValueEx "PATH" "$INSTDIR"
  Pop $0
  DetailPrint "Ridge: EnVar::AddValueEx PATH ($INSTDIR) -> $0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

; Remove exactly Ridge's installation directory from PATH on uninstall.
!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  EnVar::SetHKCU
  EnVar::DeleteValue "PATH" "$INSTDIR"
  Pop $0
  DetailPrint "Ridge: EnVar::DeleteValue PATH ($INSTDIR) -> $0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend
