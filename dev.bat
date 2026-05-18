@echo off
setlocal enabledelayedexpansion
title UniversalConverter v1.5.0 - Dev
cd /d "%~dp0"

:: ── Dossier logs ─────────────────────────────────────────────────────────────
if not exist ".logs" mkdir ".logs"
if not exist ".logs\archive" mkdir ".logs\archive"

:: Rotation si le log depasse 1 MB
if exist ".logs\dev.log" (
    for %%F in (".logs\dev.log") do set "FSIZE=%%~zF"
    if !FSIZE! GTR 1048576 (
        for /f "tokens=*" %%D in ('powershell -NoProfile -Command "Get-Date -Format 'yyyy-MM-dd_HHmm'"') do (
            move ".logs\dev.log" ".logs\archive\dev_%%D.log" >nul
        )
    )
)

:: ── Prereqs ──────────────────────────────────────────────────────────────────
where npm   >nul 2>&1 || (echo [ERREUR] npm introuvable. Installer Node.js. & pause & exit /b 1)
where cargo >nul 2>&1 || (echo [ERREUR] cargo introuvable. Installer Rust.   & pause & exit /b 1)

:: ── Kill instance precedente ─────────────────────────────────────────────────
taskkill /F /IM universalconverter.exe >nul 2>&1

:: ── Variables d'environnement Tauri / Rust ───────────────────────────────────
set "RUST_LOG=info,universalconverter=debug"
set "RUST_BACKTRACE=1"

:: ── Header ───────────────────────────────────────────────────────────────────
echo.
echo  ================================================
echo   UniversalConverter v1.5.0  ^|  Mode DEV
echo  ================================================
echo   Logs en temps reel : .logs\dev.log
echo   Arreter            : Ctrl+C
echo  ================================================
echo.

:: ── Lancement avec tee temps reel ────────────────────────────────────────────
:: [string]$_ convertit les ErrorRecord PS en string propre sans guillemets internes
powershell -NoProfile -Command "$ErrorActionPreference='Continue'; npm run tauri dev 2>&1 | ForEach-Object { [string]$_ } | Tee-Object -FilePath '.logs\dev.log' -Append"

set "EXIT_CODE=%ERRORLEVEL%"

:: ── Pied de log (single quotes PS uniquement, pas de guillemets imbriques) ───
powershell -NoProfile -Command "Add-Content -Path '.logs\dev.log' -Value ('[' + (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss') + '] [INFO] Processus termine - code %EXIT_CODE%')"

echo.
if %EXIT_CODE% NEQ 0 (
    echo  [ERREUR] Processus termine avec le code %EXIT_CODE%
    echo  Consultez .logs\dev.log pour les details.
) else (
    echo  [OK] Processus termine proprement.
)
echo.
pause
exit /b %EXIT_CODE%
