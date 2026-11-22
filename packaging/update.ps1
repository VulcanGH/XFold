param(
    [Parameter(Mandatory)][ValidateSet('Check', 'Prepare', 'Apply')][string]$Mode,
    [Parameter(Mandatory)][string]$Repository,
    [Parameter(Mandatory)][string]$Current,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$Certificate,
    [Parameter(Mandatory)][string]$Application,
    [string]$Target,
    [int]$WaitFor,
    [string]$StateDirectory
)

$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$versionPattern = '^(0|[1-9]\d{0,4})\.(0|[1-9]\d{0,4})\.(0|[1-9]\d{0,4})\z'

function Assert-Installed {
    # Setup is a 32-bit NSIS process, even though the installed app is x64.
    $registry = [Microsoft.Win32.RegistryKey]::OpenBaseKey('LocalMachine', 'Registry32')
    try {
        $key = $registry.OpenSubKey('Software\XFold')
        if (!$key) { throw 'Install the current XFold Setup to enable in-app updates.' }
        try { $root = $key.GetValue('InstallDir') }
        finally { $key.Dispose() }
    } finally {
        $registry.Dispose()
    }
    if (!$root) { throw 'The machine-wide XFold installation path is missing.' }
    $expected = [IO.Path]::GetFullPath((Join-Path $root 'app\xfold.exe'))
    if ([IO.Path]::GetFullPath($Application) -ne $expected) {
        throw 'This instance does not match the registered XFold installation.'
    }
    if ((& $Application --version) -ne "xfold $Current" -or $LASTEXITCODE -ne 0) {
        throw 'The registered XFold version changed. Open XFold again before updating.'
    }
}

function Get-Release {
    $uri = "https://api.github.com/repos/$Repository/releases/latest"
    try {
        $release = Invoke-RestMethod -Uri $uri -TimeoutSec 15 -UseBasicParsing -Headers @{
            Accept = 'application/vnd.github+json'
            'User-Agent' = "XFold/$Current"
        }
    } catch {
        if ($_.Exception.Response.StatusCode -eq [Net.HttpStatusCode]::NotFound) { return }
        throw 'XFold could not check GitHub Releases. Try again after checking your connection.'
    }
    if ($release.draft -or $release.prerelease -or $release.tag_name -cnotmatch '^v') { return }
    $version = $release.tag_name.Substring(1)
    if ($version -notmatch $versionPattern -or [version]$version -le [version]$Current) { return }
    $name = "XFold-$version-windows-x64-setup.exe"
    $url = "https://github.com/$Repository/releases/download/v$version/$name"
    $assets = @($release.assets | Where-Object {
        $_.name -ceq $name -and $_.browser_download_url -ceq $url -and
        $_.digest -cmatch '^sha256:[0-9a-f]{64}$' -and $_.size -gt 0 -and $_.size -le 536870912
    })
    if ($assets.Count -eq 1) {
        [pscustomobject]@{ Version = $version; Url = $url; Size = $assets[0].size; Hash = $assets[0].digest.Substring(7) }
    }
}

function Assert-TrustedInstaller([string]$Path) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne 'Valid' -or !$signature.SignerCertificate) {
        throw 'The XFold installer has no valid Windows signature.'
    }
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $pin = [BitConverter]::ToString($sha256.ComputeHash($signature.SignerCertificate.RawData)).Replace('-', '')
    } finally {
        $sha256.Dispose()
    }
    if ($pin -ne $Certificate) { throw 'The installer was signed by an unexpected publisher.' }
    $metadata = (Get-Item -LiteralPath $Path).VersionInfo
    if ($metadata.ProductName -cne 'XFold' -or $metadata.FileVersion -cne $Target) {
        throw 'The signed installer does not match the selected XFold version.'
    }
}

try {
    if ($Mode -eq 'Check') {
        $release = Get-Release
        if ($release) { Write-Output $release.Version }
        exit 0
    }
    if ($Target -notmatch $versionPattern -or [version]$Target -le [version]$Current) {
        throw 'Choose a newer XFold version.'
    }
    $installer = Join-Path $PSScriptRoot "XFold-$Target-windows-x64-setup.exe"
    if ($Mode -eq 'Prepare') {
        Assert-Installed
        $release = Get-Release
        if (!$release -or $release.Version -cne $Target) {
            throw 'The requested release is no longer available. Check for updates again.'
        }
        Invoke-WebRequest -UseBasicParsing -Uri $release.Url -TimeoutSec 180 -MaximumRedirection 5 -OutFile $installer
        if ((Get-Item -LiteralPath $installer).Length -ne $release.Size -or
            (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $release.Hash) {
            throw 'The downloaded installer does not match the GitHub release.'
        }
        Assert-TrustedInstaller $installer
        exit 0
    }

    if ($WaitFor -le 0 -or !$StateDirectory) { throw 'Update restart information is missing.' }
    $runningApp = Get-Process -Id $WaitFor -ErrorAction SilentlyContinue
    if ($runningApp -and !$runningApp.WaitForExit(30000)) {
        throw 'The previous XFold process did not exit. The update was postponed.'
    }
    $resultFile = Join-Path $StateDirectory 'update-result.txt'
    try {
        Assert-Installed
        Assert-TrustedInstaller $installer
        $install = Start-Process -FilePath $installer -ArgumentList '/S' -Verb RunAs -PassThru -Wait
        if ($install.ExitCode -ne 0 -or (& $Application --version) -ne "xfold $Target") {
            throw 'Windows Setup could not finish the update. Close any other XFold instances and try again.'
        }
        Set-Content -LiteralPath $resultFile -Value "Updated to XFold $Target." -Encoding UTF8
    } catch {
        Set-Content -LiteralPath $resultFile -Value $_.Exception.Message -Encoding UTF8
    } finally {
        if (Test-Path -LiteralPath $Application -PathType Leaf) {
            $escapedState = $StateDirectory -replace '(\\+)$', '$1$1'
            Start-Process -FilePath $Application -ArgumentList ('--state-dir "' + $escapedState + '"') -WindowStyle Hidden
        }
        Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $PSCommandPath -ErrorAction SilentlyContinue
        [IO.Directory]::Delete($PSScriptRoot)
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
