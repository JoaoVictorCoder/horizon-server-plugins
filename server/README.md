# Server folder

1. `cp ../dist/plugins/* plugins/` (after `../build.sh`).
2. `./iniciar.sh --initSettings` writes `server.properties` and `eula.txt`.
3. Accept Mojang's EULA (`eula=true`) and adjust `server.properties`.
4. `./iniciar.sh` starts the server. Type commands in the terminal; `stop` shuts it down.

The world, the plugins' settings (`plugins/<Name>/`) and `plugins/permissions.yml` live here and stay out of git.
