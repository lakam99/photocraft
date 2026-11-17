param(
    [Parameter(Mandatory)][string]$BinaryDirectory,
    [Parameter(Mandatory)][string]$ChatPluginDirectory,
    [Parameter(Mandatory)][string]$TranslationDirectory,
    [Parameter(Mandatory)][string]$FontDirectory,
    [Parameter(Mandatory)][string]$Version,
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$binaryRoot = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$pluginRoot = (Resolve-Path -LiteralPath $ChatPluginDirectory).Path
$translationRoot = (Resolve-Path -LiteralPath $TranslationDirectory).Path
$translationCommit = (Get-Content -LiteralPath (Join-Path $projectRoot 'craft-i18n.rev') -Raw).Trim()
if ($translationCommit -notmatch '^[a-f0-9]{40}$' -or -not (Test-Path -LiteralPath (Join-Path $translationRoot 'locales'))) { throw 'Expected the pinned translation build input' }
# Archived build inputs carry their source revision; a checkout must be at that same revision.
if (Test-Path -LiteralPath (Join-Path $translationRoot '.git')) {
    $actualTranslationCommit = & git -C $translationRoot rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify translation source' }
} else {
    $actualTranslationCommit = (Get-Content -LiteralPath (Join-Path $translationRoot 'SOURCE-COMMIT.txt') -Raw).Trim()
}
if ($actualTranslationCommit -ne $translationCommit) { throw 'Translation notices must match craft-i18n.rev' }
$fontRoot = (Resolve-Path -LiteralPath $FontDirectory).Path
$fontCommit = (Get-Content -LiteralPath (Join-Path $projectRoot 'craft-fonts.rev') -Raw).Trim()
if ($fontCommit -notmatch '^[a-f0-9]{40}$') { throw 'Invalid pinned font revision' }
if (Test-Path -LiteralPath (Join-Path $fontRoot '.git')) {
    $actualFontCommit = & git -C $fontRoot rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Cannot identify font source' }
} else {
    $actualFontCommit = (Get-Content -LiteralPath (Join-Path $fontRoot 'SOURCE-COMMIT.txt') -Raw).Trim()
}
if ($actualFontCommit -ne $fontCommit) { throw 'Font notices must match craft-fonts.rev' }
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Expected a stable release version' }
$inPackage = $false
$sourceVersion = $null
foreach ($line in Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPackage = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $sourceVersion = $Matches[1]; break }
}
if ($sourceVersion -ne $Version) { throw 'Source workspace version mismatch' }
& git -C $projectRoot diff --quiet HEAD --
if ($LASTEXITCODE -ne 0) { throw 'Commit the verified source before packaging' }
$sourceCommit = & git -C $projectRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot identify release source' }
$pluginCommit = '41edaa0f7de0e153b5747d217a913011491a5bf2'
# Keep bundled notices aligned with the immutable plugin actually resolved by this source.
$pluginSource = "git+https://github.com/lakam99/craft-ai-chat?rev=$pluginCommit#$pluginCommit"
if (-not (Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.lock') -Raw).Contains($pluginSource)) {
    throw 'Update the pinned plugin notices to match Cargo.lock before packaging'
}
# A transitive version change can require different notices even when the plugin pin is unchanged.
$lockText = (Get-Content -LiteralPath (Join-Path $projectRoot 'Cargo.lock') -Raw).Replace("`r`n", "`n")
$algorithm = [Security.Cryptography.SHA256]::Create()
try { $lockHash = ([BitConverter]::ToString($algorithm.ComputeHash([Text.Encoding]::UTF8.GetBytes($lockText)))).Replace('-', '').ToLowerInvariant() }
finally { $algorithm.Dispose() }
$notices = Get-Content -LiteralPath (Join-Path $projectRoot 'third_party/AI-CHAT-DEPENDENCIES.txt') -Raw
if (-not $notices.Contains("Cargo.lock SHA256 (UTF-8, LF): $lockHash")) { throw 'Refresh AI Chat dependency notices after changing Cargo.lock' }
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $projectRoot "dist/fork-$Version" }
$releaseRoot = [IO.Path]::GetFullPath($OutputDirectory)
$zip = Join-Path $releaseRoot "photocraft-$Version-windows-x64-portable.zip"
$sums = Join-Path $releaseRoot 'SHA256SUMS.txt'
foreach ($file in @($zip, $sums)) {
    if (Test-Path -LiteralPath $file) { throw "Release output already exists: $file" }
}
$stage = Join-Path $releaseRoot ('stage-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($stage) | Out-Null
foreach ($program in @(@('photocraft.exe', 2), @('photocraft-cli.exe', 3))) {
    $file = Join-Path $binaryRoot $program[0]
    $bytes = [IO.File]::ReadAllBytes($file)
    if ($bytes.Length -lt 128 -or $bytes[0] -ne 77 -or $bytes[1] -ne 90) { throw 'Invalid Windows executable' }
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($pe -lt 0 -or $pe -gt $bytes.Length - 96) { throw 'Invalid PE header offset' }
    if ([BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664 -or [BitConverter]::ToUInt16($bytes, $pe + 0x5c) -ne $program[1]) { throw 'Wrong executable architecture or subsystem' }
    Copy-Item -LiteralPath $file -Destination $stage
}
$guiVersion = (Get-Item -LiteralPath (Join-Path $stage 'photocraft.exe')).VersionInfo
if ($guiVersion.ProductVersion -ne $Version) { throw 'Desktop resource version mismatch' }
$cliVersion = & (Join-Path $stage 'photocraft-cli.exe') --version
if ($LASTEXITCODE -ne 0 -or $cliVersion -notmatch [regex]::Escape($Version) -or $cliVersion -notmatch [regex]::Escape($sourceCommit.Substring(0, 9))) { throw 'CLI version or build commit mismatch' }
foreach ($file in @('README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md', 'craft-i18n.rev', 'craft-fonts.rev')) {
    Copy-Item -LiteralPath (Join-Path $projectRoot $file) -Destination $stage
}
[IO.Directory]::CreateDirectory((Join-Path $stage 'docs')) | Out-Null
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/ai-chat.md') -Destination (Join-Path $stage 'docs')
# Extract notices from the immutable plugin revision linked by this build, not its working tree.
$archive = Join-Path $releaseRoot ('plugin-notices-' + [guid]::NewGuid().ToString('N') + '.zip')
& git -C $pluginRoot archive --format=zip '--prefix=licenses/craft-ai-chat/' "--output=$archive" $pluginCommit LICENSE-MIT LICENSE-APACHE NOTICE README.md third_party
if ($LASTEXITCODE -ne 0) { throw 'Cannot extract pinned plugin notices' }
[IO.Compression.ZipFile]::ExtractToDirectory($archive, $stage)
$translationNotices = Join-Path $stage 'licenses/craft-i18n'
[IO.Directory]::CreateDirectory($translationNotices) | Out-Null
foreach ($file in @('LICENSE-MIT', 'LICENSE-APACHE', 'ATTRIBUTION.md')) {
    Copy-Item -LiteralPath (Join-Path $translationRoot $file) -Destination $translationNotices
}
$fontNotices = Join-Path $stage 'licenses/craft-fonts'
[IO.Directory]::CreateDirectory($fontNotices) | Out-Null
Copy-Item -LiteralPath (Join-Path $fontRoot 'ATTRIBUTION.md') -Destination $fontNotices
foreach ($line in Get-Content -LiteralPath (Join-Path $fontRoot 'fonts/manifest.txt')) {
    if (-not $line.Trim() -or $line.Trim().StartsWith('#')) { continue }
    $fields = $line.Split(' | ')
    if ($fields.Count -ne 8 -or $fields[4] -ne 'OFL-1.1') { throw 'Expected reviewed OFL font inputs' }
    $fontFile = Join-Path $fontRoot $fields[2]
    if ((Get-FileHash -LiteralPath $fontFile -Algorithm SHA256).Hash.ToLowerInvariant() -ne $fields[6]) { throw 'Font input differs from its manifest' }
    $noticeName = 'OFL-' + [IO.Path]::GetFileName([IO.Path]::GetDirectoryName($fields[5])) + '.txt'
    Copy-Item -LiteralPath (Join-Path $fontRoot $fields[5]) -Destination (Join-Path $fontNotices $noticeName)
}
foreach ($relative in @('assets/fonts', 'assets/icons', 'assets/dict', 'assets/app-icon', 'crates/algo/data', 'third_party')) {
    $folder = Join-Path $projectRoot $relative
    if (Test-Path -LiteralPath $folder) {
        $destination = Join-Path $stage "licenses/$relative"
        [IO.Directory]::CreateDirectory($destination) | Out-Null
        Get-ChildItem -LiteralPath $folder -File -Filter '*.txt' | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $destination }
    }
}
[IO.File]::WriteAllText((Join-Path $stage 'portable.txt'), "PhotoCraft portable; retain PhotoCraftData when upgrading.`n", [Text.UTF8Encoding]::new($false))
[IO.File]::WriteAllText((Join-Path $stage 'BUILD.txt'), "PhotoCraft fork $Version`nSource: https://github.com/lakam99/photocraft/commit/$sourceCommit`nAI plugin: $pluginCommit`nTranslations: https://github.com/storytold/craft-i18n/commit/$translationCommit`nFonts: https://github.com/storytold/craft-fonts/commit/$fontCommit`nWindows x64 portable; unsigned.`n", [Text.UTF8Encoding]::new($false))
[IO.Compression.ZipFile]::CreateFromDirectory($stage, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($sums, "$hash  $([IO.Path]::GetFileName($zip))`n", [Text.UTF8Encoding]::new($false))
Get-Item -LiteralPath $zip, $sums | Select-Object Name, Length
