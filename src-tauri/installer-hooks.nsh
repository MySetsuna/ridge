; Ridge NSIS installer hooks
;
; Append $INSTDIR to the current-user PATH so `tmux` (the Ridge shim) resolves
; from any shell — not only inside Ridge's own PTY panes.
;
; Uses StrFunc ${StrLoc} (declared in the generated installer.nsi) for
; idempotent detection, and WordFunc ${WordReplace} (installer.nsi already
; `!include`s WordFunc.nsh) for safe removal on uninstall.

!include "WinMessages.nsh"

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
