; Sylva Control used to be called Sylva Desktop, and before that Cortex. Each installer put the
; app in a folder named after its name at the time, so after installing Sylva Control we remove
; any old install (its files, Start menu and desktop shortcuts, and its Apps list entry).
; Sign-ins and settings are kept: they live under the app identifier (app.pine.desktop), which
; never changed, and the old uninstallers only delete app data when asked to.
!macro REMOVE_OLD_INSTALL NAME
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}" "UninstallString"
  ReadRegStr $R9 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}" "InstallLocation"
  ${If} $R8 != ""
    ${If} $R9 == "$\"$INSTDIR$\""
      ; Installed over the old folder: the uninstaller there is ours now, so only tidy up.
      Delete "$INSTDIR\${NAME}.exe"
      DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\${NAME}"
    ${Else}
      ExecWait '$R8 /S'
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTINSTALL
  !insertmacro REMOVE_OLD_INSTALL "Cortex"
  !insertmacro REMOVE_OLD_INSTALL "Sylva Desktop"
!macroend
