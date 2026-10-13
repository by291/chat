param([Parameter(Mandatory)][string]$OutputDirectory,[string]$ProjectName='chat-dev',[string]$EnvFile='.env',[string]$ComposeFile='compose.yaml',[string]$DatabaseName,[string]$ProcessStateFile)
$ErrorActionPreference='Stop'
$chatRoot=Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $chatRoot
if($ProjectName -notmatch '^[a-z0-9][a-z0-9-]+$'){throw 'Invalid project name.'}
$sourceEnv=(Resolve-Path -LiteralPath $EnvFile).Path
$values=@{};foreach($line in Get-Content -LiteralPath $sourceEnv){if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$'){$values[$Matches[1]]=$Matches[2].Trim()}}
$storageConfig=if($values.SEAWEED_S3_CONFIG){$values.SEAWEED_S3_CONFIG}else{Join-Path $chatRoot 'infra/seaweedfs/s3.json'}
$storageConfig=(Resolve-Path -LiteralPath $storageConfig).Path
if(!$ProcessStateFile -and $ProjectName -eq 'chat-dev') {
    $localProcesses=Join-Path $chatRoot '.superpowers/dev/processes.json'
    if(Test-Path -LiteralPath $localProcesses){$ProcessStateFile=$localProcesses}
}
$dbUser=if($values.POSTGRES_USER){$values.POSTGRES_USER}else{'chat'}
if(!$DatabaseName){$DatabaseName=if($values.POSTGRES_DB){$values.POSTGRES_DB}else{'chat'}}
if($DatabaseName -notmatch '^[a-zA-Z0-9_]+$' -or $dbUser -notmatch '^[a-zA-Z0-9_]+$'){throw 'Invalid database identifier.'}
$output=[IO.Path]::GetFullPath($OutputDirectory)
if((Test-Path -LiteralPath $output) -and (Get-ChildItem -LiteralPath $output -Force|Select-Object -First 1)){throw 'Backup destination must be empty.'}
New-Item -ItemType Directory -Path $output -Force|Out-Null
$composeArgs=@('compose','--project-name',$ProjectName,'--env-file',$sourceEnv,'--file',$ComposeFile)
. (Join-Path $PSScriptRoot 'backup-writers.ps1')
$writers=New-BackupWriterState $composeArgs $ProcessStateFile $values
$seaweedId=(& docker @composeArgs ps -q seaweedfs).Trim()
if(!$seaweedId){throw 'SeaweedFS must be running before this backup.'}
$storageStopped=$false
try {
    Stop-BackupWriters $writers $composeArgs

    $dump="/tmp/chat-backup-$([guid]::NewGuid().ToString('N')).dump"
    & docker @composeArgs exec -T postgres pg_dump -U $dbUser -d $DatabaseName -Fc -f $dump
    if($LASTEXITCODE -ne 0){throw 'PostgreSQL backup failed.'}
    & docker @composeArgs cp "postgres:$dump" (Join-Path $output 'postgres.dump')
    if($LASTEXITCODE -ne 0){throw 'Backup copy failed.'}
    & docker @composeArgs exec -T postgres rm $dump
    if($LASTEXITCODE -ne 0){throw 'Temporary backup removal failed.'}

    # All writers and public PUT ingress are quiescent before either snapshot starts.
    & docker @composeArgs stop seaweedfs;if($LASTEXITCODE -ne 0){throw 'SeaweedFS stop failed.'}
    $storageStopped=$true
    docker run --rm --volumes-from "${seaweedId}:ro" --mount "type=bind,source=$output,target=/backup" debian:bookworm-slim tar -cf /backup/seaweed.tar -C /data .
    if($LASTEXITCODE -ne 0){throw 'SeaweedFS backup failed.'}
}finally{
    try {
        if($storageStopped){& docker @composeArgs up -d --no-recreate --wait --wait-timeout 60 seaweedfs;if($LASTEXITCODE -ne 0){throw 'SeaweedFS restart failed.'}}
    } finally {
        Start-BackupWriters $writers $composeArgs
    }
}
Copy-Item -LiteralPath $sourceEnv -Destination (Join-Path $output 'configuration.env')
Copy-Item -LiteralPath $storageConfig -Destination (Join-Path $output 'seaweed-s3.json')
$hashes=@{};foreach($name in @('postgres.dump','seaweed.tar','configuration.env','seaweed-s3.json')){$hashes[$name]=(Get-FileHash -LiteralPath (Join-Path $output $name) -Algorithm SHA256).Hash}
@{schema=1;createdUtc=[DateTime]::UtcNow.ToString('O');database=$DatabaseName;dbUser=$dbUser;sourceProject=$ProjectName;hashes=$hashes;phase=2;coordinatedWriters=$true}|ConvertTo-Json -Depth 5|Set-Content -LiteralPath (Join-Path $output 'manifest.json')
Write-Output "Backup saved to $output. Protect this directory: it includes private data and credentials."
