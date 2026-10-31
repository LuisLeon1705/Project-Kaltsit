# Kal'tsit Accessories & Modules

> Official modular ecosystem for **Kal'tsit AI Companion**.
> Enables external games, engines, and tools to connect to Kal'tsit's autonomic nervous system, CTHG memory, and voice synthesis.

---

## 🏛 Architecture

Kal'tsit is designed with a detached, modular philosophy:
- **Brain & Core** lives in the main repository: high-performance Rust core, CTHG dynamic graph memory, Kyutai Pocket TTS, local Llama inference, and autonomous wandering.
- **Accessories & Modules** live here: game mods, OS integrations, and external plugins that interface with the core over ultra-low-latency IPC sockets (`127.0.0.1:25599`).

```
┌──────────────────────────────────────────────┐
│             GAME / ENVIRONMENT               │
│  (Minecraft, Game Engines, Discord, OBS...)  │
│  - Physical Embodiment                       │
│  - World State & Telemetry                   │
│  - In-game Chat & UI                         │
└──────────────────────▲───────────────────────┘
                       │ Local IPC (20 Hz / JSON)
┌──────────────────────▼───────────────────────┐
│            KAL'TSIT COMPANION CORE           │
│  - Reflex Instincts (<0.005 ms CPU)          │
│  - CTHG Graph Mind & Reflection              │
│  - Cloned Neural Voice (Pocket TTS)          │
└──────────────────────────────────────────────┘
```

---

## 📦 Available Accessories

| Module | Target | Version | Description |
| :--- | :--- | :--- | :--- |
| [`minecraft`](./minecraft) | Minecraft Java (Fabric) | `26.2+` | Standalone companion entity with full world interactions (mining, building, door air pockets, potion drinking, mob defense, chat & voice). |

---

## 🛠 Adding a New Accessory

Every accessory in this repository follows a clean specification:
1. **`manifest.json`**: Metadata defining the accessory, ID, target platform, and IPC protocol version.
2. **IPC Telemetry**: Streams environment telemetry at 10-20 Hz to `127.0.0.1:25599`.
3. **Action Execution**: Executes sub-millisecond reflex actions sent by Kal'tsit's instinct handler.
4. **Chat Bridge**: Forwards in-environment dialogue to Kal'tsit and broadcasts her replies.
