param([switch]$Prepare)

$ErrorActionPreference='Stop'
if(!$Prepare){throw 'Use -Prepare to check an already deployed staging application.'}

$origin=$env:CHAT_DEPLOYMENT_ORIGIN
$mediaOrigin=$env:CHAT_DEPLOYMENT_MEDIA_ORIGIN
foreach($value in @($origin,$mediaOrigin)) {
    [Uri]$address=$null
    if([string]::IsNullOrWhiteSpace($value) -or
        ![Uri]::TryCreate([string]$value,[UriKind]::Absolute,[ref]$address) -or
        $address.Scheme -ne 'https' -or $address.UserInfo -or
        $address.AbsolutePath -ne '/' -or $address.Query -or $address.Fragment) {
        throw 'Set CHAT_DEPLOYMENT_ORIGIN and CHAT_DEPLOYMENT_MEDIA_ORIGIN to HTTPS origins.'
    }
}
if($origin.TrimEnd('/') -eq $mediaOrigin.TrimEnd('/')) {
    throw 'Staging media must use a separate origin.'
}

# Only an explicitly configured local staging CA may bypass certificate validation.
$insecure=$env:CHAT_DEPLOYMENT_INSECURE_TLS -eq 'true'
$response=Invoke-WebRequest -Uri "$($origin.TrimEnd('/'))/health/ready" -TimeoutSec 10 -SkipCertificateCheck:$insecure
if($response.StatusCode -ne 200){throw 'Staging application is not ready.'}
Write-Output 'Staging application is ready. Run pnpm -C web test:deployment with two staging invitation tokens.'
