<#
.SYNOPSIS
  Installe (ou désinstalle) le menu contextuel Windows 11 de WinSAR, pour le développement.

.DESCRIPTION
  Signe le package sparse avec le certificat de développement, l'approuve sur
  la machine (UAC, une fois), puis l'enregistre en pointant vers target\<Configuration>,
  où se trouvent winsar-app.exe et winsar_shell.dll. L'installeur NSIS fait la même chose
  pour une installation normale, voir README.md.

.PARAMETER Configuration
  release (par défaut) ou debug.

.PARAMETER InstallDir
  Dossier d'une installation faite par l'installeur (par exemple %LOCALAPPDATA%\WinSAR).
  Sert à obtenir le menu principal de Windows 11 sur ce PC avec une version publiée non
  signée : le package est resigné avec le certificat de développement, approuvé ici seulement.

.PARAMETER Uninstall
  Retire le package. Le certificat reste en place pour les prochaines installations.

.PARAMETER RestartExplorer
  Redémarre l'Explorateur pour qu'il recharge la DLL tout de suite.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -RestartExplorer

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -InstallDir "$env:LOCALAPPDATA\WinSAR" -RestartExplorer
#>
[CmdletBinding()]
param(
  [ValidateSet("release", "debug")]
  [string] $Configuration = "release",
  [string] $InstallDir,
  [switch] $Uninstall,
  [switch] $RestartExplorer
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

if ($PSVersionTable.PSEdition -eq "Core") {
  # Sous PowerShell 7, le module Appx passe par Windows PowerShell.
  Import-Module Appx -UseWindowsPowerShell -WarningAction SilentlyContinue
}

function Restart-Explorer {
  Write-Host "Redémarrage de l'Explorateur…"
  Stop-Process -Name explorer -Force
  Start-Sleep -Seconds 2
  if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) { Start-Process explorer }
}

$existing = Get-AppxPackage -Name "WinSAR"
if ($existing) {
  Write-Host "Retrait de la version installée ($($existing.Version))…"
  $existing | Remove-AppxPackage
}
if ($Uninstall) {
  if ($RestartExplorer) { Restart-Explorer }
  Write-Host "Menu contextuel WinSAR désinstallé."
  return
}

$Bin = if ($InstallDir) { (Resolve-Path $InstallDir).Path } else { Join-Path $Root "target\$Configuration" }
foreach ($file in "winsar-app.exe", "winsar_shell.dll", "winsar_preview.dll") {
  if (-not (Test-Path (Join-Path $Bin $file))) {
    throw "$file absent de $Bin. Compilez d'abord, voir packaging\README.md."
  }
}

$cert = Get-WinSARCertificate
Confirm-WinSARTrust $cert
$msix = New-WinSARMsix $cert
Add-AppxPackage -Path $msix -ExternalLocation $Bin
# Le menu principal est en place : le menu classique de repli ferait doublon.
Start-Process (Join-Path $Bin "winsar-app.exe") -ArgumentList "--unregister-classic-menu" -Wait
if ($RestartExplorer) { Restart-Explorer }
Write-Host "Menu contextuel WinSAR installé, binaires : $Bin"
