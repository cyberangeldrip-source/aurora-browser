param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Notes,
    [Parameter(Mandatory)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
    throw 'Укажите стабильную версию Aurora.'
}
$exe = Get-Item -LiteralPath $Binary
if ($exe.Name -cne 'aurora-browser.exe' -or $exe.VersionInfo.ProductVersion -cne $Version) {
    throw 'Имя или версия EXE не совпадают с выпуском.'
}
$summary = (Get-Content -LiteralPath $Notes -Raw -Encoding utf8).Trim()
if (-not $summary -or $summary.Length -gt 3500) {
    throw 'Длина описания должна быть от 1 до 3500 символов.'
}
$keyDirectory = Join-Path $env:LOCALAPPDATA 'AuroraReleaseKeys'
$privateKey = Join-Path $keyDirectory 'releases.key'
$protectedPassword = Join-Path $keyDirectory 'releases.password.dpapi'
$publicKey = Join-Path $PSScriptRoot '../src/update/public.key'
if ((Get-FileHash -LiteralPath "$privateKey.pub").Hash -ne (Get-FileHash -LiteralPath $publicKey).Hash) {
    throw 'Ключ подписи отличается от ключа, встроенного в Aurora.'
}

$directory = New-Item -ItemType Directory -Path $OutputDirectory -Force
$manifest = Join-Path $directory.FullName 'aurora-update.json'
[ordered]@{
    version = $Version
    platform = 'windows-x86_64'
    notes = $summary
    bytes = $exe.Length
    sha256 = (Get-FileHash -LiteralPath $exe.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
} | ConvertTo-Json -Compress | Set-Content -LiteralPath $manifest -Encoding utf8NoBOM -NoNewline

$previousPassword = $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD
try {
    $secure = ConvertTo-SecureString (Get-Content -LiteralPath $protectedPassword -Raw)
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ConvertFrom-SecureString $secure -AsPlainText
    & cargo tauri signer sign --private-key-path $privateKey $manifest *> $null
    if ($LASTEXITCODE) {
        throw 'Tauri не смог подписать манифест.'
    }
} finally {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $previousPassword
}
Get-Item -LiteralPath $manifest, "$manifest.sig" | Select-Object FullName, Length
