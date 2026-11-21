# Gets the adb Oculibre ships on Windows (#17), at build time: Google's Android SDK Platform-Tools,
# a pinned version checked by its SHA-256, then only adb and its two USB DLLs, with Google's NOTICE
# and our licence note. Into desktop\target\adb\ (not in git); tauri.windows.conf.json installs
# that folder as adb\ next to Oculibre.
#
# Why we may ship it: adb is AOSP code under Apache-2.0 (the SDK terms, section 3.5, leave open
# source components to their own licences; Google's NOTICE says Apache-2.0); adb.exe contains
# libusb, LGPL-2.1, which allows it as a separate program, with its licence and a pointer to its
# source. A temporary way (USB debugging): Android Open Accessory (#21) will replace it.
$ErrorActionPreference = "Stop"
$Version = "37.0.1"
$Sha256 = "45f4d63113e895ebde0c90f194099a4676b6ac653bd28d54314a9e022bbc1a99"
$Url = "https://dl.google.com/android/repository/platform-tools_r$Version-win.zip"

$Target = Join-Path $PSScriptRoot "..\..\..\target\adb"
$Stamp = Join-Path $Target "version.txt"
if ((Test-Path $Stamp) -and ((Get-Content $Stamp) -eq $Version)) { exit 0 }

$Work = Join-Path $env:TEMP "oculibre-adb"
Remove-Item -Recurse -Force $Work -ErrorAction SilentlyContinue
New-Item -ItemType Directory $Work | Out-Null
$Zip = Join-Path $Work "platform-tools.zip"
& curl.exe --fail --silent --show-error --location --output $Zip $Url
if ($LASTEXITCODE -ne 0) { throw "cannot download $Url" }
$Got = (Get-FileHash -Algorithm SHA256 $Zip).Hash.ToLower()
if ($Got -ne $Sha256) { throw "platform-tools ${Version}: SHA-256 $Got, expected $Sha256" }
& tar.exe -xf $Zip -C $Work
if ($LASTEXITCODE -ne 0) { throw "cannot extract $Zip" }

Remove-Item -Recurse -Force $Target -ErrorAction SilentlyContinue
New-Item -ItemType Directory $Target | Out-Null
foreach ($f in "adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll", "NOTICE.txt") {
    Copy-Item (Join-Path $Work "platform-tools\$f") $Target
}
Copy-Item (Join-Path $PSScriptRoot "adb-licences.txt") $Target
Set-Content $Stamp $Version
Remove-Item -Recurse -Force $Work
Write-Host "adb $Version ready in $Target"
