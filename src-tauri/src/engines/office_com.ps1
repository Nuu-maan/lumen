param([string]$InputPath, [string]$OutputPath)
$ErrorActionPreference = 'Stop'
$ext = [IO.Path]::GetExtension($InputPath).ToLowerInvariant()
$app = $null
try {
    if ($ext -in '.xls', '.xlsx', '.xlsm', '.xlsb', '.xlt', '.xltx', '.xltm', '.ods', '.csv') {
        $app = New-Object -ComObject Excel.Application
        $app.DisplayAlerts = $false
        $app.AutomationSecurity = 3
        $book = $app.Workbooks.Open($InputPath, 0, $true)
        $book.ExportAsFixedFormat(0, $OutputPath)
        $book.Close($false)
    } elseif ($ext -in '.ppt', '.pptx', '.pptm', '.pps', '.ppsx', '.ppsm', '.pot', '.potx', '.odp') {
        $app = New-Object -ComObject PowerPoint.Application
        $app.AutomationSecurity = 3
        $deck = $app.Presentations.Open($InputPath, $true, $false, $false)
        $deck.SaveAs($OutputPath, 32)
        $deck.Close()
    } elseif ($ext -in '.vsd', '.vsdx', '.vsdm') {
        $app = New-Object -ComObject Visio.InvisibleApp
        $drawing = $app.Documents.OpenEx($InputPath, 2)
        $drawing.ExportAsFixedFormat(1, $OutputPath, 0, 0)
        $drawing.Close()
    } elseif ($ext -eq '.pub') {
        $app = New-Object -ComObject Publisher.Application
        $pub = $app.Open($InputPath, $true, $false)
        $pub.ExportAsFixedFormat(2, $OutputPath)
        $pub.Close()
    } else {
        $app = New-Object -ComObject Word.Application
        $app.Visible = $false
        $app.DisplayAlerts = 0
        $app.AutomationSecurity = 3
        $doc = $app.Documents.Open($InputPath, $false, $true, $false, 'lumen-no-password')
        $doc.ExportAsFixedFormat($OutputPath, 17)
        $doc.Close($false)
    }
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
} finally {
    if ($app -ne $null) { try { $app.Quit() } catch {} }
}
