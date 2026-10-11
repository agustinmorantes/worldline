; Puts worldline-cli on the user's PATH so it works from any terminal opened afterwards.

!macro NSIS_HOOK_POSTINSTALL
  nsExec::Exec '"$INSTDIR\worldline-cli.exe" user-path add'
  Pop $0
  ${If} $0 <> 0
    DetailPrint "Couldn't add worldline-cli to PATH (exit code $0)"
  ${EndIf}
  SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; An update reinstalls right after, so the entry stays.
  ${If} $UpdateMode <> 1
    nsExec::Exec '"$INSTDIR\worldline-cli.exe" user-path remove'
    Pop $0
    SendMessage ${HWND_BROADCAST} ${WM_SETTINGCHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
!macroend
