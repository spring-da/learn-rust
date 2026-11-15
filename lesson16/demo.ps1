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
        if ($null -ne $Payload) { $message.Content = [System.Net.Http.StringContent]::new(($Payload | ConvertTo-Json -Compress), [Text.Encoding]::UTF8, 'application/json') }
        $response = $client.SendAsync($message).GetAwaiter().GetResult()
        try {
            $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
            $body = if ([string]::IsNullOrWhiteSpace($text)) { $null } else { $text | ConvertFrom-Json }
            $cache = if ($response.Headers.Contains('X-Receipt-Cache')) { $response.Headers.GetValues('X-Receipt-Cache') -join ',' } else { '' }
            [pscustomobject]@{Status=[int]$response.StatusCode; Body=$body; Cache=$cache}
        } finally { $response.Dispose() }
    } finally { $message.Dispose() }
}

function Assert-Result($Condition, [string]$Message) { if (-not $Condition) { throw $Message } }

try {
    $before = Send-JsonRequest 'GET' '/cache/stats'
    Assert-Result ($before.Status -eq 200 -and $before.Body.capacity -eq 2 -and $before.Body.ttl_ms -ge 500 -and $before.Body.ttl_ms -le 5000) 'Start the lesson with CACHE_CAPACITY=2 and CACHE_TTL_MS=2000 before running this demo.'
    $suffix = [Guid]::NewGuid().ToString('N')
    $from = Send-JsonRequest 'POST' '/users' @{name=('CacheSender-'+$suffix.Substring(0,8)); initial_points=100} ('sender-'+$suffix)
    $to = Send-JsonRequest 'POST' '/users' @{name=('CacheReceiver-'+$suffix.Substring(0,8)); initial_points=20} ('receiver-'+$suffix)
    Assert-Result ($from.Status -eq 201 -and $to.Status -eq 201) 'Registration failed.'
    $payload = @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=30}
    $firstTransfer = Send-JsonRequest 'POST' '/transfers' $payload ('first-'+$suffix)
    Assert-Result ($firstTransfer.Status -eq 201) 'Transfer failed.'
    $path = '/transfers/' + $firstTransfer.Body.id
    $miss = Send-JsonRequest 'GET' $path
    $hit = Send-JsonRequest 'GET' $path
    Assert-Result ($miss.Cache -eq 'MISS' -and $hit.Cache -eq 'HIT') 'Expected a database miss followed by a cache hit.'
    $secondTransfer = Send-JsonRequest 'POST' '/transfers' @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=10} ('second-'+$suffix)
    Assert-Result ($secondTransfer.Status -eq 201) 'Second transfer failed.'
    $snapshot = Send-JsonRequest 'GET' $path
    $current = Send-JsonRequest 'GET' ('/users/'+$from.Body.id)
    Assert-Result ($snapshot.Cache -eq 'HIT' -and $snapshot.Body.from_points -eq 70 -and $current.Body.points -eq 60 -and $current.Cache -eq '') 'Receipt snapshot was confused with current account state.'
    $deleted = Send-JsonRequest 'DELETE' ('/cache/receipts/'+$firstTransfer.Body.id)
    Assert-Result ($deleted.Status -eq 204) 'Cache deletion failed.'
    $afterDelete = Send-JsonRequest 'GET' $path
    Assert-Result ($afterDelete.Cache -eq 'MISS') 'Manual deletion did not force a database read.'
    Start-Sleep -Milliseconds ([int]$before.Body.ttl_ms + 100)
    $afterExpiry = Send-JsonRequest 'GET' $path
    Assert-Result ($afterExpiry.Cache -eq 'MISS') 'Expired entry was returned.'
    $secondRead = Send-JsonRequest 'GET' ('/transfers/'+$secondTransfer.Body.id)
    Assert-Result ($secondRead.Status -eq 200) 'Second receipt read failed.'
    $thirdTransfer = Send-JsonRequest 'POST' '/transfers' @{from_user_id=$from.Body.id; to_user_id=$to.Body.id; amount=1} ('third-'+$suffix)
    Assert-Result ($thirdTransfer.Status -eq 201) 'Third transfer failed.'
    $thirdRead = Send-JsonRequest 'GET' ('/transfers/'+$thirdTransfer.Body.id)
    Assert-Result ($thirdRead.Status -eq 200) 'Third receipt read failed.'
    $afterEviction = Send-JsonRequest 'GET' $path
    Assert-Result ($afterEviction.Cache -eq 'MISS') 'FIFO did not evict the oldest receipt.'
    $stats = Send-JsonRequest 'GET' '/cache/stats'
    Assert-Result ($stats.Body.entries -le 2 -and $stats.Body.evictions -gt $before.Body.evictions -and $stats.Body.expirations -gt $before.Body.expirations) 'Cache bounds or expiry statistics are incorrect.'
    $final = Send-JsonRequest 'GET' ('/users/'+$from.Body.id)
    Assert-Result ($final.Body.points -eq 59) 'User balance should come from the current database state.'
    [pscustomobject]@{
        ReceiptID=$firstTransfer.Body.id; FromUserID=$from.Body.id; ToUserID=$to.Body.id
        FirstRead=$miss.Cache; RepeatRead=$hit.Cache; AfterBalanceChange=$snapshot.Cache
        AfterManualDelete=$afterDelete.Cache; AfterTTL=$afterExpiry.Cache; AfterFIFOEviction=$afterEviction.Cache
        Entries=$stats.Body.entries; Capacity=$stats.Body.capacity
        DatabaseLoads=$stats.Body.database_loads; Hits=$stats.Body.hits
        Expirations=$stats.Body.expirations; Evictions=$stats.Body.evictions
        OriginalReceiptFromPoints=$afterEviction.Body.from_points; CurrentFromPoints=$final.Body.points
    } | ConvertTo-Json
} finally { $client.Dispose() }
