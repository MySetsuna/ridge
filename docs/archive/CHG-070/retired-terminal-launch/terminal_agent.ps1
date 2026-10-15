# Loaded only by Ridge's interactive PowerShell. Never edits the user's profile.
& {
foreach ($entry in $__ridgeTerminalProfiles) {
    if (!$entry.terminalProtocol -or $entry.terminalProtocol.protocol -ne 'app-server') { continue }
    $name = [string]$entry.executable
    if ($name -notmatch '^[a-zA-Z0-9_.-]+$') { continue }
    $original = Get-Command $name -ErrorAction SilentlyContinue
    if (!$original) { continue }
    if ($original.CommandType -eq 'Alias') { $original = $original.ResolvedCommand }
    if ($original.CommandType -notin @('Function', 'Application', 'ExternalScript')) { continue }
    $invocation = @{
        kind = if ($original.CommandType -eq 'Function') { 'function' } else { 'path' }
        command = if ($original.CommandType -eq 'Function') { $original.Definition } else { $original.Path }
        shell = (Get-Process -Id $PID).Path
        config = $entry.terminalProtocol
    }
    $helper = $__ridgeTerminalKernel
    $function = {
        $arguments = @($args | ForEach-Object { [string]$_ })
        $bypass = $false
        foreach ($argument in $arguments) {
            if ($argument -in @('--help', '-h', '--version', '-V', '--remote') -or $argument.StartsWith('--remote=')) { $bypass = $true }
        }
        # Global option values are not subcommands.
        for ($index = 0; $index -lt $arguments.Count; $index++) {
            $argument = $arguments[$index]
            if ($argument -in @('-c', '--config', '-p', '--profile', '--enable', '--disable', '--cd', '-C', '--model', '-m', '--sandbox', '-s', '--ask-for-approval', '-a', '--image', '-i', '--local-provider', '--add-dir')) { $index++; continue }
            if (!$argument.StartsWith('-')) { if ($argument -in $invocation.config.passthrough) { $bypass = $true }; break }
        }
        if ($bypass) {
            if ($invocation.kind -eq 'function') { & ([scriptblock]::Create($invocation.command)) @arguments }
            else { & $invocation.command @arguments }
            return
        }
        $prior = $env:RIDGE_COMMUNE_LAUNCH
        try {
            $payload = @{ kind=$invocation.kind; command=$invocation.command; shell=$invocation.shell; config=$invocation.config; arguments=$arguments }
            $env:RIDGE_COMMUNE_LAUNCH = [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($payload | ConvertTo-Json -Depth 12 -Compress)))
            & $helper --commune-terminal
        } finally { $env:RIDGE_COMMUNE_LAUNCH = $prior }
    }.GetNewClosure()
    Set-Item -LiteralPath "function:global:$name" -Value $function
}
}
Remove-Variable __ridgeTerminalProfiles, __ridgeTerminalKernel -ErrorAction SilentlyContinue
