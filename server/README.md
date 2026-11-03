# Server folder

1. `cp ../dist/plugins/* plugins/` (after `../build.sh`).
2. `./iniciar.sh --initSettings` writes `server.properties` and `eula.txt`.
3. Accept Mojang's EULA (`eula=true`) and adjust `server.properties`.
4. `./iniciar.sh` starts the server. Type commands in the terminal; `stop` shuts it down.

On Windows, `.\iniciar.ps1` in PowerShell does what `./iniciar.sh` does (build with `./build.sh` in Git Bash, with
MinGW-w64's `mingw64\bin` on PATH).

The world, the plugins' settings (`plugins/<Name>/`) and `plugins/permissions.yml` live here and stay out of git.
