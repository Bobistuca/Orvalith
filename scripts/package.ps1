# Empacota o Orvalith para Windows: build release travado, zip portátil e SHA-256.
# Uso (PowerShell, na raiz do repositório):  .\scripts\package.ps1
$ErrorActionPreference = 'Stop'
$env:PATH += ";$env:USERPROFILE\.cargo\bin"
Set-Location (Split-Path $PSScriptRoot -Parent)

cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'build falhou' }

$version = (Select-String -Path 'Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$name = "Orvalith-$version-windows-x64"
$dist = Join-Path 'dist' $name
if (Test-Path 'dist') { Remove-Item -Recurse -Force 'dist' }
New-Item -ItemType Directory -Force $dist | Out-Null

Copy-Item 'target\release\orvalith.exe' (Join-Path $dist 'Orvalith.exe')
foreach ($f in 'LICENSE', 'README.md', 'THIRD_PARTY_LICENSES.md', 'CHANGELOG.md') {
    Copy-Item $f $dist
}

$zip = "dist\$name.zip"
Compress-Archive -Path $dist -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Out-File "dist\SHA256SUMS.txt" -Encoding ascii
Write-Host "Pronto: $zip"
Write-Host "SHA-256: $hash"
