# Horizon Server Plugins

Servidor de **Minecraft: Java Edition 26.3** em Rust com suporte a **plugins no estilo Paper**, construído em cima
do [Horizon](https://github.com/horizon-rust/horizon), de **Ludovic Marie e contribuidores**.

> Este repositório tem **só a parte do servidor**: a API de plugins, o host que carrega `plugins/`, os plugins de
> exemplo e um patch pequeno no motor do Horizon. O jogo, o motor do servidor e todo o resto são do Horizon
> ([créditos](CREDITS.md)). Bugs do próprio Horizon vão para o [repositório deles](https://github.com/horizon-rust/horizon/issues).

## O que tem

- **Plugins em Rust** com cara de Paper: `plugin.yml`, `on_enable`/`on_disable`, eventos com prioridade e
  cancelamento, comandos com permissão e tab-complete, scheduler em ticks, `config.yml`, cores `&a`/`§a`.
- **`plugins/permissions.yml`**: grupos, herança, `-permissao` para negar, curingas `homes.*`.
- **`/plugins`** e **`/plugins reload`**: recarrega plugins e configs sem reiniciar.
- **Eventos**: entrar/sair (com a mensagem), mover, morrer, chat, cliques, quebrar/colocar blocos, baldes,
  ataques, spawn e morte de mobs, explosões, líquidos e fogo, pistões, menus de baú, ticks.
- **Plugins de exemplo** (`api/examples/`):
  - **Homes**: `/sethome`, `/home`, `/delhome`, `/homes` (menu), proteção do spawn.
  - **Dragão**: `/dragao [cor]` constrói um dragãozinho de blocos.
  - **HorizonEdit**: os comandos do WorldEdit (`//wand`, `//set`, `//replace`, `//copy`, `//paste`, `//rotate`,
    `//undo`, `//sphere`, `//stack`...), escritos do zero.

Documentação completa da API: [docs/plugins.md](docs/plugins.md).

Os `.jar` do Paper **não** rodam aqui: eles são bytecode Java feito para as classes do Bukkit. Um plugin do
Paper se porta para esta API quase linha por linha (veja a tabela em `docs/plugins.md`).

## Compilar o servidor

Precisa de [Rust](https://rustup.rs), `curl`, `tar`, `git` e um compilador C.

```sh
./build.sh
```

O script baixa o Horizon no commit de `engine/HORIZON_COMMIT`, junta as crates de plugins, aplica
`engine/horizon.patch` e compila só o servidor (sem som nem controle). Resultado:

- `dist/horizon-server-engine`: o servidor;
- `dist/plugins/`: os plugins de exemplo.

A primeira compilação demora (uns 10 minutos); as seguintes são rápidas.

## Rodar

1. **Arquivos do jogo.** O servidor precisa dos dados do 26.3 preparados a partir da **sua** cópia do Minecraft;
   nada da Mojang é distribuído aqui. O jeito fácil: instale o [launcher do Horizon](https://playhorizon.dev) e
   jogue uma vez. Ele deixa os arquivos em
   `~/.local/share/horizon-launcher/server-runtime/game-files/horizon-client/26.3`.
   Detalhes: [hosting.md do Horizon](https://github.com/horizon-rust/horizon/blob/main/docs/hosting.md).
2. **Pasta do servidor.** `server/` já tem o `iniciar.sh`. Copie os plugins que quiser para `server/plugins/`:

   ```sh
   cp dist/plugins/* server/plugins/
   cd server && ./iniciar.sh --initSettings   # cria server.properties e eula.txt
   ```

   Leia e aceite o EULA da Mojang em `eula.txt` (`eula=true`) e ajuste o `server.properties`.
3. **Iniciar.** `./iniciar.sh`. O console é o próprio terminal: `op SeuNome`, `plugins`, `stop`...

Outra pasta de jogo: `RESOURCES=/caminho/26.3 ./iniciar.sh`.

## Escrever um plugin

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
horizon-plugin-api = { git = "https://github.com/JoaoVictorCoder/horizon-server-plugins" }
```

```rust
use horizon_plugin_api::prelude::*;

#[derive(Default)]
struct Ola;

impl Plugin for Ola {
    fn on_enable(&mut self, ctx: &mut Ctx<Self>) {
        ctx.on(|_, _, e: &mut PlayerJoinEvent| {
            e.player.send_message(&color::translate('&', "&aBem-vindo!"));
        });
    }
}

horizon_plugin!(Ola, PluginDescription::new("Ola", "1.0.0"));
```

`cargo build --release` gera `libola.so`, que vai em `plugins/`. Para testar sem servidor:

```sh
cargo run -p horizon-plugin-host --example plugin_dry_run -- pasta/com/plugins "comando arg"
```

## Estrutura

| Pasta | O que é |
|---|---|
| `api/` | `horizon-plugin-api`: a API dos plugins (sem dependências, ABI em C) e os exemplos |
| `host/` | `horizon-plugin-host`: carrega `plugins/`, eventos, comandos, permissões, reload |
| `engine/` | `horizon.patch` (as mudanças no motor do Horizon) e o commit em que ele se aplica |
| `server/` | a pasta do servidor, com `iniciar.sh` |
| `docs/plugins.md` | a documentação da API |

## Licença e créditos

MIT, como o Horizon. Veja [LICENSE](LICENSE) e [CREDITS.md](CREDITS.md). Minecraft é marca da Mojang Studios;
este projeto não tem ligação com a Mojang, a Microsoft, o Horizon, o Paper ou o WorldEdit.
