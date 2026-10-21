<#
.SYNOPSIS
  Change la version de Lynk partout où elle figure.

.DESCRIPTION
  Cargo.toml ([workspace.package]) fait foi ; ce script y reporte la nouvelle
  version, puis dans app/package.json, app/package-lock.json, tauri.conf.json et
  Cargo.lock. Le manifeste MSIX la lit lui-même au moment de l'empaquetage.

.EXAMPLE
  ./packaging/set-version.ps1 0.2.0
#>
param([Parameter(Mandatory)] [string] $Version)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "common.ps1")

# Windows n'accepte que des numéros dans la version d'un package MSIX.
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw "Version attendue sous la forme 1.2.3 (reçu : $Version)." }

function Set-InFile([string] $Path, [string] $Pattern) {
  $file = Join-Path $Root $Path
  $text = [System.IO.File]::ReadAllText($file)
  $new = [regex]::Replace($text, $Pattern, { param($m) $m.Groups[1].Value + $Version + $m.Groups[2].Value })
  if ($new -eq $text -and $text -notmatch [regex]::Escape($Version)) { throw "Version introuvable dans $Path." }
  [System.IO.File]::WriteAllText($file, $new, [System.Text.UTF8Encoding]::new($false))
  Write-Host "  $Path"
}

$old = Get-LynkVersion
Write-Host "Lynk $old → $Version"
Set-InFile "Cargo.toml" '(?ms)(^\[workspace\.package\].*?^version\s*=\s*")[^"]+(")'
Set-InFile "app/package.json" '(?m)(^  "version": ")[^"]+(")'
# Racine du fichier et paquet "" : les deux entrées au nom de l'application.
Set-InFile "app/package-lock.json" '("name": "lynk-app",\s*"version": ")[^"]+(")'
Set-InFile "app/src-tauri/tauri.conf.json" '(?m)(^  "version": ")[^"]+(")'

& cargo update --workspace --offline --manifest-path (Join-Path $Root "Cargo.toml")
if ($LASTEXITCODE -ne 0) { throw "Mise à jour de Cargo.lock impossible." }
Write-Host "  Cargo.lock"
Write-Host "Fait. Pour publier : commit, puis git tag v$Version et git push --tags."