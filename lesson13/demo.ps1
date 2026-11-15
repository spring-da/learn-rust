param(
    [string]$BaseUrl = 'http://127.0.0.1:8081',
    [ValidateRange(500,10000)][int]$DelayMs = 2000,
    [string]$Name = ('Shutdown-' + [Guid]::NewGuid().ToString('N').Substring(0,8)),
    [string]$Key = ('demo-' + [Guid]::NewGuid().ToString('N'))
)

$ErrorActionPreference = 'Stop'
$BaseUrl = $BaseUrl.TrimEnd('/')
$null = Invoke-RestMethod -Uri ($BaseUrl + '/readyz') -TimeoutSec 3
$handler = [System.Net.Http.HttpClientHandler]::new()
$handler.UseProxy = $false
$client = [System.Net.Http.HttpClient]::new($handler)
$client.Timeout = [TimeSpan]::FromSeconds(15)
$request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, $BaseUrl + '/users')
$request.Headers.Add('Idempotency-Key', $Key)
$request.Headers.Add('X-Demo-Delay-Ms', $DelayMs.ToString())
$payload = @{name=$Name; initial_points=100} | ConvertTo-Json -Compress
$request.Content = [System.Net.Http.StringContent]::new($payload, [Text.Encoding]::UTF8, 'application/json')
try {
    Write-Output ('DEMO_NAME=' + $Name)
    Write-Output ('DEMO_KEY=' + $Key)
    $pending = $client.SendAsync($request)
    $observed = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $state = Invoke-RestMethod -Uri ($BaseUrl + '/demo/state') -TimeoutSec 2
        if ($state.active_requests -gt 0) { $observed = $true; break }
        if ($pending.IsCompleted) { throw 'Request completed before its active state was observed; increase DelayMs.' }
        Start-Sleep -Milliseconds 20
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $observed) { throw 'No active request observed.' }
    Write-Output ('ACTIVE_BEFORE_SHUTDOWN=' + $state.active_requests)
    $shutdown = $client.PostAsync($BaseUrl + '/demo/shutdown', [System.Net.Http.StringContent]::new('')).GetAwaiter().GetResult()
    try {
        Write-Output ('SHUTDOWN_STATUS=' + [int]$shutdown.StatusCode)
        if ([int]$shutdown.StatusCode -ne 202) { throw 'Shutdown request was not accepted.' }
    } finally { $shutdown.Dispose() }
    try {
        $response = $pending.GetAwaiter().GetResult()
        try {
            Write-Output ('REGISTRATION_STATUS=' + [int]$response.StatusCode)
            Write-Output ('REGISTRATION_BODY=' + $response.Content.ReadAsStringAsync().GetAwaiter().GetResult())
        } finally { $response.Dispose() }
    } catch {
        # 强制关闭连接时客户端可能收到连接错误；这是需要观察的演示结果。
        Write-Output ('REGISTRATION_ERROR=' + $_.Exception.Message)
    }
} finally {
    $request.Dispose()
    $client.Dispose()
}
