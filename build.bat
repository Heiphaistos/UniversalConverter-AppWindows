@echo off
setlocal enabledelayedexpansion
title UniversalConverter v1.6.0 - Build Release
cd /d "%~dp0"

:: ── Dossier logs ─────────────────────────────────────────────────────────────
if not exist ".logs" mkdir ".logs"
if not exist ".logs\archive" mkdir ".logs\archive"

:: Rotation si le log depasse 1 MB
if exist ".logs\build.log" (
    for %%F in (".logs\build.log") do set "FSIZE=%%~zF"
    if !FSIZE! GTR 1048576 (
        for /f "tokens=*" %%D in ('powershell -NoProfile -Command "Get-Date -Format 'yyyy-MM-dd_HHmm'"') do (
            move ".logs\build.log" ".logs\archive\build_%%D.log" >nul
        )
    )
)

echo.
echo  ================================================
echo   UniversalConverter v1.6.0  ^|  Build Release
echo  ================================================
echo   Logs : .logs\build.log
echo  ================================================
echo.

:: ── 1. Prereqs ───────────────────────────────────────────────────────────────
echo [1/5] Verification des outils...
where npm   >nul 2>&1 || (echo [ERREUR] npm introuvable.   & pause & exit /b 1)
where cargo >nul 2>&1 || (echo [ERREUR] cargo introuvable. & pause & exit /b 1)
echo       OK

:: ── 2. Kill ──────────────────────────────────────────────────────────────────
echo [2/5] Arret des processus en cours...
taskkill /F /IM universalconverter.exe >nul 2>&1
echo       OK

:: ── 3. Clean ─────────────────────────────────────────────────────────────────
echo [3/5] Nettoyage des artefacts...
if exist dist rmdir /s /q dist
echo       OK

:: ── 4. Build avec logs temps reel ────────────────────────────────────────────
echo [4/5] Compilation (peut prendre plusieurs minutes)...
echo.
powershell -NoProfile -Command "$ErrorActionPreference='Continue'; npm run tauri build 2>&1 | ForEach-Object { [string]$_ } | Tee-Object -FilePath '.logs\build.log' -Append"
set "BUILD_CODE=%ERRORLEVEL%"

:: ── 5. Resultat ──────────────────────────────────────────────────────────────
echo.
echo [5/5] Verification...
if %BUILD_CODE% NEQ 0 (
    echo.
    echo  [ERREUR] Build echoue - code %BUILD_CODE%
    echo  Consultez .logs\build.log pour les details.
) else (
    echo       OK
    echo.
    echo  [OK] Build reussi !
    echo  Exe    : src-tauri\target\release\universalconverter.exe
    echo  Bundle : src-tauri\target\release\bundle\
    echo  Log    : .logs\build.log
)

powershell -NoProfile -Command "Add-Content -Path '.logs\build.log' -Value ('[' + (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss') + '] [INFO] Build termine - code %BUILD_CODE%')"

echo.
pause
exit /b %BUILD_CODE%
