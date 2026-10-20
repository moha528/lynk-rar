; Crochets de l'installeur NSIS de Tauri : enregistrement des extensions de
; l'Explorateur (menu contextuel Windows 11, aperçu des archives) via le
; package MSIX « sparse » qui pointe vers le dossier d'installation.

; Une DLL chargée (explorer.exe, prevhost.exe) ne peut pas être écrasée, mais elle
; peut être renommée : on libère ainsi son nom pour la nouvelle version.
!macro LYNK_RELEASE_DLL NAME
  IfFileExists "$INSTDIR\${NAME}" 0 +4
    Delete "$INSTDIR\${NAME}.old"
    Rename "$INSTDIR\${NAME}" "$INSTDIR\${NAME}.old"
    Delete /REBOOTOK "$INSTDIR\${NAME}.old"
!macroend

!macro NSIS_HOOK_PREINSTALL
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Get-AppxPackage -Name Lynk | Remove-AppxPackage"`
  !insertmacro LYNK_RELEASE_DLL "lynk_shell.dll"
  !insertmacro LYNK_RELEASE_DLL "lynk_preview.dll"
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Échoue sans bloquer l'installation si le certificat n'est pas approuvé sur
  ; la machine : Lynk marche alors, simplement sans intégration à l'Explorateur.
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Add-AppxPackage -Path '$INSTDIR\Lynk.msix' -ExternalLocation '$INSTDIR'"`
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::ExecToLog `powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "Get-AppxPackage -Name Lynk | Remove-AppxPackage"`
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Delete /REBOOTOK "$INSTDIR\lynk_shell.dll.old"
  Delete /REBOOTOK "$INSTDIR\lynk_preview.dll.old"
!macroend
