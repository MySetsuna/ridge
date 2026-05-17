!macro NSIS_HOOK_INIT
  Push $0
  ${GetParent} $INSTDIR $0
  StrCpy $INSTDIR "$0\ridge\${VERSION}"
  Pop $0
!macroend

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

