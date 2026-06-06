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
  Push $1
  ReadRegStr $0 HKCU "Environment" "PATH"
  ${If} $0 == ""
    WriteRegExpandStr HKCU "Environment" "PATH" "$INSTDIR"
  ${Else}
    ${StrLoc} $1 "$0" "$INSTDIR" ">"
    ${If} $1 == ""
      WriteRegExpandStr HKCU "Environment" "PATH" "$0;$INSTDIR"
    ${EndIf}
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Push $0
  Push $1
  ReadRegStr $0 HKCU "Environment" "PATH"
  ${If} $0 != ""
    ${WordReplace} "$0" ";$INSTDIR" "" "+" $1
    ${WordReplace} "$1" "$INSTDIR;" "" "+" $0
    ${WordReplace} "$0" "$INSTDIR" "" "+" $1
    WriteRegExpandStr HKCU "Environment" "PATH" "$1"
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
  Pop $1
  Pop $0
!macroend

