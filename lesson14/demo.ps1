param([string]$BaseUrl = 'http://127.0.0.1:8081')

$ErrorActionPreference = 'Stop'
$BaseUrl = $BaseUrl.TrimEnd('/')
$httpHandler = [System.Net.Http.HttpClientHandler]::new()
$httpHandler.UseProxy = $false
$client = [System.Net.Http.HttpClient]::new($httpHandler)
$client.Timeout = [TimeSpan]::FromSeconds(10)

function Send-JsonRequest([string]$Method, [string]$Path, $Payload = $null, [string]$Key = '') {
    $message = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::new($Method), $BaseUrl + $Path)
    try {
        if ($Key -ne '') { $message.Headers.Add('Idempotency-Key', $Key) }
        if ($null -ne $Payload) {
            $json = $Payload | ConvertTo-Json -Compress
            $message.Content = [System.Net.Http.StringContent]::new($json, [Text.Encoding]::UTF8, 'application/json')
        }
        $response = $client.SendAsync($message).GetAwaiter().GetResult()
        try {
            $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            [pscustomobject]@{Status=[int]$response.StatusCode; Body=($text | ConvertFrom-Json)}
        } finally { $response.Dispose() }
    } finally { $message.Dispose() }
}

function Assert-Result($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }

try {
    $key = 'demo-' + [Guid]::NewGuid().ToString('N')
    $registration = @{name=('Optimistic-' + [Guid]::NewGuid().ToString('N').Substring(0,8)); initial_points=100}
    $created = Send-JsonRequest 'POST' '/users' $registration $key
    Assert-Result ($created.Status -eq 201 -and $created.Body.version -eq 0) 'Registration failed.'
    $path = '/users/' + $created.Body.id
    # A、B 都先读到 points=100、version=0。
    $a = Send-JsonRequest 'GET' $path
    $b = Send-JsonRequest 'GET' $path
    $first = Send-JsonRequest 'PUT' ($path+'/points') @{points=($a.Body.points+20); expected_version=$a.Body.version}
    Assert-Result ($first.Status -eq 200 -and $first.Body.points -eq 120 -and $first.Body.version -eq 1) 'First update failed.'
    $stale = Send-JsonRequest 'PUT' ($path+'/points') @{points=($b.Body.points-20); expected_version=$b.Body.version}
    Assert-Result ($stale.Status -eq 409) 'Stale update should be rejected.'
    # B 的业务意图是扣 20：重新读取后，120-20=100。
    $latest = Send-JsonRequest 'GET' $path
    $retry = Send-JsonRequest 'PUT' ($path+'/points') @{points=($latest.Body.points-20); expected_version=$latest.Body.version}
    Assert-Result ($retry.Status -eq 200) 'Recalculated retry failed.'
    $final = Send-JsonRequest 'GET' $path
    Assert-Result ($final.Body.points -eq 100 -and $final.Body.version -eq 2) 'Unexpected final account state.'
    $replay = Send-JsonRequest 'POST' '/users' $registration $key
    Assert-Result ($replay.Status -eq 201 -and $replay.Body.points -eq 100 -and $replay.Body.version -eq 0) 'Registration replay should retain its original snapshot.'
    [pscustomobject]@{
        UserID=$created.Body.id
        FirstUpdate=$first.Status
        StaleUpdate=$stale.Status
        RecalculatedRetry=$retry.Status
        FinalPoints=$final.Body.points
        FinalVersion=$final.Body.version
        RegistrationReplayVersion=$replay.Body.version
    } | ConvertTo-Json
} finally { $client.Dispose() }
