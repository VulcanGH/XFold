param(
    [Parameter(Mandatory)][string]$ArchiveUri,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][string]$Destination,
    [string]$LocalArchive
)

$ErrorActionPreference = 'Stop'
$archive = [IO.Path]::Combine($PSScriptRoot, 'mkvtoolnix.7z')
$tar = [IO.Path]::Combine($env:SystemRoot, 'System32\tar.exe')
$curl = [IO.Path]::Combine($env:SystemRoot, 'System32\curl.exe')
$failureCode = 11

try {
    if ([IO.File]::Exists($LocalArchive)) {
        [IO.File]::Copy($LocalArchive, $archive, $true)
    } else {
        $curlOptions = @('--fail', '--location', '--silent', '--show-error', '--proto', '=https',
            '--proto-redir', '=https', '--retry', '2', '--retry-max-time', '160', '--max-time', '80',
            '--header', 'Cache-Control: no-cache', '--output', $archive, $ArchiveUri)
        & $curl @curlOptions
        if ($LASTEXITCODE -ne 0) { throw "The official MKVToolNix download failed (curl exit $LASTEXITCODE)." }
    }
    # Setup can inherit a PowerShell 7 module path from its caller. Keep hashing
    # independent of module autoloading in its Windows PowerShell child.
    $failureCode = 15
    $algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $stream = [IO.File]::OpenRead($archive)
        try {
            $actual = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '')
        } finally {
            $stream.Dispose()
        }
    } finally {
        $algorithm.Dispose()
    }
    if ($actual -ne $Sha256) {
        $failureCode = 12
        throw 'The MKVToolNix archive failed its SHA-256 check.'
    }
    $failureCode = 13
    [void][IO.Directory]::CreateDirectory($Destination)
    & $tar -xf $archive -C $Destination
    if ($LASTEXITCODE -ne 0) { throw 'Windows could not extract the MKVToolNix archive.' }
    foreach ($name in @('mkvmerge.exe', 'mkvextract.exe')) {
        if (![IO.File]::Exists([IO.Path]::Combine($Destination, "mkvtoolnix\$name"))) {
            throw "The MKVToolNix download is missing $name."
        }
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit $failureCode
}
