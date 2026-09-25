$ErrorActionPreference = 'Stop'
$bundleDirectory = 'target/debug/bundle/nsis'
$installer = Get-ChildItem -LiteralPath $bundleDirectory -File -Filter '*-setup.exe' | Select-Object -First 1
if (-not $installer) { throw 'NSIS installer was not created' }
$sourceCommit = (git rev-parse HEAD).Trim()
$shortCommit = $sourceCommit.Substring(0, 8)
$version = (Get-Content package.json -Raw | ConvertFrom-Json).version
$artifactName = "Clash-Verge-Clew_${version}_${shortCommit}_win-x64_unsigned"
$hashSources = @(
  $installer.FullName,
  'vendor/clew/build/Release/clew.exe',
  'vendor/clew/build/Release/WinDivert.dll',
  'vendor/clew/build/Release/WinDivert64.sys',
  'src-tauri/sidecar/verge-mihomo-x86_64-pc-windows-msvc.exe',
  'src-tauri/sidecar/verge-mihomo-alpha-x86_64-pc-windows-msvc.exe'
)
$hashes = foreach ($file in $hashSources) {
  $item = Get-Item -LiteralPath $file
  [ordered]@{ file = $file; bytes = $item.Length; sha256 = (Get-FileHash -Algorithm SHA256 $file).Hash.ToLowerInvariant() }
}
$hashes | ForEach-Object { "$($_.sha256)  $($_.file)" } |
  Set-Content -LiteralPath (Join-Path $bundleDirectory 'SHA256SUMS.txt') -Encoding utf8
$provenance = [ordered]@{
  integration_version = $version
  clash_verge_rev_base = '2.5.4'
  clew_import_baseline = 'v0.10.0 (vendored with integration modifications)'
  source_commit = $sourceCommit
  source_repository = $env:GITHUB_REPOSITORY
  target = 'Windows x64 / internal-test / unsigned'
  mihomo_stable = $env:CLEW_TEST_MIHOMO_STABLE_VERSION
  mihomo_alpha = $env:CLEW_TEST_MIHOMO_ALPHA_VERSION
  windivert = '2.2.2-A'
  files = @($hashes)
}
$provenance | ConvertTo-Json -Depth 6 |
  Set-Content -LiteralPath (Join-Path $bundleDirectory 'build-provenance.json') -Encoding utf8
"name=$artifactName" | Out-File -FilePath $env:GITHUB_OUTPUT -Append -Encoding utf8
