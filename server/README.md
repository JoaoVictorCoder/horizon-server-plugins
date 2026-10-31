# Pasta do servidor

1. `cp ../dist/plugins/* plugins/` (depois de `../build.sh`).
2. `./iniciar.sh --initSettings` cria `server.properties` e `eula.txt`.
3. Aceite o EULA da Mojang (`eula=true`) e ajuste o `server.properties`.
4. `./iniciar.sh`. Comandos no próprio terminal; `stop` para desligar.

O mundo, as configs dos plugins (`plugins/<Nome>/`) e o `plugins/permissions.yml` ficam aqui e não vão para o git.
