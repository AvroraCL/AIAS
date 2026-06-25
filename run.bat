@echo off
echo AIAS Electron v4.1.0
echo.

if not exist "node_modules\" (
    echo [1/3] Installing dependencies...
    call npm install
    if errorlevel 1 (
        echo npm install failed.
        pause
        exit /b 1
    )
)

echo [2/3] Cleaning port 5173...
for /f "tokens=5" %%a in ('netstat -ano ^| findstr ":5173.*LISTENING"') do (
    taskkill /F /PID %%a >nul 2>&1
    echo Killed PID %%a on port 5173
)

echo [3/3] Starting AIAS...
call npm run dev
pause
