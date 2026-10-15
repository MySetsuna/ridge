; Ridge NSIS installer hooks.
;
; Ridge and rdg are separate products. This installer adds only the Ridge
; desktop directory to PATH; the optional rdg CLI is released separately.
!addplugindir /x86-unicode "${__FILEDIR__}\nsis-plugins\x86-unicode"
!define RIDGE_INSTALLER_RUNTIME "${__FILEDIR__}\..\target\runtime-binaries\ridge-kernel-installer.exe"

; Run before the maintenance page can invoke an older uninstaller.
!define MUI_CUSTOMFUNCTION_GUIINIT RidgeRuntimePreflight
Var RidgeRuntimeGuard

Function RidgeRuntimePreflight
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=ridge-installer-runtime.exe "${RIDGE_INSTALLER_RUNTIME}"
  nsExec::ExecToStack '"$PLUGINSDIR\ridge-installer-runtime.exe" --installer-check'
  Pop $0
  Pop $1
  ${If} $0 != 0
    DetailPrint "$1"
    MessageBox MB_OK|MB_ICONSTOP "Ridge installation cannot continue. Explicitly stop the Runtime service, then retry.$\r$\n$1" /SD IDOK
    SetErrorLevel 2
    Quit
  ${EndIf}
FunctionEnd

; Reserve both boot and process locks until installation completes. The helper
; observes this exact installer process instance and releases on abort/crash.
!macro RidgeRuntimeReserve
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=ridge-installer-runtime.exe "${RIDGE_INSTALLER_RUNTIME}"
  System::Call 'kernel32::GetCurrentProcessId() i .r0'
  Exec '"$PLUGINSDIR\ridge-installer-runtime.exe" --installer-parent $0 --installer-status "$PLUGINSDIR\ridge-runtime.status" --installer-release "$PLUGINSDIR\ridge-runtime.release"'
  StrCpy $RidgeRuntimeGuard 1
  StrCpy $2 0
  ${Do}
    ClearErrors
    FileOpen $0 "$PLUGINSDIR\ridge-runtime.status" r
    ${IfNot} ${Errors}
      FileRead $0 $1
      FileClose $0
      ${If} $1 == "READY"
        ${ExitDo}
      ${ElseIf} $1 != ""
        DetailPrint "$1"
        MessageBox MB_OK|MB_ICONSTOP "Ridge installation cannot reserve the Runtime.$\r$\n$1" /SD IDOK
        SetErrorLevel 2
        Quit
      ${EndIf}
    ${EndIf}
    IntOp $2 $2 + 1
    ${If} $2 >= 100
      MessageBox MB_OK|MB_ICONSTOP "Ridge Runtime reservation timed out. Installation stopped; data preserved." /SD IDOK
      SetErrorLevel 2
      Quit
    ${EndIf}
    Sleep 100
  ${Loop}
  SetOutPath "$INSTDIR"
!macroend

!macro RidgeRuntimeRelease
  ${If} $RidgeRuntimeGuard == 1
    FileOpen $0 "$PLUGINSDIR\ridge-runtime.release" w
    FileWrite $0 "RELEASE"
    FileClose $0
    StrCpy $2 0
    ${Do}
      ${If} ${FileExists} "$PLUGINSDIR\ridge-runtime.done"
        ${ExitDo}
      ${EndIf}
      IntOp $2 $2 + 1
      ${If} $2 >= 100
        SetErrorLevel 2
        Quit
      ${EndIf}
      Sleep 100
    ${Loop}
    StrCpy $RidgeRuntimeGuard 0
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro RidgeRuntimeReserve
!macroend

; Add only ridge.exe's installation directory to the user's PATH.
!macro NSIS_HOOK_POSTINSTALL
  !insertmacro RidgeRuntimeRelease
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
  !insertmacro RidgeRuntimeReserve
  Push $0
  EnVar::SetHKCU
  EnVar::DeleteValue "PATH" "$INSTDIR"
  Pop $0
  DetailPrint "Ridge: EnVar::DeleteValue PATH ($INSTDIR) -> $0"
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  !insertmacro RidgeRuntimeRelease
!macroend
