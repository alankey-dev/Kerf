# Builds the Windows release artefacts for one target into target\dist\, in two phases so that no signing
# secret is ever in the environment of `cargo build` (every build.rs and proc-macro in the graph can read it):
#   Kerf-<version>-windows-<arch>.msi and .zip (portable)
#
#   pwsh tools/release/package-windows.ps1 <version> [-Target x86_64-pc-windows-msvc|aarch64-pc-windows-msvc]
#       build phase: builds kerf.exe, then the unsigned .zip and .msi
#   pwsh tools/release/package-windows.ps1 <version> -Target <triple> -Sign
#       sign phase: signs the built kerf.exe, then rebuilds the .zip and .msi around it and signs the .msi
#
# Needs cargo-wix (`cargo install cargo-wix`). The WiX Toolset 3.14 binaries are fetched into target\ and
# checked against a pinned checksum every time. The build phase ignores the signing variables and leaves
# kerf.exe at target\sign-input\kerf.exe. The sign phase runs no `cargo build`: with WINDOWS_CERT_PFX
# (base64) and WINDOWS_CERT_PASSWORD set, it signs with signtool, and takes both out of the environment
# before cargo-wix and the WiX tools run. Without them target\dist\unsigned.txt names both artefacts.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [string]$Target = 'x86_64-pc-windows-msvc',
    [switch]$Sign
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
$exe = "target\$Target\release\kerf.exe"
$stash = Join-Path $root 'target\sign-input'

# Read the certificate, then take it out of the environment, so that nothing run afterwards can see it.
$pfxBase64 = $env:WINDOWS_CERT_PFX
$pfxPassword = $env:WINDOWS_CERT_PASSWORD
Remove-Item Env:WINDOWS_CERT_PFX, Env:WINDOWS_CERT_PASSWORD -ErrorAction SilentlyContinue

if ($Sign -and -not $pfxBase64) {
    Write-Output 'No signing secrets: keeping the unsigned build.'
    "$name.msi", "$name.zip" | Add-Content (Join-Path $dist 'unsigned.txt')
    exit 0
}

function Sign-File([string]$path) {
    $pfx = Join-Path $env:RUNNER_TEMP 'kerf.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($pfxBase64))
    try {
        $signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" | Sort-Object FullName | Select-Object -Last 1
        & $signtool.FullName sign /f $pfx /p $pfxPassword /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $path
        & $signtool.FullName verify /pa $path
    }
    finally {
        Remove-Item $pfx -ErrorAction SilentlyContinue
    }
}

# cargo-wix drives the WiX 3 compiler and linker; WiX 4 and later are not compatible with it. Download and
# check every time, so a directory left in target\ from an earlier run is never trusted.
function Install-Wix([string]$wix) {
    $download = Join-Path $root 'target\wix314-binaries.zip'
    Invoke-WebRequest -Uri 'https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip' -OutFile $download
    $sum = (Get-FileHash -Algorithm SHA256 $download).Hash
    if ($sum -ne '6AC824E1642D6F7277D0ED7EA09411A508F6116BA6FAE0AA5F2C7DAA2FF43D31') { throw "WiX download checksum mismatch: $sum" }
    Remove-Item -Recurse -Force $wix -ErrorAction SilentlyContinue
    Expand-Archive -Force $download $wix
}

if ($Sign) {
    # Sign the exe the build phase produced, and put it where `cargo wix --no-build` looks for it.
    New-Item -ItemType Directory -Force (Split-Path $exe) | Out-Null
    Copy-Item (Join-Path $stash 'kerf.exe') $exe -Force
    Sign-File $exe
    Remove-Item (Join-Path $dist "$name.zip"), (Join-Path $dist "$name.msi") -ErrorAction SilentlyContinue
}
else {
    cargo build --release --locked --target $Target
    New-Item -ItemType Directory -Force $stash | Out-Null
    Copy-Item $exe (Join-Path $stash 'kerf.exe') -Force
}

$zip = Join-Path $dist "$name.zip"
$stage = Join-Path $root "target\windows-package\$name"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe $stage
Compress-Archive -Force -Path $stage -DestinationPath $zip

$wix = Join-Path $root 'target\wix314'
Install-Wix $wix
$msi = Join-Path $dist "$name.msi"
cargo wix --no-build --nocapture --target $Target --install-version $Version --bin-path $wix --output $msi
if ($Sign) { Sign-File $msi }

Get-ChildItem $dist
