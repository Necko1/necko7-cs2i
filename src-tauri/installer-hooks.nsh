!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
  ${If} $UpdateMode != 1
    ExecWait '"$INSTDIR\necko7-cs2i.exe" --uninstall-cleanup'
  ${EndIf}
!macroend
