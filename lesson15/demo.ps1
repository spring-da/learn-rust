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
            $message.Content = [System.Net.Http.StringContent]::new(($Payload | ConvertTo-Json -Compress), [Text.Encoding]::UTF8, 'application/json')
        }
        $response = $client.SendAsync($message).GetAwaiter().GetResult()
        try {
            $replayed = if ($response.Headers.Contains('Idempotency-Replayed')) { $response.Headers.GetValues('Idempotency-Replayed') -join ',' } else { '' }
            [pscustomobject]@{
                Status=[int]$response.StatusCode
                Body=($response.Content.ReadAsStringAsync().GetAwaiter().GetResult() | ConvertFrom-Json)
                Replayed=$replayed
            }
        } finally { $response.Dispose() }
    } finally { $message.Dispose() }
}

function Assert-Result($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }

try {
    $suffix = [Guid]::NewGuid().ToString('N')
    $from = Send-JsonRequest 'POST' '/users' @{name=('Sender-'+$suffix.Substring(0,8)); initial_points=100} ('sender-'+$suffix)
    $to = Send-JsonRequest 'POST' '/users' @{name=('Receiver-'+$suffix.Substring(0,8)); initial_points=20} ('receiver-'+$suffix)
    Assert-Result ($from.Status -eq 201 -and $to.Status -eq 201) 'Account registration failed.'
    $key = 'transfer-' + $suffix
    $payload = @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=30}
    $first = Send-JsonRequest 'POST' '/transfers' $payload $key
    Assert-Result ($first.Status -eq 201 -and $first.Replayed -eq 'false' -and $first.Body.from_points -eq 70 -and $first.Body.to_points -eq 50) 'First transfer failed.'
    $replay = Send-JsonRequest 'POST' '/transfers' $payload $key
    Assert-Result ($replay.Status -eq 201 -and $replay.Replayed -eq 'true' -and $replay.Body.id -eq $first.Body.id) 'Transfer was not replayed.'
    $conflict = Send-JsonRequest 'POST' '/transfers' @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=31} $key
    Assert-Result ($conflict.Status -eq 409) 'Key reuse with different parameters should fail.'
    $failedKey = 'retry-' + $suffix
    $failed = Send-JsonRequest 'POST' '/transfers' @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=1000} $failedKey
    Assert-Result ($failed.Status -eq 409 -and $failed.Body.error -eq 'insufficient points') 'Insufficient balance should fail.'
    $afterFailureFrom = Send-JsonRequest 'GET' ('/users/'+$from.Body.id)
    $afterFailureTo = Send-JsonRequest 'GET' ('/users/'+$to.Body.id)
    Assert-Result ($afterFailureFrom.Body.points -eq 70 -and $afterFailureTo.Body.points -eq 50 -and $afterFailureFrom.Body.version -eq 1 -and $afterFailureTo.Body.version -eq 1) 'Failed transfer changed account state.'
    # 只有成功提交才绑定键：失败后修正金额，可以继续使用尚未绑定的键。
    $corrected = Send-JsonRequest 'POST' '/transfers' @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=10} $failedKey
    Assert-Result ($corrected.Status -eq 201 -and $corrected.Replayed -eq 'false') 'Failed request incorrectly reserved its key.'
    $finalFrom = Send-JsonRequest 'GET' ('/users/'+$from.Body.id)
    $finalTo = Send-JsonRequest 'GET' ('/users/'+$to.Body.id)
    Assert-Result ($finalFrom.Body.points -eq 60 -and $finalTo.Body.points -eq 60 -and $finalFrom.Body.version -eq 2 -and $finalTo.Body.version -eq 2) 'Unexpected final balances or versions.'
    $originalReceipt = Send-JsonRequest 'GET' ('/transfers/'+$first.Body.id)
    Assert-Result ($originalReceipt.Status -eq 200 -and $originalReceipt.Body.from_points -eq 70 -and $originalReceipt.Body.to_points -eq 50 -and $originalReceipt.Body.from_version -eq 1) 'Original receipt should remain a snapshot.'
    [pscustomobject]@{
        FromUserID=$from.Body.id; ToUserID=$to.Body.id
        FirstTransfer=$first.Status; Replay=$replay.Status; ReplayFlag=$replay.Replayed
        DifferentParameters=$conflict.Status; InsufficientPoints=$failed.Status; CorrectedRetry=$corrected.Status
        FromPoints=$finalFrom.Body.points; ToPoints=$finalTo.Body.points
        TotalPoints=($finalFrom.Body.points+$finalTo.Body.points)
        FromVersion=$finalFrom.Body.version; ToVersion=$finalTo.Body.version
        OriginalReceiptFromPoints=$originalReceipt.Body.from_points
    } | ConvertTo-Json
} finally { $client.Dispose() }
