# Captura a janela do Orvalith (apenas o retângulo da janela) para um PNG.
# Uso: .\scripts\screenshot.ps1 -Out docs\design\home-depois.png [-Width 1280 -Height 720] [-Exe caminho]
# Usa PrintWindow: não captura a tela inteira. A janela é aberta, redimensionada, capturada e fechada. Requer sessão gráfica interativa.
param(
    [string]$Out = 'screenshot.png',
    [int]$Width = 1280,
    [int]$Height = 720,
    [string]$Exe = 'target\release\orvalith.exe',
    [int]$WaitSeconds = 5,
    # Preferências (config.json) a usar; a captura roda com um APPDATA isolado e nunca toca nas preferências reais.
    [string]$Config = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System; using System.Runtime.InteropServices;
public class W {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr h, int x, int y, int w, int hh, bool repaint);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
}
'@
[void][W]::SetProcessDPIAware()
$iso = Join-Path ([IO.Path]::GetTempPath()) ('orvalith-shot-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force (Join-Path $iso 'Orvalith') | Out-Null
if ($Config) { Copy-Item $Config (Join-Path $iso 'Orvalith\config.json') }
$env:APPDATA = $iso
$p = Start-Process -FilePath $Exe -PassThru
try {
    Start-Sleep -Seconds $WaitSeconds
    $p.Refresh()
    $h = $p.MainWindowHandle
    if ($h -eq [IntPtr]::Zero) { throw 'janela não encontrada' }
    [void][W]::MoveWindow($h, 40, 40, $Width, $Height, $true)
    [void][W]::SetForegroundWindow($h)
    Start-Sleep -Seconds 2
    $r = New-Object W+RECT
    [void][W]::GetWindowRect($h, [ref]$r)
    $w = $r.R - $r.L; $hh = $r.B - $r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    # PrintWindow lê o conteúdo da própria janela (nunca de outras janelas na frente dela).
    $hdc = $g.GetHdc()
    $ok = [W]::PrintWindow($h, $hdc, 2)
    $g.ReleaseHdc($hdc)
    if (-not $ok) { throw 'PrintWindow falhou' }
    $dir = Split-Path $Out -Parent
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force $dir | Out-Null }
    $bmp.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
    Write-Host "Salvo: $Out (${w}x${hh})"
} finally {
    Remove-Item -Recurse -Force $iso -ErrorAction SilentlyContinue
    if (-not $p.HasExited) { $p.CloseMainWindow() | Out-Null; Start-Sleep -Seconds 1; if (-not $p.HasExited) { $p.Kill() } }
}
