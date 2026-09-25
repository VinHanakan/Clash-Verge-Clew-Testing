$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$winDivertUrl = 'https://reqrypt.org/download/WinDivert-2.2.2-A.zip'
$winDivertHash = '63CB41763BB4B20F600B6DE04E991A9C2BE73279E317D4D82F237B150C5F3F15'
$archive = Join-Path $env:RUNNER_TEMP 'WinDivert-2.2.2-A.zip'
Invoke-WebRequest -Uri $winDivertUrl -OutFile $archive
if ((Get-FileHash -Algorithm SHA256 $archive).Hash -ne $winDivertHash) {
  throw 'WinDivert download SHA-256 mismatch'
}
$extracted = Join-Path $env:RUNNER_TEMP 'windivert-extracted'
Expand-Archive -LiteralPath $archive -DestinationPath $extracted
$driverSource = Join-Path $extracted 'WinDivert-2.2.2-A/x64'
$driverDestination = 'vendor/clew/WinDivert-2.2.2-A/x64'
New-Item -ItemType Directory -Path $driverDestination -Force | Out-Null
foreach ($name in @('WinDivert.lib', 'WinDivert.dll', 'WinDivert64.sys')) {
  Copy-Item -LiteralPath (Join-Path $driverSource $name) -Destination (Join-Path $driverDestination $name)
}

$vcpkgRoot = Join-Path $env:RUNNER_TEMP 'clew-vcpkg'
git clone --filter=blob:none https://github.com/microsoft/vcpkg.git $vcpkgRoot
if ($LASTEXITCODE -ne 0) { throw 'vcpkg clone failed' }
git -C $vcpkgRoot checkout 93c50752b23e350ca6b9063a167f0a4cf8a3b3eb
if ($LASTEXITCODE -ne 0) { throw 'vcpkg checkout failed' }
& (Join-Path $vcpkgRoot 'bootstrap-vcpkg.bat') -disableMetrics
if ($LASTEXITCODE -ne 0) { throw 'vcpkg bootstrap failed' }
& (Join-Path $vcpkgRoot 'vcpkg.exe') install quill nlohmann-json cpp-httplib asio --triplet x64-windows
if ($LASTEXITCODE -ne 0) { throw 'vcpkg install failed' }

$toolchain = Join-Path $vcpkgRoot 'scripts/buildsystems/vcpkg.cmake'
cmake -S vendor/clew -B vendor/clew/build -G 'Visual Studio 17 2022' -A x64 "-DCMAKE_TOOLCHAIN_FILE=$toolchain" -DVCPKG_TARGET_TRIPLET=x64-windows -DCLEW_ENABLE_WEBVIEW2=OFF
if ($LASTEXITCODE -ne 0) { throw 'Clew CMake configure failed' }
cmake --build vendor/clew/build --config Release --parallel 4
if ($LASTEXITCODE -ne 0) { throw 'Clew build failed' }
$helperDirectory = 'vendor/clew/build/Release'

Get-ChildItem -LiteralPath (Join-Path $vcpkgRoot 'installed/x64-windows/bin') -Filter '*.dll' | ForEach-Object {
  Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $helperDirectory $_.Name) -Force
}
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vsPath = & $vswhere -latest -products '*' -property installationPath
if (-not $vsPath) { throw 'Visual Studio installation not found' }
$crtDirectory = Get-ChildItem -Path (Join-Path $vsPath 'VC/Redist/MSVC') -Directory -Recurse |
  Where-Object { $_.Name -eq 'Microsoft.VC143.CRT' -and $_.FullName -match '[\\/]x64[\\/]' } |
  Sort-Object FullName -Descending | Select-Object -First 1
if (-not $crtDirectory) { throw 'VC143 CRT directory not found' }
foreach ($name in @('msvcp140.dll', 'msvcp140_1.dll', 'msvcp140_2.dll', 'msvcp140_atomic_wait.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')) {
  $source = Join-Path $crtDirectory.FullName $name
  if (-not (Test-Path -LiteralPath $source)) { throw "Missing VC143 runtime: $name" }
  Copy-Item -LiteralPath $source -Destination (Join-Path $helperDirectory $name)
}

$alphaVersionUrl = 'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt'
$alphaVersion = [Text.Encoding]::UTF8.GetString((Invoke-WebRequest -Uri $alphaVersionUrl).Content).Trim()
if ($alphaVersion -notmatch '^alpha-[a-f0-9]+$') { throw "Unexpected Mihomo Alpha version: $alphaVersion" }
$env:CLEW_TEST_MIHOMO_ALPHA_VERSION = $alphaVersion
"CLEW_TEST_MIHOMO_ALPHA_VERSION=$alphaVersion" | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
node scripts/prebuild.mjs x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Mihomo/resource prebuild failed' }

$requiredFiles = @(
  'vendor/clew/build/Release/clew.exe',
  'vendor/clew/build/Release/WinDivert.dll',
  'vendor/clew/build/Release/WinDivert64.sys',
  'vendor/clew/build/Release/brotlienc.dll',
  'vendor/clew/build/Release/brotlidec.dll',
  'vendor/clew/build/Release/brotlicommon.dll',
  'src-tauri/sidecar/verge-mihomo-x86_64-pc-windows-msvc.exe',
  'src-tauri/sidecar/verge-mihomo-alpha-x86_64-pc-windows-msvc.exe'
)
foreach ($file in $requiredFiles) {
  if (-not (Test-Path -LiteralPath $file)) { throw "Missing build input: $file" }
}
