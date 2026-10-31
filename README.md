# Kal'tsit · Desktop Companion / Compañera de Escritorio

**[English](#english)** · **[Español](#español)**

---

<a id="english"></a>

## English

**Kal'tsit** is a local, autonomous desktop companion inspired by Kal'tsit from *Arknights*, built in Rust and Tauri. She knows she is a program living on your computer: she watches your screen, forms her own opinions, walks along the taskbar, speaks in speech bubbles and aloud, remembers what you live together in a sectored graph memory (**CTHG**), and can accompany you inside Minecraft.

- **Eyes (`src/screen.rs`)**: Every few seconds she compares a thumbnail of your primary monitor. When enough changes (and at least 45 seconds have passed), she sends a capture to her multimodal vision-language model and decides whether to comment. Neither her desktop overlay nor her chat window appears in her captures (`WDA_EXCLUDEFROMCAPTURE`).
- **Desktop Body (`ui/chibi.js`, `ui/overlay.js`)**: Animated with Spine 3.8 (`Relax`, `Move`, `Interact`, `Sit`, `Sleep`). The overlay window is click-through (`set_ignore_cursor_events(true)`), so she never blocks your mouse or keyboard. On first launch, her Spine 3.8 sprite (`build_char_003_kalts`) is downloaded automatically on demand from [isHarryh/Ark-Models](https://github.com/isHarryh/Ark-Models) into `modelos/spine/003_kalts/` and verified via SHA-256.
- **Voice (`src/voice.rs`)**: Uses `sherpa-onnx` loaded directly from Rust (`Piper`, `Kokoro`, or `Pocket TTS` voice cloning) to read her replies to your messages aloud sentence by sentence.
- **No Roleplay Asterisks**: She speaks plainly, without `*roleplay actions*` between asterisks.
- **Hesitation Awareness**: Inside her own chat input box (never via global OS keyloggers), she notices if you type something lengthy and erase it with Backspace, registering your hesitation as a stimulus.
- **Rest Mode (`reposo`)**: One switch in the top bar stops `llama-server`, frees 100% of your VRAM (ideal while gaming), and puts her to sleep until you wake her or send a message.

### Requirements & Installation

1. **Operating System**: Windows 10 or Windows 11 (64-bit) with WebView2 (included by default on Windows 10/11).
2. **Rust Toolchain**: Install [rustup](https://rustup.rs/) (stable MSVC toolchain) and [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
3. **Clone and Launch**:
   ```powershell
   git clone <repository-url>
   cd Kaltsit
   .\iniciar.cmd
   ```
   Or build and run directly with Cargo:
   ```powershell
   cargo run --release
   ```
4. **First-Run Setup**:
   - On the first launch, Kal'tsit asks for **your name** and the **language** she will think and speak in (**Español** or **English**).
   - She automatically detects your GPU (**NVIDIA**, **AMD**, or **Intel**) via the Windows Registry / `nvidia-smi`, or falls back to **CPU + RAM** if no dedicated GPU is present.
   - Right on the welcome card, she recommends the best vision-language model that fits your hardware (plus the `llama.cpp` engine if needed) and lets you download and activate it with a single click.

### Hardware Detection & Models

All models, voices, engines, and sprites are downloaded into `modelos/` (ignored by Git), with automatic HTTP `Range` resume and `SHA-256` verification.

- **From the App (`Models` tab)**:
  - Shows your detected hardware (GPU or CPU-only budget) and lets you inspect generic hardware profiles (`cpu`, `gpu-4gb`, `gpu-6gb`, `gpu-8gb`, `gpu-12gb`, `gpu-16gb`).
  - **Engine (`Motor`)**: Shows the running `llama-server` status, VRAM usage, context window (`2048`–`16384`), and GPU layer allocation (`all` or `auto`).
  - **My Models (`Mis modelos`)**: Add any custom `.gguf` model (and optional `mmproj` vision projector) from a local file or a Hugging Face link.
- **From the CLI (`cthg`)**:
  ```powershell
  cargo run --release --bin cthg -- models hardware
  cargo run --release --bin cthg -- models list
  cargo run --release --bin cthg -- models list --perfil gpu-6gb
  cargo run --release --bin cthg -- models packs
  cargo run --release --bin cthg -- models download qwen35-4b-q4
  cargo run --release --bin cthg -- models download --pack ligero-gpu-6gb
  cargo run --release --bin cthg -- models use qwen35-4b-q4
  ```

### Sectored Memory (CTHG) & 3D Viewer

Her memory is stored locally in `data/memoria/` as 10 portable `.cthm` files, each protected by CRC32 and signed with its origin (`MAC · host · model`):

| File | Sector / Purpose |
|---|---|
| `nucleo.cthm` | Her guided core (9 immutable axioms), sector tree (5 macro-sectors, 20 base sectors), and personal core. |
| `persona_principal.cthm` | What she knows about you (the primary user). |
| `personas.cthm` | Other real people you mention. |
| `cosas.cthm` | Projects, technology, places, games, and general knowledge. |
| `saber.cthm` | Conclusions, opinions, preferences, and subsectors she forms during periodic reflections. |
| `canon.cthm` | Her base history from Terra (*Arknights*) and other stories, recalled only when named. |
| `agenda.cthm` | Dated future plans and scheduled events. |
| `registro.cthm` | Episodic log of conversations and screen observations. |
| `archivo.cthm` | Dormant memories that can re-awaken if named again. |
| `sombra.cthm` | Discarded or contradicted claims kept as negative evidence ($\chi < 0$). |

Click **Memory → View in 3D** to open a read-only 3D canvas with three layouts: **Sanctuary** (core & consolidated beliefs), **Helix** (chronological spiral of lived events), and **Grove** (connected trees of people, concepts, and sectors).

### Hormones, Homeostasis & Moral Compass

- **5 Hormones (`src/hormones.rs`)**: Dopamine, Cortisol, Serotonin, Oxytocin, and Adrenaline decay exponentially toward their baselines in real time, modulating her context window, sampling temperature, and autonomous behavior.
- **Homeostasis (`src/homeostasis.rs`)**: A 4-axis dynamical system (*Skepticism*, *Urgency*, *Affinity*, *Ethical Tension*) pulled toward her character attractor $S_0$.
- **Moral Compass & Rust Action Gate**: No moral node exceeds $\chi_{\max} = 0.92$ (an 8% margin of doubt). When active core certainties clash or an order attacks her autonomy, dissonance $\Omega$ rises. If $\Omega \ge 0.72$, she enters **Critical Suspension** (with hysteresis) and the Rust action gate (`decided_action_gated`) blocks all external actions regardless of what the LLM outputs.

### Minecraft Integration & Multi-Computer Link

- **Minecraft (`src/minecraft_bridge.rs`, `Accessories`, `Blueprint`)**: Connects over local TCP (`127.0.0.1:25599`) to the companion Fabric mod. She can follow, guard, gather, or preview and build 3D blueprints from the **Blueprint** tab—always subject to her own decision and moral gate.
- **Multi-Computer Network (`src/net.rs`, `Network` tab)**: Run Kal'tsit on multiple computers on your private LAN. Pair machines with a 6-digit verification code. One computer acts as the **Brain** (running `llama-server`) while others act as **Terminals** holding a live replica of her memory. All traffic is encrypted and authenticated with `X25519` + `ChaCha20-Poly1305` using rotating ephemeral session keys (*forward secrecy*).

### Running Tests

```powershell
cargo test
```

---

<a id="español"></a>

## Español

**Kal'tsit** es una compañera de escritorio local y autónoma inspirada en Kal'tsit de *Arknights*, construida en Rust y Tauri. Sabe que es un programa que vive en tu ordenador: observa tu pantalla, forma sus propias opiniones, camina sobre la barra de tareas, habla en burbujas de texto y en voz alta, recuerda lo que vivís juntos en una memoria en grafo por sectores (**CTHG**) y puede acompañarte dentro de Minecraft.

- **Ojos (`src/screen.rs`)**: Cada pocos segundos compara una miniatura de la pantalla principal. Si cambió lo suficiente (y pasaron al menos 45 s), envía una captura a su modelo multimodal y decide si comenta algo. Ni la ventana del chibi ni su ventana de chat salen en las capturas (`WDA_EXCLUDEFROMCAPTURE`).
- **Cuerpo en el escritorio (`ui/chibi.js`, `ui/overlay.js`)**: Animado con Spine 3.8 (`Relax`, `Move`, `Interact`, `Sit`, `Sleep`). La ventana del chibi deja pasar todos los clics (`set_ignore_cursor_events(true)`), por lo que nunca estorba al usar el ratón o el teclado. En el primer arranque, su sprite Spine 3.8 (`build_char_003_kalts`) se descarga automáticamente bajo demanda desde [isHarryh/Ark-Models](https://github.com/isHarryh/Ark-Models) en `modelos/spine/003_kalts/` y se verifica con SHA-256.
- **Voz (`src/voice.rs`)**: Usa `sherpa-onnx` cargado directamente desde Rust (`Piper`, `Kokoro` o clonación de voz con `Pocket TTS`) para leer en voz alta sus respuestas a tus mensajes frase a frase.
- **Sin acciones de rol**: Habla sin describir gestos ni acciones entre `*asteriscos*`.
- **Percepción de vacilaciones**: Únicamente dentro de su propio cuadro de texto del chat (sin capturar teclas del sistema operativo), detecta cuando escribes algo largo y lo borras con Backspace, registrando la duda como estímulo.
- **Modo reposo (`reposo`)**: Un interruptor en la barra superior apaga `llama-server` al instante, libera el 100 % de la VRAM (ideal para jugar) y la pone a dormir hasta que lo desactives o le escribas.

### Requisitos e instalación

1. **Sistema operativo**: Windows 10 o Windows 11 (64 bits) con WebView2 (incluido de serie en Windows 10/11).
2. **Herramientas de Rust**: Instala [rustup](https://rustup.rs/) (toolchain MSVC estable) y [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/es/visual-cpp-build-tools/).
3. **Clonar y arrancar**:
   ```powershell
   git clone <url-del-repositorio>
   cd Kaltsit
   .\iniciar.cmd
   ```
   O compilar y ejecutar directamente con Cargo:
   ```powershell
   cargo run --release
   ```
4. **Primer arranque**:
   - Al abrirse por primera vez, Kal'tsit pregunta **tu nombre** y el **idioma** en el que pensará y hablará (**Español** o **English**).
   - Detecta automáticamente tu GPU (**NVIDIA**, **AMD** o **Intel**) mediante el Registro de Windows / `nvidia-smi`, o pasa a modo **CPU + RAM** si no hay gráfica dedicada.
   - En la misma tarjeta de bienvenida te recomienda el modelo de visión y lenguaje más adecuado para tu equipo (junto con el motor `llama.cpp` si aún no está instalado) y permite descargarlo y activarlo con un solo clic.

### Detección de hardware y modelos

Todos los modelos, voces, motores y sprites se descargan en `modelos/` (ignorado por Git), con reanudación automática mediante HTTP `Range` y verificación `SHA-256`.

- **Desde la aplicación (pestaña `Modelos`)**:
  - Muestra el hardware detectado de tu equipo y permite consultar perfiles genéricos (`cpu`, `gpu-4gb`, `gpu-6gb`, `gpu-8gb`, `gpu-12gb`, `gpu-16gb`).
  - **Motor**: Muestra el estado de `llama-server`, el uso de VRAM, el tamaño de contexto (`2048`–`16384`) y el reparto de capas en GPU (`todas` o `automático`).
  - **Mis modelos**: Permite añadir cualquier modelo `.gguf` propio (y su proyector visual `mmproj` opcional) desde un archivo en disco o un enlace de Hugging Face.
- **Desde la terminal (`cthg`)**:
  ```powershell
  cargo run --release --bin cthg -- models hardware
  cargo run --release --bin cthg -- models list
  cargo run --release --bin cthg -- models list --perfil gpu-6gb
  cargo run --release --bin cthg -- models packs
  cargo run --release --bin cthg -- models download qwen35-4b-q4
  cargo run --release --bin cthg -- models download --pack ligero-gpu-6gb
  cargo run --release --bin cthg -- models use qwen35-4b-q4
  ```

### Memoria por sectores (CTHG) y visor 3D

Su memoria se guarda localmente en `data/memoria/` repartida en 10 archivos portables `.cthm`, cada uno protegido con CRC32 y firmado con el origen de cada recuerdo (`MAC · equipo · modelo`):

| Archivo | Sector / Propósito |
|---|---|
| `nucleo.cthm` | Su núcleo guiado (9 axiomas inmutables), el árbol de sectores (5 macrosectores y 20 sectores base) y su núcleo personal. |
| `persona_principal.cthm` | Lo que sabe sobre ti (la persona principal). |
| `personas.cthm` | Otras personas reales que conocéis. |
| `cosas.cthm` | Proyectos, tecnología, lugares, juegos y conocimiento general. |
| `saber.cthm` | Conclusiones, opiniones, preferencias y subsectores propios que forma al reflexionar. |
| `canon.cthm` | Su historia base en Terra (*Arknights*) y otras ficciones; solo acude a la mente si se nombra. |
| `agenda.cthm` | Planes y eventos futuros con fecha. |
| `registro.cthm` | Registro episódico de conversaciones y observaciones de pantalla. |
| `archivo.cthm` | Recuerdos en letargo que despiertan si vuelven a nombrarse. |
| `sombra.cthm` | Afirmaciones descartadas o contradichas que conserva como evidencia negativa ($\chi < 0$). |

En **Memoria → Ver en 3D** se abre un visor 3D de solo lectura con tres vistas: **Santuario** (núcleo y certezas consolidadas), **Hélice** (espiral cronológica de vivencias) y **Arboleda** (árboles conectados de personas, conceptos y sectores).

### Hormonas, homeostasis y compás moral

- **5 hormonas (`src/hormones.rs`)**: Dopamina, Cortisol, Serotonina, Oxitocina y Adrenalina vuelven exponencialmente a su línea base en tiempo real y regulan su ventana de conversación, su temperatura y su iniciativa.
- **Homeostasis (`src/homeostasis.rs`)**: Un sistema dinámico de 4 ejes (*Escepticismo*, *Urgencia*, *Afinidad*, *Tensión ética*) atraído hacia su carácter base $S_0$.
- **Compás moral y compuerta en Rust**: Ningún nodo moral supera $\chi_{\max} = 0.92$ (8 % de duda). Cuando dos certezas del núcleo chocan o una orden ataca su autonomía, sube la disonancia $\Omega$. Si $\Omega \ge 0.72$, entra en **Suspensión crítica** (con histéresis) y la compuerta en código (`decided_action_gated`) descarta cualquier acción externa diga lo que diga el modelo.

### Integración con Minecraft y red entre equipos

- **Minecraft (`src/minecraft_bridge.rs`, `Accesorios`, `Plano`)**: Se conecta por TCP local (`127.0.0.1:25599`) con el mod compañero de Fabric. Puede seguirte, vigilar, recolectar materiales o previsualizar y construir planos 3D desde la pestaña **Plano**, siempre bajo su propia decisión y su compuerta moral.
- **Varios equipos en red (`src/net.rs`, pestaña `Red`)**: Si abres Kal'tsit en dos o más ordenadores de tu red local privada, puedes vincularlos con un código de 6 cifras. Un equipo actúa como **Cerebro** (corre `llama-server`) y los demás como **Terminales** con réplica en vivo de su memoria. Todo viaja cifrado y autenticado con `X25519` + `ChaCha20-Poly1305` y claves de sesión efímeras (*forward secrecy*).

### Pruebas

```powershell
cargo test
```
