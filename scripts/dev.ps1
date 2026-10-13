param([switch]$InitEnv, [switch]$Up,[switch]$Start,[switch]$Stop)
$ErrorActionPreference = 'Stop'
$chatRoot = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot
$runtime=Join-Path $chatRoot '.superpowers/dev'
$stateFile=Join-Path $runtime 'processes.json'
if($Stop){
    if(Test-Path -LiteralPath $stateFile){$state=Get-Content -LiteralPath $stateFile -Raw|ConvertFrom-Json;foreach($owned in $state){$proc=Get-Process -Id $owned.pid -ErrorAction SilentlyContinue;if($proc -and $proc.Path -eq $owned.path -and $proc.StartTime.ToUniversalTime().Ticks.ToString() -eq $owned.started){Stop-Process -Id $owned.pid -Force}};Remove-Item -LiteralPath $stateFile}
    if(!$Start -and !$Up -and !$InitEnv){exit 0}
}
if ($InitEnv) {
    if (Test-Path -LiteralPath '.env') { throw '.env already exists; keep existing credentials.' }
    function New-LocalSecret { $bytes = New-Object byte[] 32; [Security.Cryptography.RandomNumberGenerator]::Fill($bytes); [Convert]::ToHexString($bytes).ToLowerInvariant() }
    $dbPassword = New-LocalSecret
    $envText = Get-Content -LiteralPath '.env.example' -Raw
    $envText = $envText.Replace('postgres://chat:CHANGE_ME', "postgres://chat:$dbPassword").Replace('POSTGRES_PASSWORD=CHANGE_ME', "POSTGRES_PASSWORD=$dbPassword")
    foreach ($key in @('CENTRIFUGO_API_KEY', 'INTERNAL_PROXY_KEY', 'SEAWEED_SECRET_KEY')) { $envText = $envText.Replace("$key=CHANGE_ME", "$key=$(New-LocalSecret)") }
    [IO.File]::WriteAllText((Join-Path $chatRoot '.env'), $envText)
    $secret = ($envText -split "`n" | Where-Object { $_ -match '^SEAWEED_SECRET_KEY=' }) -replace '^SEAWEED_SECRET_KEY=', ''
    New-Item -ItemType Directory -Path (Join-Path $chatRoot 'infra/seaweedfs') -Force | Out-Null
    $s3 = @{identities = @(@{name='chat';credentials=@(@{accessKey='chat-local';secretKey=$secret.Trim()});actions=@('Admin','Read','Write','List','Tagging')})} | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText((Join-Path $chatRoot 'infra/seaweedfs/s3.json'), $s3)
    Write-Host 'Generated local credentials in .env and SeaweedFS config.'
}
if ($Up) {
    docker compose up -d --wait
    if ($LASTEXITCODE -ne 0) { throw 'Infrastructure startup failed.' }
}
if($Start){
    if(Test-Path -LiteralPath $stateFile){throw 'Developer processes already recorded. Use -Stop before starting again.'}
    if(!(Test-Path -LiteralPath '.env') -or !(Test-Path -LiteralPath 'web/node_modules')){throw 'Initialize .env, infrastructure and web dependencies first.'}
    New-Item -ItemType Directory -Path $runtime -Force|Out-Null
    cargo build --bins
    if($LASTEXITCODE -ne 0){throw 'Rust build failed.'}
    $env:CHAT_SHUTDOWN_FILE=Join-Path $runtime 'maintenance.stop'
    if(Test-Path -LiteralPath $env:CHAT_SHUTDOWN_FILE){throw 'Maintenance marker remains; inspect interrupted maintenance before starting.'}
    cargo run --quiet --bin admin -- media-init
    if($LASTEXITCODE -ne 0){Write-Warning 'Media bootstrap failed; text chat can start. Retry cargo run --bin admin -- media-init after storage recovers.'}
    $owned=@()
    try{
        foreach($name in @('api','worker')){
            $binary=Join-Path $runtime "$name.exe";Copy-Item -LiteralPath (Join-Path $chatRoot "target/debug/$name.exe") -Destination $binary -Force
            $proc=Start-Process -FilePath $binary -WorkingDirectory $chatRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runtime "$name.log") -RedirectStandardError (Join-Path $runtime "$name.err.log")
            $owned+=@{pid=$proc.Id;path=$proc.Path;started=$proc.StartTime.ToUniversalTime().Ticks.ToString()}
        }
        $node=(Get-Command node).Source
        $proc=Start-Process -FilePath $node -ArgumentList @((Join-Path $chatRoot 'web/node_modules/vite/bin/vite.js'),'--host','localhost','--port','5173','--strictPort') -WorkingDirectory (Join-Path $chatRoot 'web') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runtime 'web.log') -RedirectStandardError (Join-Path $runtime 'web.err.log')
        $owned+=@{pid=$proc.Id;path=$proc.Path;started=$proc.StartTime.ToUniversalTime().Ticks.ToString()}
        $owned|ConvertTo-Json -Depth 4|Set-Content -LiteralPath $stateFile
        for($attempt=0;$attempt -lt 60;$attempt++){foreach($process in $owned){if(!(Get-Process -Id $process.pid -ErrorAction SilentlyContinue)){throw 'A developer process exited; inspect .superpowers/dev logs.'}};try{if((Invoke-WebRequest -Uri 'http://127.0.0.1:8080/health/ready' -TimeoutSec 2).StatusCode -eq 200 -and (Invoke-WebRequest -Uri 'http://localhost:5173' -TimeoutSec 2).StatusCode -eq 200){Write-Output 'Developer app ready at http://localhost:5173';exit 0}}catch{};Start-Sleep -Milliseconds 250}
        throw 'Developer processes did not become ready; inspect .superpowers/dev logs.'
    }catch{foreach($process in $owned){$proc=Get-Process -Id $process.pid -ErrorAction SilentlyContinue;if($proc -and $proc.Path -eq $process.path){Stop-Process -Id $process.pid -Force}};if(Test-Path -LiteralPath $stateFile){Remove-Item -LiteralPath $stateFile};throw}
}
