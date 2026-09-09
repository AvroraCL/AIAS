@echo off
rem AIAS 开发调试一键启动：自动配置构建环境，双击即可运行未打包版本
setlocal
cd /d "%~dp0"

set "NODE_DIR=C:\Program Files\nodejs"
set "MINGW64=%LOCALAPPDATA%\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin"
set "CARGO_BIN=%USERPROFILE%\.cargo\bin"
set "PATH=%NODE_DIR%;%MINGW64%;%CARGO_BIN%;%PATH%"

if not exist "%NODE_DIR%\node.exe" (
  echo [错误] 未找到 node.exe：%NODE_DIR%
  pause & exit /b 1
)

netstat -ano | findstr ":5183" | findstr "LISTENING" >nul && echo [提示] 端口 5183 已被占用，若已有 dev 在跑请先关掉旧窗口。

echo [1/2] 启动前端 vite (127.0.0.1:5183)，窗口标题 AIAS-vite...
start "AIAS-vite" cmd /k ""%NODE_DIR%\node.exe" node_modules\vite\bin\vite.js --host 127.0.0.1"

echo [2/2] 启动 tauri dev（首次 debug 编译需几分钟，之后增量很快）...
node node_modules\@tauri-apps\cli\tauri.js dev --config "{""build"":{""beforeDevCommand"":""""}}"

echo.
echo tauri dev 已退出，如 AIAS-vite 窗口还开着请手动关闭。
pause
