# Fonctions partagées par install-shell.ps1 et build-msix.ps1 (à dot-sourcer).

# Lancé depuis PowerShell 7 (npm, GitHub Actions), Windows PowerShell hérite de son
# PSModulePath et charge alors des modules prévus pour PowerShell 7 : le lecteur
# Cert: et Add-AppxPackage disparaissent. On revient à ses propres chemins.
if ($PSVersionTable.PSEdition -eq "Desktop") {
  $env:PSModulePath = @(
    (Join-Path ([Environment]::GetFolderPath("MyDocuments")) "WindowsPowerShell\Modules"),
    (Join-Path $env:ProgramFiles "WindowsPowerShell\Modules"),
    (Join-Path $PSHOME "Modules")
  ) -join ";"
  Import-Module Microsoft.PowerShell.Security -ErrorAction Stop
}

$Script:Root = Split-Path -Parent $PSScriptRoot
$Script:Publisher = "CN=Lynk Dev"

function Find-SdkTool([string] $Name) {
  $tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\$Name" -ErrorAction SilentlyContinue |
    Sort-Object { [version] $_.Directory.Parent.Name } -Descending |
    Select-Object -First 1
  if (-not $tool) { throw "$Name introuvable : installez le SDK Windows 11." }
  $tool.FullName
}

# Version du workspace (`[workspace.package]` de Cargo.toml) : la seule à tenir à jour.
function Get-LynkVersion {
  $toml = Get-Content (Join-Path $Root "Cargo.toml") -Raw
  if ($toml -notmatch '(?ms)^\[workspace\.package\].*?^version\s*=\s*"(\d+\.\d+\.\d+)"') {
    throw "Version introuvable dans Cargo.toml ([workspace.package])."
  }
  $Matches[1]
}

function Invoke-Tool([string] $Exe, [string[]] $Arguments) {
  $output = & $Exe @Arguments 2>&1
  if ($LASTEXITCODE -ne 0) {
    $output | Write-Host
    throw "$(Split-Path -Leaf $Exe) a échoué (code $LASTEXITCODE)."
  }
}

# Certificat de signature. Pour une diffusion publique, définir LYNK_CERT_THUMBPRINT
# avec l'empreinte d'un vrai certificat de signature de code (son sujet doit alors
# remplacer Publisher dans AppxManifest.xml).
function Get-LynkCertificate {
  if ($env:LYNK_CERT_THUMBPRINT) {
    $cert = Get-ChildItem Cert:\CurrentUser\My, Cert:\LocalMachine\My |
      Where-Object Thumbprint -eq $env:LYNK_CERT_THUMBPRINT | Select-Object -First 1
    if (-not $cert) { throw "Certificat $env:LYNK_CERT_THUMBPRINT introuvable." }
    return $cert
  }
  $cert = Get-ChildItem Cert:\CurrentUser\My |
    Where-Object { $_.Subject -eq $Publisher -and $_.HasPrivateKey -and $_.NotAfter -gt (Get-Date) } |
    Select-Object -First 1
  if (-not $cert) {
    Write-Host "Création du certificat de développement $Publisher…"
    $cert = New-SelfSignedCertificate -Type Custom -Subject $Publisher `
      -KeyUsage DigitalSignature -FriendlyName "Lynk (développement)" `
      -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(5) `
      -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
  }
  $cert
}

# Windows refuse un package signé par un inconnu : le certificat auto-signé doit
# figurer dans les « Personnes autorisées » de la machine. Une élévation, une fois.
function Confirm-LynkTrust($Cert) {
  if (Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object Thumbprint -eq $Cert.Thumbprint) { return }
  $cer = Join-Path $env:TEMP "lynk-dev.cer"
  Export-Certificate -Cert $Cert -FilePath $cer | Out-Null
  Write-Host "Approbation du certificat (fenêtre UAC)…"
  $import = "Import-Certificate -FilePath '$cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null"
  $proc = Start-Process powershell -Verb RunAs -Wait -PassThru -ArgumentList "-NoProfile", "-Command", $import
  Remove-Item $cer -ErrorAction SilentlyContinue
  if ($proc.ExitCode -ne 0) { throw "Le certificat n'a pas été approuvé." }
}

# Empaquette le manifeste et les logos, signe, et rend le chemin du .msix.
function New-LynkMsix($Cert) {
  $stage = Join-Path $Root "target\msix"
  Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory (Join-Path $stage "Assets") -Force | Out-Null
  # Version et éditeur ne s'écrivent pas à la main : celle du workspace, et le sujet
  # du certificat (Windows refuse un package dont l'éditeur diffère du signataire).
  $manifest = Get-Content (Join-Path $PSScriptRoot "AppxManifest.xml") -Raw -Encoding utf8
  $publisher = [System.Security.SecurityElement]::Escape($Cert.Subject)
  $manifest = $manifest -replace '(<Identity [^>]*?)Publisher="[^"]*"', "`$1Publisher=`"$publisher`""
  $manifest = $manifest -replace '(<Identity [^>]*?)Version="[^"]*"', "`$1Version=`"$(Get-LynkVersion).0`""
  [System.IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest, [System.Text.UTF8Encoding]::new($false))
  $icons = Join-Path $Root "app\src-tauri\icons"
  Copy-Item "$icons\Square150x150Logo.png", "$icons\Square44x44Logo.png", "$icons\StoreLogo.png" (Join-Path $stage "Assets")

  $msix = Join-Path $Root "target\Lynk.msix"
  Write-Host "Empaquetage et signature de $msix…"
  Invoke-Tool (Find-SdkTool "makeappx.exe") @("pack", "/o", "/nv", "/d", $stage, "/p", $msix)
  $store = if ($Cert.PSParentPath -like "*LocalMachine*") { @("/sm", "/s", "My") } else { @("/s", "My") }
  Invoke-Tool (Find-SdkTool "signtool.exe") (@("sign", "/fd", "SHA256", "/sha1", $Cert.Thumbprint) + $store + @($msix))
  $msix
}
