param([switch]$Prepare,[switch]$Cleanup,[switch]$StartWorker,[switch]$PauseAfterPublish,[switch]$StartWeb,[switch]$PrepareInfrastructure,[switch]$StopInfrastructure)
$ErrorActionPreference='Stop'
$chatRoot=(Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path.TrimEnd([char[]]'\/')
Set-Location -LiteralPath $chatRoot
$workDir=Join-Path $chatRoot '.superpowers/sdd/2026-10-12-phase-1-private-chat/integration'
New-Item -ItemType Directory -Path $workDir -Force | Out-Null
$stateFile=Join-Path $workDir 'stack.json'
$infrastructureEnv=Join-Path $workDir 'infrastructure.env'

function Initialize-TestEnvironment {
    # Dedicated fixtures never read development credentials or join its volumes.
    $values=@{}
    foreach($line in Get-Content -LiteralPath .env.example) {
        if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$') {
            $values[$Matches[1]]=$Matches[2].Trim()
        }
    }
    $values.POSTGRES_PASSWORD='chat-test-only'
    $values.DATABASE_URL='postgres://chat:chat-test-only@127.0.0.1:55433/chat'
    $values.VALKEY_URL='redis://127.0.0.1:56380'
    $values.CENTRIFUGO_API_URL='http://127.0.0.1:8001/api'
    $values.CENTRIFUGO_API_KEY='chat-test-realtime'
    $values.INTERNAL_PROXY_KEY='chat-test-proxy'
    $values.SEAWEED_ACCESS_KEY='chat-test-media'
    $values.SEAWEED_SECRET_KEY='chat-test-media-secret'
    $values.ALLOWED_ORIGINS='http://localhost:5174'
    $values.BIND_ADDR='127.0.0.1:18080'
    $values.INTERNAL_BIND_ADDR='127.0.0.1:18081'
    $values.WORKER_METRICS_ADDR='127.0.0.1:18082'
    $values.MEDIA_S3_INTERNAL_ENDPOINT='http://127.0.0.1:8334'
    $values.MEDIA_S3_PUBLIC_ENDPOINT='http://localhost:8337'
    $values.CHAT_MEDIA_ORIGIN='http://localhost:8337'
    $values.POSTGRES_PORT='55433'
    $values.VALKEY_PORT='56380'
    $values.CENTRIFUGO_PORT='8001'
    $values.INTERNAL_PROXY_PORT='18081'
    $values.SEAWEED_S3_PORT='8334'
    $values.MEDIA_GATEWAY_PORT='8337'
    $values.SEAWEED_S3_CONFIG=Join-Path $workDir 's3.json'

    $storage=@{identities=@(@{name='test';credentials=@(@{accessKey=$values.SEAWEED_ACCESS_KEY;secretKey=$values.SEAWEED_SECRET_KEY});actions=@('Admin','Read','Write','List','Tagging')})}
    [IO.File]::WriteAllText($values.SEAWEED_S3_CONFIG,($storage | ConvertTo-Json -Depth 8))
    $text=($values.Keys | Sort-Object | ForEach-Object {"$_=$($values[$_])"}) -join "`n"
    [IO.File]::WriteAllText($infrastructureEnv,$text+"`n")
    foreach($key in $values.Keys) {
        [Environment]::SetEnvironmentVariable($key,$values[$key],'Process')
    }

}

if ($PrepareInfrastructure) {
    Initialize-TestEnvironment
    docker compose --project-name chat-test --env-file $infrastructureEnv up -d --wait *> (Join-Path $workDir 'compose.log')
    if($LASTEXITCODE -ne 0){throw 'Test infrastructure failed; inspect integration/compose.log.'}
    exit 0
}
if ($StartWeb) {
    $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
    $env:CHAT_API_PROXY=$state.apiUrl;$env:CHAT_CENTRIFUGO_PROXY='http://127.0.0.1:8001'
    $env:CHAT_MEDIA_ORIGIN='http://localhost:8337'
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
    foreach($line in (Get-Content -LiteralPath $state.envFile -Raw) -split "`n"){if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$'){[Environment]::SetEnvironmentVariable($Matches[1],$Matches[2].Trim(),'Process')}}
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
    $state.workerPid=$worker.Id
    $state | Add-Member -NotePropertyName workerStartTicks -NotePropertyValue ($worker.StartTime.ToUniversalTime().Ticks.ToString()) -Force
    $state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
    for($attempt=0;$attempt -lt 80;$attempt++) {
        $worker.Refresh()
        if($worker.HasExited){throw 'Owned test worker exited during startup; inspect worker.err.log.'}
        try {
            $response=Invoke-WebRequest -Uri 'http://127.0.0.1:18082/metrics' -Headers @{'X-Proxy-Key'=$env:INTERNAL_PROXY_KEY} -TimeoutSec 2
            if($response.StatusCode -eq 200){exit 0}
        } catch {}
        Start-Sleep -Milliseconds 250
    }
    throw 'Owned test worker metrics listener did not become ready.'
}
if ($Cleanup) {
    if (Test-Path -LiteralPath $stateFile) {
        $state=Get-Content -LiteralPath $stateFile -Raw | ConvertFrom-Json
        foreach ($ownedPid in @($state.apiPid,$state.workerPid,$state.webPid)) {
            if (!$ownedPid) {continue}
            $proc=Get-Process -Id $ownedPid -ErrorAction SilentlyContinue
            $started=if($ownedPid -eq $state.apiPid){$state.apiStartTicks}elseif($ownedPid -eq $state.workerPid){$state.workerStartTicks}else{$state.webStartTicks}
            $ownedBinary=$proc -and $proc.Path.StartsWith($chatRoot+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)
            $ownedWeb=$proc -and $ownedPid -eq $state.webPid -and $proc.Path -eq $state.nodeExePath
            if ($proc -and ($ownedBinary -or $ownedWeb) -and (!$started -or $proc.StartTime.ToUniversalTime().Ticks.ToString() -eq $started)) {Stop-Process -Id $ownedPid -Force}
        }
        if($state.mediaBucket -match '^chat-e2e-[0-9a-f]{32}$') {
            foreach($line in (Get-Content -LiteralPath $state.envFile -Raw) -split "`n"){if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$'){[Environment]::SetEnvironmentVariable($Matches[1],$Matches[2].Trim(),'Process')}}
            docker compose --project-name chat-test --env-file $state.envFile stop media-gateway *> (Join-Path $workDir 'media-gateway-cleanup.log')
            if($LASTEXITCODE -ne 0){throw 'Test media ingress shutdown failed.'}
            & (Join-Path $chatRoot 'target/debug/admin.exe') media-cleanup-test *> (Join-Path $workDir 'media-cleanup.log')
            if($LASTEXITCODE -ne 0){throw 'Owned test bucket cleanup failed.'}
        }
        if ($state.database -match '^chat_e2e_[0-9a-f]{32}$') {
            docker compose --project-name chat-test --env-file $state.envFile exec -T postgres psql -U chat -d chat -c "DROP DATABASE IF EXISTS $($state.database) WITH (FORCE)" *> (Join-Path $workDir 'cleanup.log')
            if ($LASTEXITCODE -ne 0) {throw 'Test database cleanup failed.'}
        }
        Remove-Item -LiteralPath $stateFile
    }
    if(!$StopInfrastructure){exit 0}
}
if ($StopInfrastructure) {
    if(Test-Path -LiteralPath $stateFile){throw 'Clean up the owned test processes before stopping infrastructure.'}
    if(Test-Path -LiteralPath $infrastructureEnv) {
        docker compose --project-name chat-test --env-file $infrastructureEnv down
        if($LASTEXITCODE -ne 0){throw 'Test infrastructure shutdown failed.'}
    }
    exit 0
}
if (!$Prepare) { throw 'Use -Prepare or -Cleanup.' }
if (Test-Path -LiteralPath $stateFile) {throw 'A test stack already exists. Run scripts/e2e.ps1 -Cleanup first.'}
$envFile=Join-Path $workDir 'stack.env'
$testIdentity=[Guid]::NewGuid().ToString('N')
$mediaBucket="chat-e2e-$testIdentity"
$testDb="chat_e2e_$testIdentity"
Initialize-TestEnvironment
$text=Get-Content -LiteralPath $infrastructureEnv -Raw
$text += "`nMEDIA_S3_BUCKET=$mediaBucket`n"
$shutdownFile=Join-Path $workDir 'maintenance.stop'
if(Test-Path -LiteralPath $shutdownFile){throw 'Owned test shutdown marker remains; inspect the interrupted maintenance before restarting.'}
$text += "`nCHAT_SHUTDOWN_FILE=$shutdownFile`n"
[IO.File]::WriteAllText($envFile,$text)
# Record resource ownership before startup side effects, including failed starts.
$state=@{database=$testDb;mediaBucket=$mediaBucket;apiUrl='http://127.0.0.1:18080';wsUrl='ws://127.0.0.1:8001/connection/websocket';origin='http://localhost:5174';apiPid=$null;workerPid=$null;envFile=$envFile}
$state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
foreach($line in $text -split "`n"){if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$'){[Environment]::SetEnvironmentVariable($Matches[1],$Matches[2].Trim(),'Process')}}
docker compose --project-name chat-test --env-file $envFile up -d --wait *> (Join-Path $workDir 'compose.log')
if($LASTEXITCODE -ne 0){throw 'Test infrastructure failed; inspect integration/compose.log.'}
docker compose --project-name chat-test --env-file $envFile exec -T postgres psql -U chat -d chat -c "CREATE DATABASE $testDb" *> (Join-Path $workDir 'database.log')
if($LASTEXITCODE -ne 0){throw 'Could not create isolated test database.'}
$text += "`nWORKER_METRICS_ADDR=127.0.0.1:18082`n"
$text=$text.Replace(':55433/chat',":55433/$testDb")
[IO.File]::WriteAllText($envFile,$text)
$env:DATABASE_URL=(($text -split "`n" | Where-Object {$_ -match '^DATABASE_URL='}) -replace '^DATABASE_URL=','').Trim()
cargo build --bins *> (Join-Path $workDir 'build.log')
if($LASTEXITCODE -ne 0){throw 'Rust build failed; inspect integration/build.log.'}
cargo run --quiet --bin admin -- media-init *> (Join-Path $workDir 'media-init.log')
if($LASTEXITCODE -ne 0){throw 'Test media bootstrap failed; inspect integration/media-init.log.'}
# Run a copy so later Cargo builds can replace target/debug/api.exe on Windows.
Copy-Item -LiteralPath (Join-Path $chatRoot 'target/debug/api.exe') -Destination (Join-Path $workDir 'api.exe') -Force
$api=Start-Process -FilePath (Join-Path $workDir 'api.exe') -WorkingDirectory $chatRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $workDir 'api.log') -RedirectStandardError (Join-Path $workDir 'api.err.log')
$state=@{database=$testDb;mediaBucket=$mediaBucket;apiUrl='http://127.0.0.1:18080';wsUrl='ws://127.0.0.1:8001/connection/websocket';origin='http://localhost:5174';apiPid=$api.Id;apiStartTicks=$api.StartTime.ToUniversalTime().Ticks.ToString();workerPid=$null;envFile=$envFile}
$state | ConvertTo-Json -Compress | Set-Content -LiteralPath $stateFile
for($attempt=0;$attempt -lt 80;$attempt++){
    try{$r=Invoke-WebRequest -Uri 'http://127.0.0.1:18080/health/ready' -TimeoutSec 2;if($r.StatusCode -eq 200){$state | ConvertTo-Json -Compress;exit 0}}catch{}
    if($api.HasExited){throw 'API exited; inspect integration/api.err.log.'}
    Start-Sleep -Milliseconds 250
}
throw 'Test API did not become ready.'
