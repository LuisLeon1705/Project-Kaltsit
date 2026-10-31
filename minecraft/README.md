# Kal'tsit Minecraft Companion (Standalone Fabric Mod)

> Standalone companion mod for Minecraft 26.2 (Fabric).
> Connects an in-game companion directly to Kal'tsit's Rust AI core with full world interaction capabilities.

---

## ⚡ Features

- **100% Standalone**: only needs Fabric API. No `Fake Player`, `Carpet` or other libraries.
- **A player's body**: player model with the outer skin layers (hat, jacket, sleeves, trousers), visible armour and held items, and the player animations: walking, sprinting to catch up, jumping, swimming, crouching when the Doctor crouches next to her, looking at whoever is near, swinging, eating.
- **Plays by the player's rules**:
  - Breaks blocks at the speed of the tool in her hand (wrong tool = slow and no drops), with the loot of that tool (fortune, silk touch) and wear on it.
  - Reaches 4.5 blocks like a player; gives up on blocks she cannot get to instead of getting stuck.
  - Fights with the damage and cooldown of her weapon; only attacks real threats (angry neutral mobs, hostile mobs, whoever targets her or the Doctor) and runs from a creeper about to explode.
  - Picks up what lies on the ground into her 36-slot inventory, which is saved with the world.
  - Eats from her inventory with the eating animation when hurt; with nothing of her own she takes one piece of the Doctor's most plentiful everyday food (never golden apples) and says so.
  - Drops everything where she dies and comes back five seconds later: in survival at **her own bed** (the one she placed, or the one the Doctor points at with «esta es tu cama»), or at the world spawn when she has none, like a player; in creative next to the Doctor.
  - Chops only natural trees (logs with natural leaves): your house is safe.
  - Respects no-torch zones and can stop lighting places on her own (saved per world in `kaltsit.json`).
- **Builds like a player** (the approach of Baritone's builder and Mindcraft's blueprints):
  - Plans from templates (`casa`, `refugio`, `torre`, `muro`, `puente`, `corral`, `plataforma`, with size and materials), from layered blueprints the model writes (`{"palette": {...}, "layers": [[...]]}` or Mindcraft's `levels`), or from any structure saved with a Structure Block (this world's `generated/` folder, or `.minecraft/config/kaltsit/structures/*.nbt` for every world).
  - **Blueprint first**: every plan is shown as a hologram of glowing ghost blocks (amber where something would be broken) and in the app's «Plano» tab (isometric and layer by layer, with the materials and what is missing), costing nothing. The Doctor moves it by exactly N blocks (right/left, up/down, forward/back from where he was looking), turns it, then says «constrúyela» or «descártala». Ghosts left by a closed world are cleaned up when their chunk loads.
  - Laid out in front of the Doctor, facing him; refuses sites with his chests or unbreakable blocks.
  - **Left alone she keeps working**: while building (or told to stay, or on guard) away from the Doctor she force-loads the chunks around her and the site, like a chunk loader, also when he goes to another dimension; when the build ends she waits next to it until called.
  - Works out the materials first and gathers them from her inventory, chests near the site and then the Doctor's inventory (telling what she took); stops and lists what is missing, and goes on by herself when it arrives.
  - Clears the site top-down, fills a foundation where the ground is missing, places bottom-up only where a block has something to rest on, doors and beds whole; climbs a pillar of dirt or cobblestone when a block is out of reach and takes it down afterwards; checks the result once more and repairs what fell.
  - Defends herself and the Doctor and then goes back to the site; the build can be paused, resumed or cancelled.
- **Bidirectional Chat & Voice**: every chat message reaches Kal'tsit's mind; she answers in chat (`<Kal'tsit> ...`) and aloud. Only the linked Doctor can give her orders.

---

## 🎮 In game

| Action | What it does |
| :--- | :--- |
| Talk in chat | She answers; orders from the Doctor (chop, mine, build, follow, stay, attack, torch, bed, chest…) become actions. |
| Right click with an item | Gives it to her. |
| Right click with an empty hand / Shift + right click | Opens her inventory. |
| «construye una casa de 7x9 de abeto», «haz una torre de 12», «construye mi_casa» | Starts a build (in chat, or `/kaltsit construye <texto>`). |
| «muévela 3 a la izquierda», «súbela 1», «más adelante», «gírala», «dale la vuelta» | Adjusts the blueprint before building (also from the app's «Plano» tab). |
| «constrúyela» / «descártala» | Builds the blueprint there, or throws it away. |
| «esta es tu cama» | Her respawn point in survival. |
| «pausa / sigue / cancela la obra» | `/kaltsit obra pausa\|sigue\|cancela`; `/kaltsit obra` tells how it goes. |
| «¿qué sabes construir?» | `/kaltsit estructuras`: templates and saved structures. |
| «no pongas antorchas aquí» / «ya puedes poner antorchas aquí» | `/kaltsit antorchas aqui_no [radio]` / `aqui_si`. |
| «deja de poner antorchas» / «vuelve a poner antorchas» | `/kaltsit antorchas auto false\|true`. |
| «deja de seguirme», «quédate» / «sígueme» | `/kaltsit quieta` / `/kaltsit sigueme`. «vigila» keeps guard. |
| `/kaltsit status` | Link with the app, Doctor, state, hormones, health. |
| `/kaltsit inv` | Opens her inventory. |

Her state (following, mining, in combat…) shows above the hotbar and in the app's header.

---

## 🛠 Compilation & Installation

From the app: **Accesorios → Minecraft Fabric Companion → Instalar** (also offers Fabric API if the instance lacks it).

By hand:
```powershell
python build_standalone.py            # build, update ../catalog.json and copy to the AI instance's mods/
python build_standalone.py --no-deploy # build and update the catalog only
```
The version comes from `src/main/resources/fabric.mod.json`; the script replaces older versions of the jar (here and in `mods/`) and writes the new jar's sha256 into `catalog.json`, which the app checks.
