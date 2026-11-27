; Sylva Desktop used to be called Cortex. Its installer put the app in a folder named after the
; old name, so after installing Sylva Desktop we remove the old Cortex install (its files,
; Start menu and desktop shortcuts, and its Apps list entry). Sign-ins and settings are kept:
; they live under the app identifier (app.pine.desktop), which did not change, and the old
; uninstaller only deletes app data when asked to.
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $R8 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Cortex" "UninstallString"
  ReadRegStr $R9 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Cortex" "InstallLocation"
  ${If} $R8 != ""
    ${If} $R9 == "$\"$INSTDIR$\""
      ; Installed over the old folder: the uninstaller there is ours now, so only tidy up.
      Delete "$INSTDIR\Cortex.exe"
      DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Cortex"
    ${Else}
      ExecWait '$R8 /S'
    ${EndIf}
  ${EndIf}
!macroend
