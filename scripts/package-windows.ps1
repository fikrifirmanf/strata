# Builds strata.exe (icon embedded by build.rs), then under dist\:
#   Strata-<version>-windows-x86_64-setup.exe  installer (needs Inno Setup 6: choco install innosetup)
#   strata-<version>-windows-x86_64.zip        portable copy
param([Parameter(Mandatory = $true)][string]$Version)
$ErrorActionPreference = "Stop"

cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Remove-Item -Recurse -Force dist -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force dist | Out-Null

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source,
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw "Inno Setup 6 not found; install it with: choco install innosetup" }
$fileVersion = ($Version -split '[-+]')[0]
& $iscc /Q "/DAppVersion=$Version" "/DFileVersion=$fileVersion" scripts\strata.iss
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$name = "strata-$Version-windows-x86_64"
New-Item -ItemType Directory -Force "dist\$name" | Out-Null
Copy-Item target\release\strata.exe, README.md, LICENSE, THIRD_PARTY_NOTICES.md "dist\$name\"
New-Item -ItemType Directory -Force "dist\$name\licenses" | Out-Null
Copy-Item assets\fonts\OFL.txt "dist\$name\licenses\FONTS-OFL.txt"
Copy-Item assets\db\README.md "dist\$name\licenses\DATABASE-LOGOS.md"
Compress-Archive -Path "dist\$name\*" -DestinationPath "dist\$name.zip"
Remove-Item -Recurse -Force "dist\$name"
Get-ChildItem dist
