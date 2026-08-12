@echo off
setlocal
for %%I in ("%~dp0..") do set "PAPERTIGER_ROOT=%%~fI"
if not defined PAPERTIGER_DB set "PAPERTIGER_DB=%PAPERTIGER_ROOT%\state\papertiger.sqlite"
set "PAPERTIGER_EXE=%PAPERTIGER_ROOT%\tools\papertiger\bin\papertiger.exe"
if not exist "%PAPERTIGER_EXE%" (
  >&2 echo papertiger is not installed at %PAPERTIGER_ROOT%\tools\papertiger
  >&2 echo Install a release with: papertiger setup-project "%PAPERTIGER_ROOT%"
  >&2 echo Releases: https://github.com/virtualonno/papertiger/releases
  exit /b 2
)
"%PAPERTIGER_EXE%" %*
exit /b %ERRORLEVEL%
