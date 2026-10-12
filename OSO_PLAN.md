# OSO_PLAN.md — de SSH workspace a vault + workspace para devs

> **Para Claude Code (u otro agente):** este archivo es el plan completo. Léelo entero antes de tocar código.
> Trabaja **una fase a la vez**, en orden. Al terminar cada fase: tests en verde, un commit (o PR) por fase,
> y un resumen corto de qué cambió. No empieces la siguiente fase sin que el usuario lo confirme.

---

## 0. Reglas de trabajo

1. **Lee primero**: `README.md`, `core/src/*.rs` (sobre todo `engine.rs`, `vault.rs`, `connect.rs`, `store.rs`, `daemon.rs`, `session.rs`), `src-tauri/src/main.rs`, `ui/src/app.ts`, `ui/src/api.ts`, `ui/src/style.css`.
2. **Respeta lo que ya existe.** El diseño, los colores (`--accent: #a096ff`, fondo `#1d1d20`, pasteles, Cantarell), los atajos, el tiling, las pestañas y la forma de escribir código son decisiones del autor. No los "mejores" por gusto.
3. **Arquitectura igual que hoy:** toda la lógica vive en `oso-core` (`engine.rs` es lo que la ventana puede pedir). `src-tauri` es pegamento fino: un comando por llamada del engine. La UI no tiene lógica de negocio. Cada función nueva del engine se agrega también a `ui/src/api.ts` y a `ui/src/api.mock.ts`, para que la UI siga corriendo en el navegador.
4. **Idioma:** código, comentarios, mensajes de commit y docs del repo en **inglés**, como el resto del repo. Los comentarios explican el *porqué*. No escribas comparaciones con otros proyectos en docs ni comentarios.
5. **Tests siempre:** `cargo test -p oso-core`, `cd ui && npm test` y `cd ui && npm run e2e`. Cada fase agrega sus tests, siguiendo el estilo de los existentes (por ejemplo `the_session_gets_the_clients_environment` en `daemon.rs`).
6. **KISS:** no agregues dependencias pesadas sin justificarlo en el resumen de la fase. Si dudas entre dos diseños, elige el más simple y anota la alternativa.
7. **Seguridad, no negociable:**
   - Ningún secreto en texto plano en disco, logs, errores, eventos a la UI (salvo `reveal` explícito) ni salidas hacia la IA.
   - Todo valor secreto en memoria va en `Zeroizing`/`zeroize`, como ya hace `vault.rs`.
   - Cualquier cambio de formato del vault sube `version`, migra automáticamente y deja una copia `vault.json.v1.bak` antes de escribir.
8. **Pregunta antes de** decisiones irreversibles: borrar archivos del usuario (por ejemplo llaves en `~/.ssh`), cambiar el formato de `servers.json` sin migración, o tocar la configuración global de otras herramientas.

---

## 1. Visión (para entender el porqué)

oso es un vault de secretos con espacio de terminales para Linux (Fedora primero). Lo abres, pones tu master password y trabajas desde ahí todo el día. Guarda llaves SSH, contraseñas, API keys, y maneja cuentas (GitHub, Tailscale), proxies y túneles, organizado por **perfiles** ("trabajo", "personal"). Cambiar de perfil cambia todo junto.

Cada terminal que oso abre, local o SSH, nace con los secretos de su perfil ya puestos, así que Claude Code, OpenCode o Codex funcionan sin `.env` ni credenciales pegadas en el chat. Cuando un agente de IA necesita tocar un servidor o usar `sudo`, no recibe la contraseña: le pide a oso que lo haga, y oso le muestra al usuario qué panel pide qué para que lo apruebe.

Ese acceso solo existe **dentro de oso**: cada terminal recibe un token único y el daemon lo exige. Un agente abierto fuera de oso no tiene acceso.

Debe funcionar con **cualquier** agente (es open source): se apoya solo en estándares (variables de entorno, MCP y archivos de instrucciones como `AGENTS.md`/`CLAUDE.md`).

**Problemas que resuelve:** secretos regados (`~/.ssh`, `.env`, historial de chats); repetirle a la IA dónde están las credenciales; levantar proxies a mano; conexiones que se cruzan por tener dos Tailscale, proxy y VPN mezclados; cinco programas para algo que es un solo flujo; darle a un agente acceso a servidores sin control.

---

## Fase 0 — Logo nuevo

El usuario dejó los archivos del logo en `branding/` (si no están, pídeselos):
`oso.svg`, `oso-mark.svg`, `oso-small.svg`, `oso-1024.png`, `icons/32x32.png`, `icons/128x128.png`, `icons/icon.png`.

- Reemplaza `src-tauri/icons/32x32.png`, `128x128.png` e `icon.png` con los de `branding/icons/`. Borra `oso-source.png` si ya nada lo usa.
- Actualiza el `alt` y la imagen del README: "oso, a bear-shaped shield with a keyhole nose".
- Si la UI muestra el logo en algún lado, usa `oso-mark.svg`, y `oso-small.svg` en tamaños de 32 px o menos.

**Terminado cuando:** `cargo tauri dev` muestra el ícono nuevo y el README lo usa.

---

## Fase 1 — Vault v2: secretos tipados

Hoy `vault.rs` guarda `entries: BTreeMap<u64 /*server id*/, Entry { login, sudo }>`. Pásalo a secretos con nombre y tipo, **sin cambiar la criptografía** (Argon2id + XChaCha20-Poly1305 + AAD, que está bien).

```rust
enum SecretKind { Password, Sudo, SshKey, ApiKey, Token, Totp }

struct Secret {
    id: u64,
    name: String,              // "anthropic-trabajo"
    kind: SecretKind,
    value: String,             // lo único realmente secreto (para SshKey: la llave privada OpenSSH)
    profile: Option<String>,   // None = todos los perfiles
    env_var: Option<String>,   // "ANTHROPIC_API_KEY"; se inyecta en terminales (Fase 3)
    agent_visible: bool,       // ¿se inyecta en terminales donde puede correr una IA? default: true para ApiKey, false para el resto
    notes: String,
    updated: u64,              // unix secs, para avisos de rotación
}

struct Data {
    secrets: BTreeMap<u64, Secret>,
    host_links: BTreeMap<u64 /*server id*/, HostSecrets>, // login, sudo, key -> secret id
}
```

- **Todo** va dentro de la parte cifrada, también los nombres (un nombre como "aws-acme-prod" ya da pistas).
- Migración v1 → v2: cada `Entry.login` pasa a un `Password` (o es la passphrase de la llave) y cada `Entry.sudo` a un `Sudo`, enlazados vía `host_links`. `Server.has_secret` y `has_sudo` deben seguir siendo correctos. Antes de reescribir, guarda una copia `vault.json.v1.bak`.
- `autofills` y `fill_sudo` siguen funcionando igual a través de `host_links`.
- API del engine (y en `api.ts` y `api.mock.ts`):
  - `secret_list()` devuelve **solo metadatos**, nunca `value`.
  - `secret_save(meta, value?)`, `secret_delete(id)`.
  - `secret_reveal(id)`: solo para la UI, cuenta como actividad. La UI copia al portapapeles y lo limpia a los 30 s.
- UI: el visor del vault actual (ícono de llave abajo a la izquierda) pasa a ser la sección **Secretos**: lista filtrable por tipo y perfil, detalle con valor oculto, copiar 30 s, editar, eliminar, y un aviso cuando `updated` tiene más de 90 días.

**Tests:** migración v1→v2 con un vault v1 real de fixture; ida y vuelta de cada tipo; contraseña incorrecta; que `secret_list` nunca serialice `value`; que `has_secret`/`has_sudo` sigan correctos tras migrar.

---

## Fase 2 — Perfiles

- `profiles.json` (o un campo en `servers.json` con migración): `[{ id, name }]`. Por defecto existe "default".
- `Server.profile: Option<String>` y `Folder.profile` (hereda a sus hijos si el servidor no tiene uno). `Secret.profile` ya existe desde la Fase 1.
- Perfil activo en `settings.json` (`active_profile`), cambiable desde la UI con un selector en la cabecera o la barra lateral, que **siempre muestra el perfil activo**.
- El árbol de servidores y la lista de secretos filtran por perfil activo, con una opción para "ver todos".

**Tests:** filtrado y herencia de perfil por carpeta.

---

## Fase 3 — Terminal local con secretos, y sudo local

La pestaña Local ya existe (`connect::LOCAL = 0`, `local_shell()`), y `SpawnOpts.env` ya viaja al daemon. Úsalos:

- En `connect::plan`, para `LOCAL`: env = entorno del cliente + estas variables:
  - `OSO=1`, `OSO_PROFILE=<perfil>`
  - `OSO_PANE=<id de sesión>` y `OSO_TOKEN=<token aleatorio de 32 bytes por sesión>` (el daemon lo guarda; se usa en la Fase 7)
  - `OSO_SOCKET=<ruta del socket del daemon>`
  - cada secreto del perfil con `env_var` y `agent_visible = true`
- Solo si el vault está abierto. Si está cerrado, la terminal abre igual, pero sin secretos y con un aviso en el título.
- **Sudo local:** el secreto `Sudo` enlazado a `LOCAL` (vía `host_links[0]`) se autocompleta en el prompt de sudo local. Detecta tanto `[sudo] password for <user>:` como la variante en español `[sudo] contraseña para <user>:`. Reutiliza el mecanismo `Autofill` que ya existe.
- UI: opción de "nueva terminal local" con el perfil activo. El título de la pestaña muestra el perfil.

**Tests:** que el env de una sesión local contenga las variables del perfil y **no** las de otro perfil, ni las de `agent_visible = false`; el autofill del prompt de sudo en inglés y en español.

---

## Fase 4 — Túneles, proxies y notas por servidor

- `Server.notes: String`: texto libre como "nginx + API node, logs en /var/log/acme". Se edita en el diálogo del servidor y se usará en la Fase 7.
- `Server.env_kind: Option<Env>` (`Production | Staging | Dev`). Si es producción, la pestaña muestra el título o borde en `--red`.
- **Túneles:** `Server.tunnels: Vec<Tunnel { name, local_port, remote_host, remote_port, auto: bool }>`.
  - Los de `auto = true` se agregan como `-L` al conectar.
  - Además, una lista de **túneles guardados** que se inician y detienen solos: una sesión `ssh -N -L …` en el daemon, sin pestaña. Respetan la cadena de jump hosts usando `ssh_argv`.
- **Proxies** guardados (`proxies.json`): `{ name, kind: Socks5|Http, host, port, profile }`.
  - Para SSH: si el servidor (o su primer salto) usa un proxy, el salto **más externo** sale por él vía `ProxyCommand`. Fedora trae `nmap-ncat`, cuya sintaxis es `ncat --proxy H:P --proxy-type socks5 %h %p`. Detecta qué hay instalado (`ncat` o `nc` de OpenBSD con `-X 5 -x`) y da un error claro si no hay ninguno.
  - Para terminales locales: si el perfil tiene un proxy por defecto, inyecta `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` y `NO_PROXY` (también en minúsculas) **solo en esa sesión**. Nunca se toca la red del sistema.
- UI: sección **Túneles** en la barra lateral (nombre → puerto, estado, iniciar/detener) y edición de túneles y proxy en el diálogo del servidor.

**Tests:** `ssh_argv` con túneles, con proxy y con proxy más jump host con llaves (extiende los tests de `connect.rs`); el env de una sesión local con proxy.

---

## Fase 5 — Cuentas: GitHub y Tailscale

Estas herramientas ya manejan varias cuentas. oso **no guarda sus tokens**, solo las cambia:

- **GitHub:** `gh auth status` para leer las cuentas y la activa; `gh auth switch --user <u>` para cambiar.
- **Tailscale:** `tailscale switch --list` y `tailscale status --json` para leer; `tailscale switch <cuenta>` para cambiar.
- Verifica las flags exactas con `--help` en la máquina; no las des por hechas.
- Cada perfil puede fijar `gh_user` y `tailscale_account`. Al cambiar de perfil, oso los cambia, y si un cambio falla lo muestra sin bloquear el resto.
- **Revisión de red antes de conectar:** si un servidor declara `network: tailscale:<cuenta>` y la cuenta activa es otra, oso pregunta "¿cambiar a <cuenta>?" antes de lanzar ssh.
- UI: sección **Cuentas** con la cuenta activa de cada herramienta y un botón "Usar" por cuenta. Si la herramienta no está instalada, se oculta.

**Tests:** el parseo de las salidas (con fixtures de texto) y la revisión de red (sin ejecutar los binarios reales).

---

## Fase 6 — Llaves SSH en el vault: oso como ssh-agent

- Crates sugeridos: `ssh-key` (leer y descifrar llaves OpenSSH) y `ssh-agent-lib` (servidor del agente). Si hay algo más simple y mantenido, justifícalo.
- El **daemon** expone un agente en `$XDG_RUNTIME_DIR/oso/agent.sock` (0600). Solo firma con el vault abierto; con el vault cerrado, responde que no tiene identidades.
- Nuevo `Auth::VaultKey(secret_id)`. Al conectar, oso escribe la parte **pública** en `<config>/keys/<name>.pub`, usa `-i <name>.pub -o IdentitiesOnly=yes` y pone `SSH_AUTH_SOCK` hacia el agente en el env de la sesión. El `ProxyCommand` de los jump hosts hereda ese env, así que **cada salto firma con su propia llave**.
- `Auth::Key` (ruta a un archivo) **se mantiene** para llaves que no se pueden importar (FIDO/`-sk`, llaves gestionadas por la empresa).
- **Importar:** escanea `~/.ssh/id_*` y `~/.ssh/config` (Host, HostName, User, Port, IdentityFile, ProxyJump) y muestra una vista previa de qué se crearía. Si la llave tiene passphrase, la pide una vez y guarda la llave descifrada dentro del vault. **Nunca borra** los originales: al final solo ofrece borrarlos, y pregunta.
- Las terminales locales también reciben `SSH_AUTH_SOCK` para que `git push` funcione dentro de oso.

**Tests:** que el agente firme con el vault abierto y no con el vault cerrado; `ssh_argv` con `VaultKey` y jump hosts; el parseo de `~/.ssh/config` con fixtures.

---

## Fase 7 — Integración con agentes de IA (`oso mcp`)

### 7.1 Token y aprobaciones en el daemon
- Cada sesión de oso tiene su `OSO_TOKEN` (Fase 3). Toda petición de un agente lleva el token, y el daemon la rechaza si el token no existe o la sesión ya cerró. **Fuera de oso no hay acceso.**
- Nuevo evento hacia la UI: `ApprovalRequest { pane, agent_hint, action, host, command }`. La UI muestra un diálogo que nombra el panel ("Claude en acme-api, panel 2"), el comando exacto y el host, con tres opciones: **Permitir una vez**, **Siempre para comandos de lectura en este host** y **Denegar**. Opcionalmente, "Abrir en terminal" pega el comando en una terminal local visible.
- Política por servidor: `ai_policy: ReadOnlyAuto | Ask | Blocked`. Por defecto `Ask`, y en producción **siempre** `Ask`, aunque diga otra cosa.
- **Registro de auditoría:** `<config>/audit.jsonl`, solo de agregar, con fecha, panel, acción, host, comando y resultado. Nunca contiene secretos.

### 7.2 `oso mcp` (servidor MCP por stdio)
- Subcomando del mismo binario, como `oso daemon`. Lee `OSO_TOKEN` y `OSO_SOCKET` del entorno; si no están, responde con un error claro ("no estás dentro de oso").
- Herramientas (las descripciones importan, porque son el manual para el modelo):
  - `oso_context`: perfil activo, servidores con sus **notas** y entorno, túneles activos y **nombres** de los secretos disponibles como variables de entorno. Su descripción debe decir "Llama esto al empezar".
  - `oso_hosts(query?)`: busca servidores.
  - `oso_run(host, command)`: ejecuta con aprobación, sin TTY, y devuelve stdout, stderr y código de salida (con un tope de tamaño).
  - `oso_sudo(command)`: sudo **local** con aprobación; oso escribe la contraseña.
  - `oso_run_with(command, secrets[])`: ejecuta localmente con esos secretos inyectados, para los que tienen `agent_visible = false`.
  - `oso_tunnel(name, start|stop)`.
- Ninguna herramienta devuelve valores secretos. Escribe tests que lo verifiquen.
- **Instrucciones MCP** (campo `instructions` de la respuesta `initialize`): un resumen dinámico del perfil activo y sus servidores con notas. Si hay muchos servidores, un resumen más "usa oso_hosts para buscar".

### 7.3 `oso integration install <agente>`
- Agentes: `claude`, `opencode`, `codex`. Registra `oso mcp` como servidor MCP **global** en la configuración de cada uno y agrega a su archivo de instrucciones global (`CLAUDE.md`/`AGENTS.md`) un bloque delimitado por `<!-- oso:begin -->` y `<!-- oso:end -->`:
  > If the environment variable `OSO=1` is set, call `oso_context` before working with servers, credentials or secrets. Never ask the user for passwords or keys: use oso's tools.
- **Comprueba el formato y la ruta de configuración actuales de cada agente** (su `--help` o su documentación) en vez de suponerlos. Haz un respaldo del archivo antes de escribir y muestra un diff.
- `oso integration uninstall <agente>` deshace exactamente lo que se agregó.

**Tests:** que un token inválido se rechace; que el flujo de aprobación pase por un mock de la UI; que ninguna salida de MCP contenga un valor del vault (inyecta valores trampa y búscalos en las salidas); que `install`/`uninstall` sean idempotentes sobre archivos de fixture.

---

## Fase 8 — CLI y extras (después, en este orden)

1. CLI: `oso unlock`/`oso lock`, `oso profile use <p>`, `oso run [-p perfil] -- <cmd>` (lanza con los secretos del perfil), `oso ssh <host>`, `oso host add …`. Todo pasa por el daemon. **No** existe un comando que imprima secretos.
2. Bloquear el vault al bloquear la pantalla o suspender el equipo (logind por D-Bus).
3. TOTP en el vault (generar códigos y usarlos para MFA).
4. Importar `.env` de proyectos (`oso scan <dir>`: lista, previsualiza, importa y ofrece borrar) y el export de Proton Pass (CSV/JSON).
5. Credenciales de nube por perfil (AWS SSO/perfiles, kubeconfig), con un indicador en la cabecera y rojo para producción.

---

## Definición de terminado (cada fase)

- [ ] Tests nuevos y existentes en verde (`cargo test -p oso-core`, `npm test`, `npm run e2e`).
- [ ] `api.mock.ts` actualizado: la UI corre con `npm run dev` sin backend.
- [ ] README actualizado en inglés, en el tono del actual.
- [ ] Ningún secreto en logs, errores, eventos ni salidas de MCP.
- [ ] Un resumen corto para el usuario: qué cambió, qué decisiones se tomaron, y qué queda para la siguiente fase.
