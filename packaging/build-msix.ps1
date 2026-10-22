<#
.SYNOPSIS
  Prépare ce que l'installeur NSIS embarque pour le menu contextuel :
  target\release\winsar_shell.dll, target\release\winsar_preview.dll et target\WinSAR.msix (signé).

.DESCRIPTION
  Appelé par `npm run bundle` (beforeBuildCommand de tauri.bundle.conf.json),
  avant la compilation de winsar-app : les deux fichiers sont des ressources du bundle.
#>
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

Write-Host "Compilation de winsar_shell.dll et winsar_preview.dll (release)…"
& cargo build --release -p winsar-shell -p winsar-preview --manifest-path (Join-Path $Root "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "La compilation des extensions de l'Explorateur a échoué." }

$cert = Get-WinSARCertificate
New-WinSARMsix $cert | Out-Null
