param(
    [switch]$ProbeOnly,
    [switch]$ProductionGuard,
    [string]$PackagePath,
    [string]$KernelPath,
    [string]$CliPath,
    [string]$WixDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Installation is intentionally confined to disposable GitHub Windows runners.
if (!$IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or !$env:RUNNER_TEMP) {
    throw 'Actual MSI execution requires a disposable GitHub Windows runner.'
}
if (([int][bool]$ProbeOnly + [int][bool]$ProductionGuard + [int][bool]$PackagePath) -ne 1) { throw 'Select the probe, production guard fixture, or complete package.' }
if (!$PackagePath -and !$WixDirectory) { throw 'WiX tools are required for the isolated fixture.' }
if (($ProductionGuard -and (!$KernelPath -or !$CliPath)) -or (($ProductionGuard -or $PackagePath) -and
    ([Environment]::GetEnvironmentVariable('RIDGE_KERNEL_DATA_DIR') -ne $null -or
     [Environment]::GetEnvironmentVariable('RIDGE_TEST_ALLOW_NON_BREAKAWAY') -ne $null))) {
    throw 'Production guard acceptance requires its actual Kernel, native user namespace and production breakaway behavior.'
}
$output = [IO.Path]::GetFullPath($OutputDirectory)
$runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\') + '\'
if (!$output.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase) -or $output.Contains('"')) {
    throw 'Probe output must be a child of RUNNER_TEMP.'
}
$repo = Split-Path $PSScriptRoot -Parent
if ($KernelPath) { $KernelPath = (Resolve-Path -LiteralPath $KernelPath).Path; if ($KernelPath.Contains('"')) { throw 'Invalid Kernel path.' } }
if ($CliPath) { $CliPath = (Resolve-Path -LiteralPath $CliPath).Path; if ($CliPath.Contains('"')) { throw 'Invalid CLI path.' } }
New-Item -ItemType Directory -Path $output -Force | Out-Null
$dll = Join-Path $output 'probe.dll'
$payload = Join-Path $output 'sentinel.txt'

function Invoke-Checked([string]$Executable, [string[]]$Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Executable failed: $LASTEXITCODE" }
}
if (!$PackagePath) {
[IO.File]::WriteAllText($payload, 'Ridge isolated MSI lifecycle fixture')
Push-Location $output
try {
    Invoke-Checked 'cl.exe' @('/nologo', '/std:c++17', '/W4', '/WX', '/EHsc', '/MT', '/LD',
        (Join-Path $repo 'tests/fixtures/msi-runtime/probe.cpp'), '/link', '/OUT:probe.dll', 'msi.lib', 'advapi32.lib', 'user32.lib')
} finally { Pop-Location }
Add-Type @"
using System.Runtime.InteropServices;
using System.Text;
public static class RidgeMsiRuntimeDialog {
    [DllImport(@"$dll", CharSet = CharSet.Unicode, CallingConvention = CallingConvention.StdCall, ExactSpelling = true)]
    public static extern uint ProbeDismissRuntimeError(uint pid, StringBuilder message, uint capacity);
}
"@
}

$installer = New-Object -ComObject WindowsInstaller.Installer
$nativeDataRoot = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'ridge'
if (($ProductionGuard -or $PackagePath) -and (Test-Path -LiteralPath $nativeDataRoot)) { throw 'Disposable runner already contains Ridge data; refusing to touch it.' }
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

if ($PackagePath) {
    $PackagePath = (Resolve-Path -LiteralPath $PackagePath).Path
    if (!$PackagePath.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase) -or $PackagePath.Contains('"')) { throw 'Complete package must be an isolated runner artifact.' }
    $manifest = Get-Content -LiteralPath (Join-Path (Split-Path $PackagePath) 'package-contract.json') -Raw | ConvertFrom-Json
    $packageHash = (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash
    if ($manifest.package -ne (Split-Path $PackagePath -Leaf) -or $manifest.packageSha256 -ne $packageHash -or
        $manifest.source -notmatch '^[a-f0-9]{40}$' -or $manifest.embeddedHelperSha256 -notmatch '^[a-f0-9]{64}$') { throw 'Complete package provenance mismatch.' }
    $database = $installer.OpenDatabase($PackagePath, 0)
    try {
        $properties = @{}; foreach ($row in (Read-MsiRows 'SELECT `Property`,`Value` FROM `Property`' 2)) { $properties[$row[0]] = $row[1] }
        if ($properties.ProductName -ne 'ridge' -or $properties.UpgradeCode -ne '{581A9D58-1141-5F5E-9E66-CAD20FE063CE}' -or
            $installer.ProductState($properties.ProductCode) -ne -1) { throw 'Refusing a foreign or previously registered product.' }
        $related = $installer.RelatedProducts($properties.UpgradeCode)
        try { if ($related.Count) { throw 'Refusing to replace any pre-existing related Ridge product.' } }
        finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($related) }
        $sequence = @{}; foreach ($row in (Read-MsiRows 'SELECT `Action`,`Sequence` FROM `InstallExecuteSequence`' 2)) { $sequence[$row[0]] = [int]$row[1] }
        foreach ($required in @('RidgeRuntimeReserve', 'RidgeRuntimeVerify', 'RidgeRuntimeRelease', 'InstallValidate', 'InstallInitialize', 'InstallFinalize')) {
            if (!$sequence.ContainsKey($required)) { throw "Complete package omits $required" }
        }
        if ($sequence.RidgeRuntimeReserve -ge $sequence.InstallValidate -or $sequence.RidgeRuntimeVerify -ge $sequence.InstallInitialize -or
            $sequence.RidgeRuntimeRelease -le $sequence.InstallFinalize) { throw 'Unsafe complete-package guard sequence.' }
        $KernelPath = Join-Path $output 'package-installer-helper.exe'
        $view = $database.OpenView('SELECT `Data` FROM `Binary` WHERE `Name`=''RidgeInstallHost''')
        [void]$view.Execute(); $record = $view.Fetch()
        if (!$record) { throw 'Complete package has no embedded Runtime helper.' }
        $stream = [IO.File]::Create($KernelPath)
        try {
            while ($true) {
                [string]$chunk = $record.ReadStream(1, 65536, 1)
                if (!$chunk.Length) { break }
                $bytes = [Text.Encoding]::GetEncoding(28591).GetBytes($chunk)
                $stream.Write($bytes, 0, $bytes.Length)
            }
        } finally {
            $stream.Dispose(); [void]$view.Close()
            foreach ($handle in @($record, $view)) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($handle) }
        }
    } finally { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($database) }
    if ((Get-FileHash -LiteralPath $KernelPath -Algorithm SHA256).Hash -ne $manifest.embeddedHelperSha256) { throw 'Extracted helper does not match the build manifest.' }
    $fullProduct = $properties.ProductCode
    $fullInstalled = Join-Path $output 'installed-ridge'
    $fullOwned = $false; $fullCases = @(); $fullFiles = @('ridge.exe', 'ridge-kernel.exe', 'ridge-mcp.exe', 'tmux.exe', 'ridge.theme')
    $fullCanary = Join-Path $nativeDataRoot 'installer-user-content/workspace-canary.ridge'
    New-Item -ItemType Directory -Path (Split-Path $fullCanary) -Force | Out-Null
    [IO.File]::WriteAllText($fullCanary, ('Installer preservation sample ' + [guid]::NewGuid()))
    $fullCanaryHash = (Get-FileHash -LiteralPath $fullCanary -Algorithm SHA256).Hash
    function Invoke-FullMsi([string]$Operation, [string]$Stage) {
        $target = if ($Operation -eq '/i') { '"' + $PackagePath + '"' } else { $fullProduct }
        $arguments = @($Operation, $target, '/qn', '/norestart', 'REBOOT=ReallySuppress',
            'WIXUI_EXITDIALOGOPTIONALCHECKBOX=0', ('INSTALLDIR="' + $fullInstalled + '"'), '/l*v', ('"' + (Join-Path $output ($Stage + '.log')) + '"'))
        $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -WindowStyle Hidden -PassThru
        if (!$process.WaitForExit(120000)) { throw 'Owned complete-package operation timed out; runner teardown owns cleanup.' }
        $process.Refresh(); return $process.ExitCode
    }
    try {
        foreach ($stage in @('install', 'repair', 'uninstall')) {
            if ($stage -eq 'repair') {
                # Damage only the registered resource installed by this test.
                $resource = (Resolve-Path -LiteralPath (Join-Path $fullInstalled 'ridge.theme')).Path
                if (!$resource.StartsWith($fullInstalled.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Resource deletion escaped the test installation.' }
                Remove-Item -LiteralPath $resource -Force
            }
            $fullOwned = $true
            $operation = @{ install = '/i'; repair = '/famus'; uninstall = '/x' }[$stage]
            $exit = Invoke-FullMsi $operation $stage
            if ($exit -ne 0) { throw "Complete package $stage failed: $exit" }
            $hashes = @{}
            if ($stage -ne 'uninstall') {
                if ($installer.ProductState($fullProduct) -ne 5 -or
                    $installer.ProductInfo($fullProduct, 'InstallLocation').TrimEnd('\') -ne $fullInstalled.TrimEnd('\')) { throw 'Installed product identity or directory changed.' }
                foreach ($file in $fullFiles) { $hashes[$file] = (Get-FileHash -LiteralPath (Join-Path $fullInstalled $file) -Algorithm SHA256).Hash }
                if ($stage -eq 'install') { $fullBaseline = $hashes }
                else { foreach ($file in $fullFiles) { if ($hashes[$file] -ne $fullBaseline[$file]) { throw 'Real repair did not restore the installed payload.' } } }
                if ($hashes['ridge-kernel.exe'] -ne $manifest.embeddedHelperSha256) { throw 'Installed Runtime differs from the package helper build.' }
            } else {
                if ($installer.ProductState($fullProduct) -ne -1) { throw 'Complete package remains registered.' }
                foreach ($file in $fullFiles) { if (Test-Path -LiteralPath (Join-Path $fullInstalled $file)) { throw 'Uninstall left a packaged executable or resource.' } }
            }
            if ((Get-FileHash -LiteralPath $fullCanary -Algorithm SHA256).Hash -ne $fullCanaryHash -or
                (Get-FileHash -LiteralPath $PackagePath -Algorithm SHA256).Hash -ne $packageHash) { throw 'Installer changed preserved user content or the candidate MSI.' }
            & $KernelPath --installer-check
            if ($LASTEXITCODE -ne 0) { throw 'Complete package left a Runtime reservation or started an unexpected service.' }
            $fullCases += [pscustomobject]@{ stage = $stage; exit = $exit; installedHashes = $hashes; userCanarySha256 = $fullCanaryHash }
            [pscustomobject]@{ scope = 'Complete Tauri MSI quiet install, missing-resource repair and uninstall'; productionAcceptance = $false;
                driverSource = $env:GITHUB_SHA; packageSource = $manifest.source; packageSha256 = $packageHash; cases = $fullCases
            } | ConvertTo-Json -Depth 8 | Set-Content (Join-Path $output 'full-package-results.json')
        }
    } catch {
        ($_.Exception.Message + "`n" + $_.ScriptStackTrace) | Set-Content (Join-Path $output 'primary-error.txt')
        throw
    } finally {
        if ($fullOwned -and $installer.ProductState($fullProduct) -eq 5 -and (Invoke-FullMsi '/x' 'cleanup') -ne 0) { throw 'Owned complete-package cleanup failed.' }
    }
    Write-Output 'Complete Tauri MSI: install, actual missing-resource repair and uninstall passed; remaining acceptance stays open.'
    return
}
function Invoke-ProbeMsi([string]$CaseRoot, [string]$Ui, [string]$Mode, [switch]$Uninstall, [switch]$Repair) {
    $operation = if ($Uninstall) { '/x' } elseif ($Repair) { '/fomus' } else { '/i' }
    $target = if ($Uninstall -or $Repair) { $product } else { '"' + $msi + '"' }
    $log = Join-Path $CaseRoot $(if ($Uninstall) { 'uninstall.log' } elseif ($Repair) { 'repair.log' } else { 'install.log' })
    $arguments = @($operation, $target, $Ui, '/norestart', 'REBOOT=ReallySuppress', '/l*v', ('"' + $log + '"'),
        ('RIDGE_PROBE_ROOT="' + $CaseRoot + '"'), ('PROBE_INSTALL="' + (Join-Path $CaseRoot 'installed') + '"'), "PROBE_MODE=$Mode")
    if ($ProductionGuard) { $arguments += 'RIDGE_PROBE_DATA_DIR="' + $nativeDataRoot + '"' }
    elseif ($KernelPath) { $arguments += 'RIDGE_PROBE_HELPER="' + $KernelPath + '"' }
    if ($Mode -eq 'active') { $arguments += 'PROBE_LAUNCH_KERNEL=1' }
    $process = Start-Process -FilePath "$env:SystemRoot\System32\msiexec.exe" -ArgumentList $arguments -WindowStyle Hidden -PassThru
    $deadline = (Get-Date).AddSeconds(120)
    $dialogMessages = @()
    while (!$process.WaitForExit(100)) {
        if ($ProductionGuard -and $Ui -eq '/qf') {
            $message = [Text.StringBuilder]::new(8192)
            $dismissed = [RidgeMsiRuntimeDialog]::ProbeDismissRuntimeError($process.Id, $message, 8192)
            if ($dismissed) {
                $dialogMessages += $message.ToString()
                $dialogMessages | ConvertTo-Json -AsArray | Set-Content ($log + '.runtime-errors.json')
                if ($dismissed -ne 1) { throw 'Expected native Runtime error dialog has no working confirmation button.' }
            }
        }
        if ((Get-Date) -ge $deadline) { throw "MSI $Mode timed out; runner teardown owns cleanup." }
    }
    $process.Refresh()
    return $process.ExitCode
}

function Assert-HelperReleased([string]$CaseRoot) {
    if (!$KernelPath) { return }
    $savedDataDir = $env:RIDGE_KERNEL_DATA_DIR
    try {
        $env:RIDGE_KERNEL_DATA_DIR = if ($ProductionGuard) { $null } else { Join-Path $CaseRoot 'runtime' }
        & $KernelPath --installer-check
        if ($LASTEXITCODE -ne 0) { throw 'Real Kernel installer reservation remains held after MSI completion.' }
    } finally { $env:RIDGE_KERNEL_DATA_DIR = $savedDataDir }
}

function Wait-HelperDone([string]$CaseRoot) {
    if ($ProductionGuard) {
        for ($attempt = 0; $attempt -lt 100; $attempt++) {
            if (!(Test-Path -LiteralPath (Join-Path $nativeDataRoot 'installer-msi.json'))) { return }
            Start-Sleep -Milliseconds 100
        }
        throw 'Actual production guard remains after the MSI transaction.'
    }
    $done = Join-Path $CaseRoot 'runtime.done'
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if ((Test-Path -LiteralPath $done) -and (Get-Content -LiteralPath $done -Raw) -eq 'DONE') { return }
        Start-Sleep -Milliseconds 100
    }
    throw 'Actual Kernel helper did not release after MSI completion.'
}

function Invoke-OwnedKernel([object]$Endpoint, [string]$Route, [string]$Method = 'GET', [object]$Body = $null) {
    $request = @{ Uri = "http://127.0.0.1:$($Endpoint.port)$Route"; Method = $Method; NoProxy = $true;
        Headers = @{ 'x-ridge-kernel-token' = $Endpoint.token }; TimeoutSec = 10 }
    if ($null -ne $Body) { $request.ContentType = 'application/json'; $request.Body = $Body | ConvertTo-Json -Depth 8 -Compress }
    return Invoke-RestMethod @request
}

function Test-ActiveKernel([string]$CaseRoot, [string]$Ui) {
    $endpoint = $null; $ownedKernel = $null; $pane = [guid]::NewGuid().ToString()
    $installed = Join-Path $CaseRoot 'installed'
    $endpointFile = Join-Path $nativeDataRoot 'kernel.json'
    if (Test-Path -LiteralPath $endpointFile) { throw 'Unexpected Runtime before owned active-Kernel fixture.' }
    try {
        if ((Invoke-ProbeMsi $CaseRoot $Ui 'active') -ne 0) { throw 'Actual CLI could not launch its installed sibling Kernel.' }
        for ($attempt = 0; $attempt -lt 200; $attempt++) {
            if (Test-Path -LiteralPath $endpointFile) {
                $candidate = Get-Content -LiteralPath $endpointFile -Raw | ConvertFrom-Json
                $process = Get-Process -Id $candidate.pid -ErrorAction SilentlyContinue
                if ($process -and $process.Path -eq (Join-Path $installed 'ridge-kernel.exe')) {
                    $endpoint = $candidate; $ownedKernel = $process; break
                }
            }
            Start-Sleep -Milliseconds 100
        }
        if (!$ownedKernel) { throw 'Real installed Kernel did not publish an attested endpoint.' }
        $health = Invoke-OwnedKernel $endpoint '/v1/health'
        if ($health.pid -ne $ownedKernel.Id -or $health.started_at_unix -ne $endpoint.started_at_unix) { throw 'Owned Kernel health identity mismatch.' }
        $hashes = @{}; foreach ($name in @('sentinel.txt', 'ridge.exe', 'ridge-kernel.exe')) {
            $hashes[$name] = (Get-FileHash -LiteralPath (Join-Path $installed $name) -Algorithm SHA256).Hash
        }
        if ($hashes['ridge-kernel.exe'] -ne (Get-FileHash -LiteralPath $KernelPath -Algorithm SHA256).Hash) { throw 'Live Kernel differs from the actual build.' }
        $shell = Join-Path ([Environment]::GetFolderPath('System')) 'WindowsPowerShell/v1.0/powershell.exe'
        $created = Invoke-OwnedKernel $endpoint '/v1/domain/ptys' 'POST' @{
            pty_id = $pane; workspace_id = [guid]::NewGuid().ToString(); program = $shell; cwd = $CaseRoot; role = 'shell'; cols = 80; rows = 24;
            args = @('-NoProfile', '-NonInteractive', '-Command', "while (`$true) { [Console]::WriteLine('ridge-msi-heartbeat'); Start-Sleep -Milliseconds 100 }")
        }
        if (!$created.ok) { throw 'Real heartbeat PTY creation failed.' }
        Start-Sleep -Milliseconds 500
        $baseline = @( (Invoke-OwnedKernel $endpoint '/v1/domain/ptys').ptys | Where-Object pty_id -eq $pane )[0]
        if (!$baseline.child_pid -or !$baseline.child_start_instance) { throw 'Heartbeat has no real child identity.' }
        $rows = @()
        foreach ($operation in @('install', 'repair', 'uninstall')) {
            $operationRoot = Join-Path $CaseRoot $operation
            New-Item -ItemType Directory -Path $operationRoot | Out-Null
            # Keep the installed directory fixed while placing each attempt's log separately.
            $exit = Invoke-ProbeMsi $CaseRoot $Ui 'success' -Uninstall:($operation -eq 'uninstall') -Repair:($operation -eq 'repair')
            Copy-Item -LiteralPath (Join-Path $CaseRoot "$operation.log") -Destination (Join-Path $operationRoot 'msi.log')
            if ($exit -ne 1603) { throw "Active Runtime $operation was not refused: $exit" }
            if ($Ui -eq '/qf' -and !(Test-Path -LiteralPath (Join-Path $CaseRoot "$operation.log.runtime-errors.json"))) {
                throw 'Full UI did not display the actionable native Runtime refusal.'
            }
            $errorLog = Join-Path $nativeDataRoot 'installer-last-error.txt'
            if (Test-Path -LiteralPath $errorLog) { Copy-Item -LiteralPath $errorLog -Destination (Join-Path $operationRoot 'core-error.txt') }
            $after = @( (Invoke-OwnedKernel $endpoint '/v1/domain/ptys').ptys | Where-Object pty_id -eq $pane )[0]
            if ($after.child_pid -ne $baseline.child_pid -or $after.child_start_instance -ne $baseline.child_start_instance -or
                $after.next_seq -le $baseline.next_seq) { throw 'Active PTY was replaced, stopped or lost its output heartbeat.' }
            foreach ($name in $hashes.Keys) {
                if ((Get-FileHash -LiteralPath (Join-Path $installed $name) -Algorithm SHA256).Hash -ne $hashes[$name]) { throw 'Refused MSI changed an installed file.' }
            }
            if ($installer.ProductState($product) -ne 5) { throw 'Refused MSI changed the installed product state.' }
            $rows += [pscustomobject]@{ scope = $scope; ui = $Ui; mode = "active-$operation"; exit = $exit;
                kernelPid = $health.pid; kernelStartedAt = $health.started_at_unix; installedFileHashes = $hashes;
                ptyPid = $after.child_pid; ptyStartedInstance = $after.child_start_instance;
                outputSequenceBefore = $baseline.next_seq; outputSequenceAfter = $after.next_seq }
            $baseline = $after
            $rows | ConvertTo-Json -Depth 8 -AsArray | Set-Content (Join-Path $CaseRoot 'active-results.json')
        }
        return $rows
    } catch {
        ($_.Exception.Message + "`n" + $_.ScriptStackTrace) | Set-Content (Join-Path $CaseRoot 'primary-error.txt')
        throw
    } finally {
        if ($endpoint -and $ownedKernel) {
            $current = Invoke-OwnedKernel $endpoint '/v1/health'
            if ($current.pid -ne $ownedKernel.Id -or $current.started_at_unix -ne $endpoint.started_at_unix) { throw 'Refusing cleanup of a different Kernel instance.' }
            [void](Invoke-OwnedKernel $endpoint "/v1/domain/ptys/$pane" 'DELETE')
            [void](Invoke-OwnedKernel $endpoint '/v1/shutdown' 'POST')
            if (!$ownedKernel.WaitForExit(15000)) { throw 'Owned Kernel did not shut down; runner teardown owns cleanup.' }
        }
        if ($installer.ProductState($product) -eq 5) {
            if ((Invoke-ProbeMsi $CaseRoot '/qn' 'success' -Uninstall) -ne 0) { throw 'Owned active-Kernel fixture uninstall failed.' }
            Wait-HelperDone $CaseRoot
        }
        Assert-HelperReleased $CaseRoot
    }
}

$results = @()
$packages = @()
foreach ($scope in @('perUser', 'perMachine')) {
    $product = if ($scope -eq 'perUser') { '{056D63CA-DA83-4458-8CBF-5D98C18F89BE}' } else { '{B6B6DC01-A158-4943-A3C3-DC0CA105A57F}' }
    $msi = Join-Path $output "$scope.msi"
    $object = Join-Path $output "$scope.wixobj"
    $candleArguments = @('-nologo', '-arch', 'x64', "-dProbeDll=$dll", "-dPayload=$payload",
        "-dProductCode=$product", "-dInstallScope=$scope", '-out', $object, (Join-Path $repo 'tests/fixtures/msi-runtime/probe.wxs'))
    if ($ProductionGuard) { $candleArguments += @('-dWithRuntimeGuard=1', '-dWithLiveKernel=1', "-dKernelPath=$KernelPath", "-dCliPath=$CliPath") }
    Invoke-Checked (Join-Path $WixDirectory 'candle.exe') $candleArguments
    $linkObjects = @($object)
    if ($ProductionGuard) {
        $guardObject = Join-Path $output 'runtime-guard.wixobj'
        Invoke-Checked (Join-Path $WixDirectory 'candle.exe') @('-nologo', '-arch', 'x64', "-dRidgeInstallerRuntimePath=$KernelPath",
            '-out', $guardObject, (Join-Path $repo 'src-tauri/wix/runtime-guard.wxs'))
        $linkObjects += $guardObject
    }
    Invoke-Checked (Join-Path $WixDirectory 'light.exe') (@('-nologo', '-out', $msi) + $linkObjects)
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
    if ($ProductionGuard) {
        foreach ($required in @('RidgeRuntimeReserve', 'RidgeRuntimeVerify', 'RidgeRuntimeRelease')) {
            if (@($sequence | Where-Object { $_[0] -eq $required }).Count -ne 1) { throw "Actual guard action missing: $required" }
        }
        $ordered = @{}; foreach ($action in $sequence) { $ordered[$action[0]] = [int]$action[2] }
        if ($ordered.RidgeRuntimeReserve -ge $ordered.InstallValidate -or
            $ordered.RidgeRuntimeVerify -le $ordered.RidgeRuntimeReserve -or $ordered.RidgeRuntimeVerify -ge $ordered.InstallInitialize -or
            $ordered.RidgeRuntimeRelease -le $ordered.InstallFinalize) { throw 'Runtime protection must precede file-use validation and cover the whole transaction.' }
    }
    $packages += [pscustomobject]@{ scope = $scope; product = $product; sha256 = (Get-FileHash $msi -Algorithm SHA256).Hash }
foreach ($ui in @('/qn', '/qf')) {
    foreach ($mode in @('success', 'failure', 'cancel', 'crash', 'omit-release')) {
        $caseRoot = Join-Path $output ($scope + '-' + $ui.TrimStart('/') + '-' + $mode)
        New-Item -ItemType Directory -Path $caseRoot | Out-Null
        try {
            $baseMsi = $msi
            if ($ProductionGuard -and $mode -eq 'omit-release') {
                # Fault injection touches only this isolated fixture: remove
                # cleanup, retaining both mandatory admission actions unchanged.
                $msi = Join-Path $caseRoot 'missing-cleanup.msi'
                Copy-Item -LiteralPath $baseMsi -Destination $msi
                $fault = $installer.OpenDatabase($msi, 1)
                $delete = $fault.OpenView('DELETE FROM `InstallExecuteSequence` WHERE `Action` = ''RidgeRuntimeRelease''')
                [void]$delete.Execute(); [void]$delete.Close()
                $summary = $fault.SummaryInformation(1)
                [void]$summary.GetType().InvokeMember('Property', 'SetProperty', $null, $summary, @(9, [guid]::NewGuid().ToString('B').ToUpperInvariant()))
                [void]$summary.Persist(); [void]$fault.Commit()
                foreach ($handle in @($summary, $delete, $fault)) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($handle) }
            }
            $exit = Invoke-ProbeMsi $caseRoot $ui $mode
            $expected = @{ success = 0; failure = 1603; cancel = 1602; crash = 1603; 'omit-release' = 0 }[$mode]
            if ($exit -ne $expected) {
                if ($ProductionGuard) {
                    $errorLog = Join-Path $nativeDataRoot 'installer-last-error.txt'
                    if (Test-Path -LiteralPath $errorLog) { Copy-Item -LiteralPath $errorLog -Destination (Join-Path $caseRoot 'core-error.txt') }
                }
                throw "$ui/$mode exit $exit; expected $expected"
            }
            $trace = @(Get-Content (Join-Path $caseRoot 'trace.jsonl') | ForEach-Object { $_ | ConvertFrom-Json })
            if (@($trace | Where-Object stage -eq 'before').Count -ne 1 -or
                @($trace | Where-Object { $_.stage -eq 'written' -and $_.installedFileExists }).Count -ne 1) {
                throw "$ui/$mode did not execute actual file installation."
            }
            $ending = @{ success = 'exit-success'; failure = 'exit-error'; cancel = 'exit-cancel'; crash = 'exit-error'; 'omit-release' = 'exit-success' }[$mode]
            $termination = @($trace | Where-Object { $_.stage -like 'exit-*' })
            if (@($trace | Where-Object { !$_.msiMutex.held }).Count) { throw 'MSI execution ownership was not held throughout the transaction.' }
            if ($termination.Count -ne 1 -or $termination[0].stage -ne $ending -or
                $termination[0].installedFileExists -ne ($mode -in @('success', 'omit-release'))) {
                throw "$ui/$mode termination callback did not follow commit/rollback."
            }
            if ($KernelPath) { Wait-HelperDone $caseRoot }
            if ($mode -eq 'omit-release' -and (Test-Path -LiteralPath (Join-Path $caseRoot 'runtime.release'))) {
                throw 'Fallback release was replaced by a completion callback.'
            }
            Assert-HelperReleased $caseRoot
            $before = @($trace | Where-Object stage -eq 'before')[0]
            if ($mode -eq 'omit-release') {
                $service = Get-Process -Id $before.ancestors[0].pid -ErrorAction Stop
                if ($service.StartTime.ToFileTimeUtc().ToString() -ne $before.ancestors[0].started) {
                    throw 'Missing callback was tested without the original MSI service instance.'
                }
            }
            # A CA worker exit alone can commit (actual run 44046218217). Inject
            # a subsequent business failure to verify rollback in its replacement.
            if ($mode -eq 'crash' -and ($before.process.pid -eq $termination[0].process.pid -or
                @($trace | Where-Object stage -eq 'crash').Count -ne 1 -or
                @($trace | Where-Object stage -eq 'fail').Count -ne 1)) { throw 'Worker replacement plus business failure was not exercised.' }
            $results += [pscustomobject]@{ scope = $scope; ui = $ui; mode = $mode; exit = $exit; stages = @($trace.stage);
                packageSha256 = (Get-FileHash $msi -Algorithm SHA256).Hash;
                releaseCallbackOmitted = $ProductionGuard -and $mode -eq 'omit-release';
                before = $before; termination = $termination[0] }
        } finally {
            # Recover only this fixture's private holder when a red assertion
            # intentionally leaves its callback missing; never stop a Runtime.
            if ($KernelPath -and (Test-Path (Join-Path $caseRoot 'runtime.status')) -and
                !(Test-Path (Join-Path $caseRoot 'runtime.done'))) {
                [IO.File]::WriteAllText((Join-Path $caseRoot 'runtime.release'), 'fixture cleanup')
                Wait-HelperDone $caseRoot
            }
            if ($installer.ProductState($product) -eq 5) {
                $cleanupExit = Invoke-ProbeMsi $caseRoot '/qn' 'success' -Uninstall
                if ($cleanupExit -ne 0) { throw "Isolated fixture uninstall failed: $cleanupExit" }
                if ($ProductionGuard) { Wait-HelperDone $caseRoot }
                elseif ($KernelPath -and (Get-Content -LiteralPath (Join-Path $caseRoot 'uninstall-runtime.done') -Raw) -ne 'DONE') {
                    throw 'Fixture uninstall did not release the actual Kernel helper.'
                }
                Assert-HelperReleased $caseRoot
            }
            $msi = $baseMsi
        }
    }
    if ($ProductionGuard) {
        $activeRoot = Join-Path $output ($scope + '-' + $ui.TrimStart('/') + '-active')
        New-Item -ItemType Directory -Path $activeRoot | Out-Null
        $results += @(Test-ActiveKernel $activeRoot $ui)
    }
}
if ($installer.ProductState($product) -eq 5) { throw 'Isolated fixture remains installed.' }
}
[pscustomobject]@{ scope = if ($ProductionGuard) { 'Actual production WiX guard with isolated payload' } else { 'MSI lifecycle probe only' };
    runtimeGuardImplemented = [bool]$ProductionGuard; productionAcceptance = $false;
    source = $env:GITHUB_SHA; packages = $packages; cases = $results
    helperSha256 = if ($KernelPath) { (Get-FileHash $KernelPath -Algorithm SHA256).Hash } else { $null }
} | ConvertTo-Json -Depth 12 | Set-Content (Join-Path $output 'probe-results.json')
Write-Output "MSI lifecycle probe: $($results.Count) cases; production Runtime guard remains unverified."
