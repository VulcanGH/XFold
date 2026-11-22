param(
    [Parameter(Mandatory)][string]$ArchiveUri,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][string]$Destination,
    [string]$LocalArchive
)

$ErrorActionPreference = 'Stop'
$archive = Join-Path $PSScriptRoot 'mkvtoolnix.7z'
$tar = Join-Path $env:SystemRoot 'System32\tar.exe'
$curl = Join-Path $env:SystemRoot 'System32\curl.exe'
$failureCode = 11

try {
    if ($LocalArchive -and (Test-Path -LiteralPath $LocalArchive -PathType Leaf)) {
        Copy-Item -LiteralPath $LocalArchive -Destination $archive
    } else {
        $curlOptions = @('--fail', '--location', '--silent', '--show-error', '--proto', '=https',
            '--proto-redir', '=https', '--retry', '2', '--retry-max-time', '160', '--max-time', '80',
            '--header', 'Cache-Control: no-cache', '--output', $archive, $ArchiveUri)
        & $curl @curlOptions
        if ($LASTEXITCODE -ne 0) { throw "The official MKVToolNix download failed (curl exit $LASTEXITCODE)." }
    }
    $failureCode = 12
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash -ne $Sha256) {
        throw 'The MKVToolNix archive failed its SHA-256 check.'
    }
    $failureCode = 13
    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    & $tar -xf $archive -C $Destination
    if ($LASTEXITCODE -ne 0) { throw 'Windows could not extract the MKVToolNix archive.' }
    foreach ($name in @('mkvmerge.exe', 'mkvextract.exe')) {
        if (!(Test-Path -LiteralPath (Join-Path $Destination "mkvtoolnix\$name") -PathType Leaf)) {
            throw "The MKVToolNix download is missing $name."
        }
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit $failureCode
}
