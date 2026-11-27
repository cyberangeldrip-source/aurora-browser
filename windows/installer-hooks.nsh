!macro NSIS_HOOK_POSTINSTALL
  ExecWait '"$INSTDIR\aurora-browser.exe" --register-browser' $0
  ${If} $0 != 0
    DetailPrint "Регистрация браузера Aurora в Windows не завершилась."
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ExecWait '"$INSTDIR\aurora-browser.exe" --unregister-browser'
  ${EndIf}
!macroend
