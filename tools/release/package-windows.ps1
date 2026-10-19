# Builds the Windows release artefacts for one target into target\dist\:
#   Kerf-<version>-windows-<arch>.msi and .zip (portable)
#
#   pwsh tools/release/package-windows.ps1 <version> [-Target x86_64-pc-windows-msvc|aarch64-pc-windows-msvc]
#
# Needs cargo-wix (`cargo install cargo-wix`). The WiX Toolset 3.14 binaries are fetched into target\ the
# first time, pinned by checksum. With WINDOWS_CERT_PFX (base64) and WINDOWS_CERT_PASSWORD set, kerf.exe
# and the .msi are signed with signtool; without them target\dist\unsigned.txt names both artefacts.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Target = 'x86_64-pc-windows-msvc'
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
Set-Location $root
$arch = @{ 'x86_64-pc-windows-msvc' = 'x64'; 'aarch64-pc-windows-msvc' = 'arm64' }[$Target]
if (-not $arch) { throw "No Windows package for $Target" }
$dist = Join-Path $root 'target\dist'
New-Item -ItemType Directory -Force $dist | Out-Null
$name = "Kerf-$Version-windows-$arch"

cargo build --release --locked --target $Target
$exe = "target\$Target\release\kerf.exe"

$signing = [bool]$env:WINDOWS_CERT_PFX
function Sign-File([string]$path) {
    $pfx = Join-Path $env:RUNNER_TEMP 'kerf.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERT_PFX))
    $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
    & $signtool.FullName sign /f $pfx /p $env:WINDOWS_CERT_PASSWORD /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $path
    Remove-Item $pfx
}
if ($signing) { Sign-File $exe }

$zip = Join-Path $dist "$name.zip"
$stage = Join-Path $root "target\windows-package\$name"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe $stage
Compress-Archive -Force -Path $stage -DestinationPath $zip

# cargo-wix drives the WiX 3 compiler and linker; WiX 4 and later are not compatible with it.
$wix = Join-Path $root 'target\wix314'
if (-not (Test-Path "$wix\candle.exe")) {
    $download = Join-Path $root 'target\wix314-binaries.zip'
    Invoke-WebRequest -Uri 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip' -OutFile $download
    $sum = (Get-FileHash -Algorithm SHA256 $download).Hash
    if ($sum -ne '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31') { throw "WiX download checksum mismatch: $sum" }
    Expand-Archive -Force $download $wix
}
$msi = Join-Path $dist "$name.msi"
cargo wix --no-build --nocapture --target $Target --install-version $Version --bin-path $wix --output $msi
if ($signing) { Sign-File $msi }
if (-not $signing) { "$name.msi", "$name.zip" | Add-Content (Join-Path $dist 'unsigned.txt') }

Get-ChildItem $dist
