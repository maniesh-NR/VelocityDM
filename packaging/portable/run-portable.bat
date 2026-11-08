@echo off
rem VelocityDM portable launcher (Windows).
rem Keeps all state next to the binaries: .\velocitydm-data\
set DIR=%~dp0
"%DIR%velocitydm.exe" --portable daemon
