param(
    [switch]$ProbeOnly,
    [string]$KernelPath,
    [Parameter(Mandatory)][string]$WixDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Installation is intentionally confined to disposable GitHub Windows runners.
if (!$IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or !$env:RUNNER_TEMP) {
    throw 'Actual MSI execution requires a disposable GitHub Windows runner.'
}
if (!$ProbeOnly) { throw 'Production Runtime guard acceptance is not implemented; probe evidence is insufficient.' }
$output = [IO.Path]::GetFullPath($OutputDirectory)
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (!$output.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase) -or $output.Contains('"')) {
    throw 'Probe output must be a child of RUNNER_TEMP.'
}
$repo = Split-Path $PSScriptRoot -Parent
if ($KernelPath) { $KernelPath = (Resolve-Path -LiteralPath $KernelPath).Path; if ($KernelPath.Contains('"')) { throw 'Invalid Kernel path.' } }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$dll = Join-Path $output 'probe.dll'
$payload = Join-Path $output 'sentinel.txt'
[IO.File]::WriteAllText($payload, 'Ridge isolated MSI lifecycle fixture')

function Invoke-Checked([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Executable failed: $LASTEXITCODE" }
}
Push-Location $output
try {
    Invoke-Checked 'cl.exe' @('/nologo', '/std:c++17', '/W4', '/WX', '/EHsc', '/MT', '/LD',
        (Join-Path $repo 'tests/fixtures/msi-runtime/probe.cpp'), '/link', '/OUT:probe.dll', 'msi.lib', 'advapi32.lib')
} finally { Pop-Location }

$installer = New-Object -ComObject WindowsInstaller.Installer
function Read-MsiRows([string]$Query, [int]$Columns) {
    $view = $database.OpenView($Query)
    [void]$view.Execute()
    try {
        while ($record = $view.Fetch()) {
            $row = @(); for ($column = 1; $column -le $Columns; $column++) { $row += $record.StringData($column) }
            ,$row
        }
    } finally { [void]$view.Close() }
}
function Invoke-ProbeMsi([string]$CaseRoot, [string]$Ui, [string]$Mode, [switch]$Uninstall) {
    $operation = if ($Uninstall) { '/x' } else { '/i' }
    $target = if ($Uninstall) { $product } else { '"' + $msi + '"' }
    $log = Join-Path $CaseRoot $(if ($Uninstall) { 'uninstall.log' } else { 'install.log' })
    $arguments = @($operation, $target, $Ui, '/norestart', 'REBOOT=ReallySuppress', '/l*v', ('"' + $log + '"'),
        ('RIDGE_PROBE_ROOT="' + $CaseRoot + '"'), ('PROBE_INSTALL="' + (Join-Path $CaseRoot 'installed') + '"'), "PROBE_MODE=$Mode")
    if ($KernelPath) { $arguments += 'RIDGE_PROBE_HELPER="' + $KernelPath + '"' }
    $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -WindowStyle Hidden -PassThru
    if (!$process.WaitForExit(120000)) { throw "MSI $Mode timed out; runner teardown owns cleanup." }
    $process.Refresh()
    return $process.ExitCode
}

function Assert-HelperReleased([string]$CaseRoot) {
    if (!$KernelPath) { return }
    $savedDataDir = $env:RIDGE_KERNEL_DATA_DIR
    try {
        $env:RIDGE_KERNEL_DATA_DIR = Join-Path $CaseRoot 'runtime'
        & $KernelPath --installer-check
        if ($LASTEXITCODE -ne 0) { throw 'Real Kernel installer reservation remains held after MSI completion.' }
    } finally { $env:RIDGE_KERNEL_DATA_DIR = $savedDataDir }
}

$results = @()
$packages = @()
foreach ($scope in @('perUser', 'perMachine')) {
    $product = if ($scope -eq 'perUser') { '{056D63CA-DA83-4458-8CBF-5D98C18F89BE}' } else { '{B6B6DC01-A158-4943-A3C3-DC0CA105A57F}' }
    $msi = Join-Path $output "$scope.msi"
    $object = Join-Path $output "$scope.wixobj"
    Invoke-Checked (Join-Path $WixDirectory 'candle.exe') @('-nologo', '-arch', 'x64', "-dProbeDll=$dll", "-dPayload=$payload",
        "-dProductCode=$product", "-dInstallScope=$scope", '-out', $object, (Join-Path $repo 'tests/fixtures/msi-runtime/probe.wxs'))
    Invoke-Checked (Join-Path $WixDirectory 'light.exe') @('-nologo', '-out', $msi, $object)
    $database = $installer.OpenDatabase($msi, 0)
    $properties = @{}
    foreach ($row in (Read-MsiRows 'SELECT `Property`, `Value` FROM `Property`' 2)) { $properties[$row[0]] = $row[1] }
    if ($properties.ProductCode -ne $product -or $properties.Manufacturer -ne 'Ridge CI fixture' -or
        $properties.UpgradeCode -ne '{1B2498C2-CBA6-4943-ADEA-806CA55B7F90}') { throw 'Unexpected fixture MSI identity.' }
    if (($properties['ALLUSERS'] -eq '1') -ne ($scope -eq 'perMachine')) { throw 'Unexpected MSI installation scope.' }
    $sequence = @(Read-MsiRows 'SELECT `Action`, `Condition`, `Sequence` FROM `InstallExecuteSequence`' 3)
    $sequence | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $output "$scope-sequence.json")
    foreach ($flag in @('-1', '-2', '-3', '-4')) {
        if (@($sequence | Where-Object { $_[2] -eq $flag }).Count -ne 1) { throw "Missing termination action $flag" }
    }
    $packages += [pscustomobject]@{ scope = $scope; product = $product; sha256 = (Get-FileHash $msi -Algorithm SHA256).Hash }
foreach ($ui in @('/qn', '/qf')) {
    foreach ($mode in @('success', 'failure', 'cancel', 'crash')) {
        $caseRoot = Join-Path $output ($scope + '-' + $ui.TrimStart('/') + '-' + $mode)
        New-Item -ItemType Directory -Path $caseRoot | Out-Null
        try {
            $exit = Invoke-ProbeMsi $caseRoot $ui $mode
            $expected = @{ success = 0; failure = 1603; cancel = 1602; crash = 1603 }[$mode]
            if ($exit -ne $expected) { throw "$ui/$mode exit $exit; expected $expected" }
            $trace = @(Get-Content (Join-Path $caseRoot 'trace.jsonl') | ForEach-Object { $_ | ConvertFrom-Json })
            if (@($trace | Where-Object stage -eq 'before').Count -ne 1 -or
                @($trace | Where-Object { $_.stage -eq 'written' -and $_.installedFileExists }).Count -ne 1) {
                throw "$ui/$mode did not execute actual file installation."
            }
            $ending = @{ success = 'exit-success'; failure = 'exit-error'; cancel = 'exit-cancel'; crash = 'exit-error' }[$mode]
            $termination = @($trace | Where-Object { $_.stage -like 'exit-*' })
            if ($termination.Count -ne 1 -or $termination[0].stage -ne $ending -or
                $termination[0].installedFileExists -ne ($mode -eq 'success')) {
                throw "$ui/$mode termination callback did not follow commit/rollback."
            }
            if ($KernelPath -and (Get-Content -LiteralPath (Join-Path $caseRoot 'runtime.done') -Raw) -ne 'DONE') {
                throw "$scope/$ui/$mode did not release the actual Kernel helper."
            }
            Assert-HelperReleased $caseRoot
            $before = @($trace | Where-Object stage -eq 'before')[0]
            # A CA worker exit alone can commit (actual run 44046218217). Inject
            # a subsequent business failure to verify rollback in its replacement.
            if ($mode -eq 'crash' -and ($before.process.pid -eq $termination[0].process.pid -or
                @($trace | Where-Object stage -eq 'crash').Count -ne 1 -or
                @($trace | Where-Object stage -eq 'fail').Count -ne 1)) { throw 'Worker replacement plus business failure was not exercised.' }
            $results += [pscustomobject]@{ scope = $scope; ui = $ui; mode = $mode; exit = $exit; stages = @($trace.stage);
                before = $before; termination = $termination[0] }
        } finally {
            if ($installer.ProductState($product) -eq 5) {
                $cleanupExit = Invoke-ProbeMsi $caseRoot '/qn' 'success' -Uninstall
                if ($cleanupExit -ne 0) { throw "Isolated fixture uninstall failed: $cleanupExit" }
                if ($KernelPath -and (Get-Content -LiteralPath (Join-Path $caseRoot 'uninstall-runtime.done') -Raw) -ne 'DONE') {
                    throw 'Fixture uninstall did not release the actual Kernel helper.'
                }
                Assert-HelperReleased $caseRoot
            }
        }
    }
}
if ($installer.ProductState($product) -eq 5) { throw 'Isolated fixture remains installed.' }
}
[pscustomobject]@{ scope = 'MSI lifecycle probe only'; runtimeGuardImplemented = $false; productionAcceptance = $false;
    source = $env:GITHUB_SHA; packages = $packages; cases = $results
    helperSha256 = if ($KernelPath) { (Get-FileHash $KernelPath -Algorithm SHA256).Hash } else { $null }
} | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $output 'probe-results.json')
Write-Output "MSI lifecycle probe: $($results.Count) cases; production Runtime guard remains unverified."
