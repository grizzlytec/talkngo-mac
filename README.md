# TalknGo

**Dictado por voz y reescritura de texto, todo en tu Mac, sin internet.**
_Voice dictation and text rewriting, all on your Mac, no internet needed._

[Español](#español) · [English](#english) · [Based on Handy](#basado-en-handy--based-on-handy)

---

## Español

### Qué hace

- **Dictado local.** Mantén o toca una tecla, habla, y el texto aparece donde está tu cursor, en cualquier app. La transcripción corre en tu Mac con un modelo local (recomendado: **Parakeet V3**). Nada sale de tu computadora.
- **Reescribir texto seleccionado.** Selecciona texto en cualquier app y toca **Shift derecho**. TalknGo lo reescribe en el mismo lugar con un modelo local vía Ollama, en 6 estilos: _Corregir gramática_, _Formal_, _Más amable_, _Más claro_, _Más corto_ y **Mi voz** (como lo escribirías tú, siguiendo tu propia guía de voz).

### Requisitos

- Mac con Apple Silicon (M1 o posterior), macOS 10.15 o más reciente.
- Para reescribir: [Ollama](https://ollama.com) instalado.

### Instalación (para un amigo)

1. Descarga el `.zip` desde la sección **Releases** de este repositorio y descomprímelo.
2. Mueve **TalknGo** a la carpeta **Aplicaciones**.
3. La app no está firmada: haz **clic derecho → Abrir** (la primera vez), y confirma **Abrir**.
4. Da permiso de **Micrófono** y de **Accesibilidad** cuando macOS lo pida (Ajustes del Sistema → Privacidad y seguridad). Sin Accesibilidad no puede escribir el texto ni escuchar las teclas.
5. En **Ajustes → Modelos**, descarga un modelo de voz. Recomendado: **Parakeet V3**.
6. Para reescribir, instala Ollama desde [ollama.com](https://ollama.com) y en Terminal ejecuta:
   ```
   ollama pull qwen2.5:7b
   ```
   TalknGo abre Ollama solo cuando hace falta. El modelo se puede cambiar en **Ajustes → Transform**.
7. Opcional, para el estilo **Mi voz**: en **Ajustes → Transform → Guía de mi voz — fuente** elige de dónde sale tu guía (cómo escribes, tono, muletillas, ejemplos) y tu lista de palabras especiales:
   - **Servidor MCP**: pega la dirección de tu servidor MCP, pulsa **Conectar** e inicia sesión una vez en el navegador. TalknGo guarda el acceso en el llavero de tu Mac y actualiza la guía y las palabras cada hora.
   - **Archivo**: un `.md` o `.txt` en tu computadora (puede estar en una carpeta de Google Drive o Dropbox; los `.gdoc` no sirven, guárdalo como texto). Opcional: un segundo archivo con palabras, una por línea.
   - **Escríbela aquí**: la guía integrada; **Abrir guía de voz** la edita.

   Las palabras de la fuente se suman a las que escribes en la app.

### Teclas

- **Dictar:** por defecto **Option + Espacio** (mantén para hablar mientras la tienes pulsada, o toca para activar/desactivar). Para cambiarla: **Ajustes → General → Atajo de transcripción**, pulsa el atajo y presiona la tecla que quieras. Si eliges **solo Option derecho**: mantenlo para dictar (soltar detiene), o dale doble toque para dictar sin tocar nada y un toque más para detener.
- **Reescribir:** toca **Shift derecho** (un toque rápido, sin otra tecla). Elige el estilo en **Ajustes → Transform** o en el menú de la barra superior.

---

## English

### What it does

- **Local dictation.** Hold or tap a key, speak, and the text appears at your cursor in any app. Transcription runs on your Mac with a local model (recommended: **Parakeet V3**). Nothing leaves your computer.
- **Rewrite selected text.** Select text in any app and tap **Right Shift**. TalknGo rewrites it in place with a local model through Ollama, in 6 styles: _Correct grammar_, _Formal_, _Friendlier_, _Clearer_, _Shorter_, and **My voice** (the way you write, following your own voice guide).

### Requirements

- Apple Silicon Mac (M1 or later), macOS 10.15 or newer.
- To rewrite text: [Ollama](https://ollama.com) installed.

### Install (for a friend)

1. Download the `.zip` from this repository's **Releases** page and unzip it.
2. Move **TalknGo** to **Applications**.
3. The app is unsigned: **right-click → Open** the first time, then confirm **Open**.
4. Grant **Microphone** and **Accessibility** when macOS asks (System Settings → Privacy & Security). Without Accessibility it cannot type the text or hear the keys.
5. In **Settings → Models**, download a speech model. Recommended: **Parakeet V3**.
6. To rewrite text, install Ollama from [ollama.com](https://ollama.com) and run in Terminal:
   ```
   ollama pull qwen2.5:7b
   ```
   TalknGo starts Ollama by itself when needed. You can change the model in **Settings → Transform**.
7. Optional, for the **My voice** style: in **Settings → Transform → My voice guide — source**, pick where your guide (how you write, tone, habits, examples) and your special-words list come from:
   - **MCP server**: paste your MCP server address, click **Connect** and sign in once in your browser. TalknGo keeps the access in your Mac's keychain and refreshes the guide and words every hour.
   - **File**: a `.md` or `.txt` on your computer (it can live in a Google Drive or Dropbox folder; `.gdoc` files do not work, save it as plain text). Optional: a second file with words, one per line.
   - **Write it here**: the built-in guide; **Open voice guide** edits it.

   Words from the source are added to the ones you type in the app.

### Make your MCP server work with TalknGo

- Expose a tool named `get_voice_guide` (no required arguments) that returns the guide as text, or as JSON text `{"version": 4, "guide": "…"}`.
- Expose a tool named `get_vocabulary` (no required arguments) that returns `{"words": ["…"]}`, a JSON array, or one word per line. An entry like `Loxone = lox one, locks one` keeps only the word before `=`.
- If your tools have other names, TalknGo lets you pick them from a menu after connecting.
- Sign-in: Streamable HTTP MCP with OAuth 2.1 (PKCE), dynamic client registration, and loopback redirects `http://127.0.0.1:<port>/callback`. The server address must use `https://`. If your server advertises a `voice` scope, TalknGo asks only for it — use it to give TalknGo read-only access.

### Keys

- **Dictate:** the default is **Option + Space** (hold to talk while pressed, or tap to toggle). To change it: **Settings → General → Transcribe Shortcut**, click it and press the key you want. If you pick **Right Option alone**: hold it to dictate (release stops), or double-tap for hands-free dictation and tap once more to stop.
- **Rewrite:** tap **Right Shift** (a quick tap, no other key). Pick the style in **Settings → Transform** or from the menu bar icon.

---

## Basado en Handy / Based on Handy

TalknGo es un derivado de **Handy**, de **CJ Pais**, publicado bajo licencia MIT. / TalknGo is a derivative of **Handy** by **CJ Pais**, released under the MIT license.

- Proyecto original / Original project: <https://github.com/cjpais/handy>
- Licencia / License: see [LICENSE](LICENSE).

Gracias a CJ Pais y a los colaboradores de Handy por el trabajo que hace esto posible. / Thanks to CJ Pais and the Handy contributors for the work that makes this possible.
