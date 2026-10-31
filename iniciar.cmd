@echo off
rem Kal'tsit: asistente de escritorio.
rem La app decide si este equipo es su cerebro y entonces arranca el modelo (data\local.json -> llama).
rem Si otro equipo de la red ya es el cerebro, este se une como terminal y no gasta VRAM.
cd /d "%~dp0"
rem Siempre recompila: si no hay cambios tarda un segundo.
cargo build --release || (pause & exit /b 1)

rem Libera la VRAM de Ollama por si este equipo tiene que pensar.
ollama stop lappland-4b >nul 2>&1

start "" target\release\kaltsit.exe %*
