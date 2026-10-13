param([switch]$Prepare,[switch]$Cleanup,[switch]$StartWorker,[switch]$PauseAfterPublish,[switch]$StartWeb)
$ErrorActionPreference='Stop'
$chatRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot
$workDir=Join-Path $chatRoot '.superpowers/sdd/2026-10-12-phase-1-private-chat/integration'
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$stateFile=Join-Path $workDir 'stack.json'
if ($StartWeb) {
    $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
    $env:CHAT_API_PROXY=$state.apiUrl;$env:CHAT_CENTRIFUGO_PROXY='http://127.0.0.1:8001'
    $nodeExe=(Get-Command node).Source
    $web=Start-Process -FilePath $nodeExe -ArgumentList @((Join-Path $chatRoot 'web/node_modules/vite/bin/vite.js'),'--host','localhost','--port','5174','--strictPort') -WorkingDirectory (Join-Path $chatRoot 'web') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $workDir 'web.log') -RedirectStandardError (Join-Path $workDir 'web.err.log')
    $state | Add-Member -NotePropertyName webPid -NotePropertyValue $web.Id -Force
    $state | Add-Member -NotePropertyName webStartTicks -NotePropertyValue ($web.StartTime.ToUniversalTime().Ticks.ToString()) -Force
    $state | Add-Member -NotePropertyName nodeExePath -NotePropertyValue $nodeExe -Force
    $state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
    for($attempt=0;$attempt -lt 80;$attempt++) {try{$r=Invoke-WebRequest -Uri 'http://localhost:5174' -TimeoutSec 2;if($r.StatusCode -eq 200){exit 0}}catch{};Start-Sleep -Milliseconds 250}
    throw 'Test web server did not become ready.'
}
if ($StartWorker) {
    $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
    foreach($line in (Get-Content -LiteralPath $state.envFile -Raw) -split "`n"){if($line -match '^([A-Z_]+)=(.*)$'){[Environment]::SetEnvironmentVariable($Matches[1],$Matches[2].Trim(),'Process')}}
    if ($PauseAfterPublish) {
        cargo build --bin worker --features test-failpoints *> (Join-Path $workDir 'worker-build.log')
        $marker=Join-Path $workDir 'published-marker.json'
        if(Test-Path -LiteralPath $marker){Remove-Item -LiteralPath $marker}
        $env:CHAT_TEST_PAUSE_AFTER_PUBLISH_MARKER=$marker
    } else {
        cargo build --bin worker *> (Join-Path $workDir 'worker-build.log')
        Remove-Item Env:CHAT_TEST_PAUSE_AFTER_PUBLISH_MARKER -ErrorAction SilentlyContinue
    }
    if($LASTEXITCODE -ne 0){throw 'Worker build failed.'}
    Copy-Item -LiteralPath (Join-Path $chatRoot 'target/debug/worker.exe') -Destination (Join-Path $workDir 'worker.exe') -Force
    $worker=Start-Process -FilePath (Join-Path $workDir 'worker.exe') -WorkingDirectory $chatRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $workDir 'worker.log') -RedirectStandardError (Join-Path $workDir 'worker.err.log')
    $state.workerPid=$worker.Id;$state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
    exit 0
}
if ($Cleanup) {
    if (Test-Path -LiteralPath $stateFile) {
        $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
        foreach ($ownedPid in @($state.apiPid,$state.workerPid,$state.webPid)) {
            if (!$ownedPid) {continue}
            $proc=Get-Process -Id $ownedPid -ErrorAction SilentlyContinue
            if ($proc -and ($proc.Path.StartsWith($chatRoot,[StringComparison]::OrdinalIgnoreCase) -or ($ownedPid -eq $state.webPid -and $proc.Path -eq $state.nodeExePath -and $proc.StartTime.ToUniversalTime().Ticks.ToString() -eq $state.webStartTicks))) {Stop-Process -Id $ownedPid -Force}
        }
        if ($state.database -match '^chat_e2e_[0-9a-f]{32}$') {
            docker compose --project-name chat-e2e --env-file $state.envFile --file infra/compose/compose.dev.yaml --file infra/compose/compose.e2e.yaml exec -T postgres psql -U chat -d chat -c "DROP DATABASE IF EXISTS $($state.database) WITH (FORCE)" *> (Join-Path $workDir 'cleanup.log')
            if ($LASTEXITCODE -ne 0) {throw 'Test database cleanup failed.'}
        }
        Remove-Item -LiteralPath $stateFile
    }
    exit 0
}
if (!$Prepare) { throw 'Use -Prepare or -Cleanup.' }
if (Test-Path -LiteralPath $stateFile) {throw 'A test stack already exists. Run scripts/e2e.ps1 -Cleanup first.'}
$envFile=Join-Path $workDir 'stack.env'
$text=(Get-Content -LiteralPath .env -Raw).Replace(':55432/',':55433/').Replace(':56379',':56380').Replace(':8000/api',':8001/api').Replace('http://localhost:5173','http://localhost:5174').Replace('BIND_ADDR=127.0.0.1:8080','BIND_ADDR=127.0.0.1:18080').Replace('INTERNAL_BIND_ADDR=127.0.0.1:8081','INTERNAL_BIND_ADDR=127.0.0.1:18081')
[IO.File]::WriteAllText($envFile,$text)
foreach($line in $text -split "`n"){if($line -match '^([A-Z_]+)=(.*)$'){[Environment]::SetEnvironmentVariable($Matches[1],$Matches[2].Trim(),'Process')}}
docker compose --project-name chat-e2e --env-file $envFile --file infra/compose/compose.dev.yaml --file infra/compose/compose.e2e.yaml up -d --wait *> (Join-Path $workDir 'compose.log')
if($LASTEXITCODE -ne 0){throw 'Test infrastructure failed; inspect integration/compose.log.'}
$testDb='chat_e2e_'+[Guid]::NewGuid().ToString('N')
docker compose --project-name chat-e2e --env-file $envFile --file infra/compose/compose.dev.yaml --file infra/compose/compose.e2e.yaml exec -T postgres psql -U chat -d chat -c "CREATE DATABASE $testDb" *> (Join-Path $workDir 'database.log')
if($LASTEXITCODE -ne 0){throw 'Could not create isolated test database.'}
$text += "`nWORKER_METRICS_ADDR=127.0.0.1:18082`n"
$text=$text.Replace(':55433/chat',":55433/$testDb")
[IO.File]::WriteAllText($envFile,$text)
$env:DATABASE_URL=(($text -split "`n" | Where-Object {$_ -match '^DATABASE_URL='}) -replace '^DATABASE_URL=','').Trim()
cargo build --bins *> (Join-Path $workDir 'build.log')
if($LASTEXITCODE -ne 0){throw 'Rust build failed; inspect integration/build.log.'}
# Run a copy so later Cargo builds can replace target/debug/api.exe on Windows.
Copy-Item -LiteralPath (Join-Path $chatRoot 'target/debug/api.exe') -Destination (Join-Path $workDir 'api.exe') -Force
$api=Start-Process -FilePath (Join-Path $workDir 'api.exe') -WorkingDirectory $chatRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $workDir 'api.log') -RedirectStandardError (Join-Path $workDir 'api.err.log')
$state=@{database=$testDb;apiUrl='http://127.0.0.1:18080';wsUrl='ws://127.0.0.1:8001/connection/websocket';origin='http://localhost:5174';apiPid=$api.Id;workerPid=$null;envFile=$envFile}
$state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
for($attempt=0;$attempt -lt 80;$attempt++){
    try{$r=Invoke-WebRequest -Uri 'http://127.0.0.1:18080/health/ready' -TimeoutSec 2;if($r.StatusCode -eq 200){$state | ConvertTo-Json -Compress;exit 0}}catch{}
    if($api.HasExited){throw 'API exited; inspect integration/api.err.log.'}
    Start-Sleep -Milliseconds 250
}
throw 'Test API did not become ready.'
