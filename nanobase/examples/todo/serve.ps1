# A tiny static file server, so the todo page is served over HTTP.
#
# The page talks to the Nanobase API from another origin, so it cannot be
# opened straight from the disk: a browser will not let a file:// page
# read an http:// answer, and the API would need to allow an origin that
# means "any file on this machine". Serving it from a port is the honest
# way, and this is small enough to read before running.
#
#   powershell -ExecutionPolicy Bypass -File serve.ps1
#   powershell -ExecutionPolicy Bypass -File serve.ps1 -Port 9000
#
# Then open http://localhost:8000/ in a browser.

param(
    [int]$Port = 8000,
    [string]$Root = $PSScriptRoot
)

$ErrorActionPreference = "Stop"

$listener = New-Object System.Net.HttpListener
$listener.Prefixes.Add("http://localhost:$Port/")
$listener.Prefixes.Add("http://127.0.0.1:$Port/")
$listener.Start()

Write-Host "Serving $Root on http://localhost:$Port/  (Ctrl-C to stop)"

$mime = @{
    ".html" = "text/html; charset=utf-8"
    ".css"  = "text/css; charset=utf-8"
    ".js"   = "text/javascript; charset=utf-8"
    ".json" = "application/json"
    ".svg"  = "image/svg+xml"
}

try {
    while ($listener.IsListening) {
        $context = $listener.GetContext()
        $request = $context.Request
        $response = $context.Response

        $relative = $request.Url.AbsolutePath.TrimStart("/")
        if ([string]::IsNullOrEmpty($relative)) { $relative = "index.html" }

        # Keep the path inside the folder: no "..", no absolute paths.
        $full = [System.IO.Path]::GetFullPath((Join-Path $Root $relative))
        if (-not $full.StartsWith([System.IO.Path]::GetFullPath($Root))) {
            $response.StatusCode = 403
            $response.Close()
            continue
        }
        if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
            $response.StatusCode = 404
            $response.Close()
            continue
        }

        $extension = [System.IO.Path]::GetExtension($full).ToLowerInvariant()
        $response.ContentType = if ($mime.ContainsKey($extension)) { $mime[$extension] } else { "application/octet-stream" }
        $bytes = [System.IO.File]::ReadAllBytes($full)
        $response.ContentLength64 = $bytes.Length
        $response.OutputStream.Write($bytes, 0, $bytes.Length)
        $response.Close()
    }
}
finally {
    $listener.Stop()
    $listener.Close()
}
