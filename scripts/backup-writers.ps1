# Coordinate only application-owned writers; callers always resume in finally.
function New-BackupWriterState([string[]]$ComposeArgs, [string]$ProcessStateFile, [hashtable]$EnvironmentValues) {
    $running = @(& docker @ComposeArgs ps --status running --services)
    if ($LASTEXITCODE -ne 0) { throw 'Could not inspect backup services.' }
    $state = @{ services = @(); native = @(); marker = $null; processFile = $null; processState = $null; environment = $EnvironmentValues }
    $state.services = @($running | Where-Object { $_ -in @('web', 'media-gateway', 'api', 'worker') })

    if (!$ProcessStateFile) { return $state }
    $state.processFile = (Resolve-Path -LiteralPath $ProcessStateFile).Path
    if (!$state.processFile.StartsWith($chatRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Native process records must stay inside this workspace.'
    }
    $state.processState = Get-Content -LiteralPath $state.processFile -Raw | ConvertFrom-Json
    $state.marker = $EnvironmentValues.CHAT_SHUTDOWN_FILE
    if (!$state.marker -and $state.processState -is [array]) {
        $state.marker = Join-Path $chatRoot '.superpowers/dev/maintenance.stop'
    }
    if (!$state.marker) { throw 'Native writers need CHAT_SHUTDOWN_FILE for graceful maintenance.' }
    $state.marker = [IO.Path]::GetFullPath($state.marker)
    if (!$state.marker.StartsWith($chatRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Maintenance marker must stay inside this workspace.'
    }
    if (Test-Path -LiteralPath $state.marker) { throw 'A maintenance marker already exists; inspect that operation first.' }

    $candidates = if ($state.processState -is [array]) {
        @($state.processState | Where-Object { [IO.Path]::GetFileName($_.path) -in @('api.exe', 'worker.exe') } | ForEach-Object { @{ role = [IO.Path]::GetFileNameWithoutExtension($_.path); pid = $_.pid; started = $_.started } })
    } else {
        @(@{ role = 'api'; pid = $state.processState.apiPid; started = $state.processState.apiStartTicks }, @{ role = 'worker'; pid = $state.processState.workerPid; started = $state.processState.workerStartTicks })
    }
    foreach ($candidate in $candidates) {
        if (!$candidate.pid) { continue }
        $process = Get-Process -Id $candidate.pid -ErrorAction SilentlyContinue
        if (!$process) { continue }
        if (!$process.Path.StartsWith($chatRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
            [IO.Path]::GetFileNameWithoutExtension($process.Path) -ne $candidate.role -or
            ($candidate.started -and $process.StartTime.ToUniversalTime().Ticks.ToString() -ne $candidate.started)) {
            throw 'Native writer identity no longer matches its owned process record.'
        }
        $state.native += @{ role = $candidate.role; process = $process; path = $process.Path }
    }
    return $state
}

function Stop-BackupWriters([hashtable]$State, [string[]]$ComposeArgs) {
    # Closing ingress also invalidates outstanding PUT capabilities for the
    # snapshot interval. No request can create bytes after storage is stopped.
    $ingress = @($State.services | Where-Object { $_ -in @('web', 'media-gateway') })
    if ($ingress.Count) {
        & docker @ComposeArgs stop --timeout 90 @ingress
        if ($LASTEXITCODE -ne 0) { throw 'Public ingress could not drain.' }
    }
    if ($State.native.Count) {
        [IO.File]::WriteAllText($State.marker, 'backup maintenance')
        $State.markerCreated = $true
        foreach ($owned in $State.native) {
            if (!$owned.process.WaitForExit(90000)) { throw 'Native writer did not finish graceful shutdown before the backup deadline.' }
        }
    }
    $writers = @($State.services | Where-Object { $_ -in @('api', 'worker') })
    if ($writers.Count) {
        & docker @ComposeArgs stop --timeout 90 @writers
        if ($LASTEXITCODE -ne 0) { throw 'Application writers could not drain.' }
    }
}

function Start-BackupWriters([hashtable]$State, [string[]]$ComposeArgs) {
    if ($State.markerCreated) { Remove-Item -LiteralPath $State.marker }
    foreach ($key in $State.environment.Keys) {
        [Environment]::SetEnvironmentVariable($key, $State.environment[$key], 'Process')
    }
    if ($State.marker) { $env:CHAT_SHUTDOWN_FILE = $State.marker }

    foreach ($owned in $State.native) {
        if (!$owned.process.HasExited) { continue }
        $runtime = Split-Path -Parent $owned.path
        $process = Start-Process -FilePath $owned.path -WorkingDirectory $chatRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runtime "$($owned.role)-after-backup.log") -RedirectStandardError (Join-Path $runtime "$($owned.role)-after-backup.err.log")
        if ($State.processState -is [array]) {
            $record = $State.processState | Where-Object { $_.path -eq $owned.path }
            $record.pid = $process.Id
            $record.started = $process.StartTime.ToUniversalTime().Ticks.ToString()
        } else {
            $State.processState."$($owned.role)Pid" = $process.Id
            $State.processState | Add-Member -NotePropertyName "$($owned.role)StartTicks" -NotePropertyValue ($process.StartTime.ToUniversalTime().Ticks.ToString()) -Force
        }
    }
    if ($State.processFile) { $State.processState | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $State.processFile }
    if ($State.services.Count) {
        & docker @ComposeArgs start @($State.services)
        if ($LASTEXITCODE -ne 0) { throw 'Application services could not resume after backup.' }
    }
}
