; Crochets de l'installeur NSIS de Tauri : enregistrement des extensions de
; l'Explorateur (menu contextuel Windows 11, aperçu des archives) via le
; package MSIX « sparse » qui pointe vers le dossier d'installation.

; Une DLL chargée (explorer.exe, prevhost.exe) ne peut pas être écrasée, mais elle
; peut être renommée : on libère ainsi son nom pour la nouvelle version.
!macro WINSAR_RELEASE_DLL NAME
  IfFileExists "$INSTDIR\${NAME}" 0 +4
    Delete "$INSTDIR\${NAME}.old"
    Rename "$INSTDIR\${NAME}" "$INSTDIR\${NAME}.old"
    Delete /REBOOTOK "$INSTDIR\${NAME}.old"
!macroend

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Get-AppxPackage -Name WinSAR | Remove-AppxPackage"`
  !insertmacro WINSAR_RELEASE_DLL "winsar_shell.dll"
  !insertmacro WINSAR_RELEASE_DLL "winsar_preview.dll"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Échoue sans bloquer l'installation si le certificat n'est pas approuvé sur la
  ; machine. WinSAR pose alors un menu contextuel classique (« Afficher plus
  ; d'options »), qui n'a besoin d'aucune signature ; sinon, il retire ce repli.
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "try { Add-AppxPackage -Path '$INSTDIR\WinSAR.msix' -ExternalLocation '$INSTDIR' -ErrorAction Stop } catch { $$_; exit 1 }"`
  Pop $0
  StrCmp $0 "0" 0 +3
    ExecWait `"$INSTDIR\winsar-app.exe" --unregister-classic-menu`
    Goto +2
  ExecWait `"$INSTDIR\winsar-app.exe" --register-classic-menu`
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Get-AppxPackage -Name WinSAR | Remove-AppxPackage"`
  ExecWait `"$INSTDIR\winsar-app.exe" --unregister-classic-menu`
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete /REBOOTOK "$INSTDIR\winsar_shell.dll.old"
  Delete /REBOOTOK "$INSTDIR\winsar_preview.dll.old"
!macroend
