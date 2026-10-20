<#
.SYNOPSIS
  Prépare ce que l'installeur NSIS embarque pour le menu contextuel :
  target\release\lynk_shell.dll, target\release\lynk_preview.dll et target\Lynk.msix (signé).

.DESCRIPTION
  Appelé par `npm run bundle` (beforeBuildCommand de tauri.bundle.conf.json),
  avant la compilation de lynk-app : les deux fichiers sont des ressources du bundle.
#>
$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

Write-Host "Compilation de lynk_shell.dll et lynk_preview.dll (release)…"
& cargo build --release -p lynk-shell -p lynk-preview --manifest-path (Join-Path $Root "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "La compilation des extensions de l'Explorateur a échoué." }

$cert = Get-LynkCertificate
New-LynkMsix $cert | Out-Null
