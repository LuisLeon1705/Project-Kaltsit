package ai.kaltsit.companion;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.authlib.GameProfile;
import com.mojang.brigadier.arguments.BoolArgumentType;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerEntityEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.fabricmc.fabric.api.message.v1.ServerMessageEvents;
import net.fabricmc.fabric.api.object.builder.v1.entity.FabricDefaultAttributeRegistry;
import net.minecraft.commands.Commands;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Registry;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ClientInformation;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.ItemTags;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.SimpleMenuProvider;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.MobCategory;
import net.minecraft.world.entity.Relative;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.monster.Creeper;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.levelgen.Heightmap;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.CopyOnWriteArrayList;

public class KaltsitMod implements ModInitializer {
    public static final String MOD_ID = "kaltsit";
    public static final UUID KALTSIT_UUID = UUID.nameUUIDFromBytes("kaltsit:companion".getBytes());

    public static final ResourceKey<EntityType<?>> KALTSIT_KEY = ResourceKey.create(
        Registries.ENTITY_TYPE,
        Identifier.fromNamespaceAndPath("kaltsit", "companion")
    );

    public static final EntityType<KaltsitEntity> KALTSIT_ENTITY = Registry.register(
        BuiltInRegistries.ENTITY_TYPE,
        KALTSIT_KEY,
        EntityType.Builder.of(KaltsitEntity::new, MobCategory.CREATURE)
            .sized(0.6F, 1.8F)
            .clientTrackingRange(10)
            .updateInterval(1)
            .build(KALTSIT_KEY)
    );

    public enum CompanionState {
        IDLE("En reposo", "§7"),
        FOLLOWING("Siguiendo", "§a"),
        GUARDING("En guardia", "§e"),
        COMBAT("En combate", "§c"),
        CHOPPING("Talando madera", "§6"),
        BUILDING("Construyendo", "§e"),
        MINING("Minando", "§d"),
        ALERT("Alerta vital", "§4");

        private final String displayName;
        private final String color;

        CompanionState(String displayName, String color) {
            this.displayName = displayName;
            this.color = color;
        }

        public String getDisplayName() { return displayName; }
        public String getColor() { return color; }
    }

    @FunctionalInterface
    public interface StateChangeHook {
        void onStateChange(CompanionState oldState, CompanionState newState, String reason);
    }

    private static LivingEntity activeEntity = null;
    private static volatile CompanionState currentState = CompanionState.FOLLOWING;
    private static volatile CompanionState previousState = CompanionState.FOLLOWING;
    private static final List<StateChangeHook> stateHooks = new CopyOnWriteArrayList<>();

    // Hormonal synchronization received from Rust AI core
    public static volatile float currentTension = 0.0f;
    public static volatile float currentRise = 0.5f;
    public static volatile float currentAfecto = 0.5f;

    // Linked player running the model (set by link_identity from the desktop app)
    private static volatile String linkedDoctorMinecraftName = "Doctor";

    // Realistic Steve Action Task State: Chopping
    private static BlockPos activeTargetLog = null;
    private static int choppingProgress = 0;
    private static int totalChoppedThisSession = 0;

    // Task Quota and target tracking
    private static int taskTargetCount = 0;
    private static int taskCurrentCount = 0;
    private static String taskTargetBlockName = "";
    private static int autoCheckCooldown = 0;

    // Realistic Steve Action Task State: Mining
    private static BlockPos activeTargetMining = null;
    private static BlockPos activeMiningStandPos = null;
    private static int miningProgress = 0;
    private static Block lastBrokenBlockType = null;

    private static String lastCapabilities = null;

    // Her life when the Doctor is not next to her: death and respawn, chunks kept running
    private static int respawnAt = -1;
    private static int joinedTick = -1;
    private static boolean buildLostNotice = false;
    private static final Set<Long> forcedChunks = new HashSet<>();
    private static ServerLevel forcedLevel = null;

    // Shared by every task: blocks she could not get to, and the walk to the drops
    private static final Set<BlockPos> unreachable = new HashSet<>();
    private static int approachTicks = 0;
    private static int collectTicks = 0;
    private static int collectPause = 0;

    private static int tickCounter = 0;
    private static final ConcurrentLinkedQueue<String> pendingChats = new ConcurrentLinkedQueue<>();
    private static final ConcurrentLinkedQueue<String> pendingCommands = new ConcurrentLinkedQueue<>();
    private static volatile String latestAction = "Idle";
    private static ServerPlayer cachedPrimaryPlayer = null;
    private static ServerPlayer tabDummyPlayer = null;

    public static void registerStateHook(StateChangeHook hook) {
        stateHooks.add(hook);
    }

    public static void updateHormones(float tension, float rise, float afecto) {
        currentTension = tension;
        currentRise = rise;
        currentAfecto = afecto;
    }

    public static void setLinkedDoctorName(String name) {
        if (name != null && !name.trim().isEmpty()) {
            linkedDoctorMinecraftName = name.trim();
            System.out.println("[Kaltsit] Doctor vinculado actualizado: " + linkedDoctorMinecraftName);
        }
    }

    public static synchronized void setState(CompanionState newState, String reason) {
        if (currentState == newState) return;
        CompanionState oldState = currentState;
        if (oldState != CompanionState.ALERT) {
            previousState = oldState;
        }
        currentState = newState;

        System.out.println("[Kaltsit] Cambio de estado: " + oldState + " -> " + newState + " (" + reason + ")");

        for (StateChangeHook hook : stateHooks) {
            try {
                hook.onStateChange(oldState, newState, reason);
            } catch (Exception e) {
                e.printStackTrace();
            }
        }
    }

    public static CompanionState getCurrentState() {
        return currentState;
    }

    /**
     * Checks if the player is the person running the AI model.
     */
    public static boolean isLinkedDoctor(ServerPlayer player) {
        if (player == null) return false;
        String name = player.getName().getString();
        if (linkedDoctorMinecraftName != null && name.equalsIgnoreCase(linkedDoctorMinecraftName)) {
            return true;
        }
        // If singleplayer / local game and this is the host/primary player
        if (cachedPrimaryPlayer != null && player.getUUID().equals(cachedPrimaryPlayer.getUUID())) {
            return true;
        }
        return false;
    }

    @Override
    public void onInitialize() {
        System.out.println("[Kaltsit] Inicializando mod standalone Kal'tsit Companion como entidad dependiente del jugador...");

        FabricDefaultAttributeRegistry.register(KALTSIT_ENTITY, KaltsitEntity.createKaltsitAttributes());

        // Hook 1: IPC Bridge sync to Rust core
        registerStateHook((oldState, newState, reason) -> {
            String json = "{\"type\":\"state_change\"," +
                "\"from\":\"" + oldState.name() + "\"," +
                "\"to\":\"" + newState.name() + "\"," +
                "\"reason\":\"" + escape(reason) + "\"}";
            KaltsitBridge.send(json);
        });

        // Hook 2: Player notification and auditory chime
        registerStateHook((oldState, newState, reason) -> {
            // Above the hotbar, like a status line: the chat stays for what she says.
            if (cachedPrimaryPlayer != null) {
                Component msg = Component.literal("§b[Kal'tsit]§r " + newState.getColor() + newState.getDisplayName() + " §8· " + reason + "§r");
                cachedPrimaryPlayer.sendSystemMessage(msg, true);
                if (newState == CompanionState.COMBAT || newState == CompanionState.ALERT) {
                    try {
                        cachedPrimaryPlayer.playSound(SoundEvents.EXPERIENCE_ORB_PICKUP, 0.5f, 1.3f);
                    } catch (Exception ignored) {}
                }
            }
        });

        // Hook 2b: where she stands survives a restart (following, staying, on guard)
        registerStateHook((oldState, newState, reason) -> KaltsitSettings.setCompanionState(newState.name()));

        // Hook 3: Visual equipment change according to state using ONLY actual inventory items
        registerStateHook((oldState, newState, reason) -> {
            if (activeEntity != null) {
                KaltsitWorldInteractor.updateEquippedGear(activeEntity, newState);
            }
        });

        // Setup Bridge handlers
        KaltsitBridge.setActionHandler(action -> latestAction = action);
        KaltsitBridge.setCommandHandler(cmd -> pendingCommands.add(cmd));
        KaltsitBridge.setChatHandler(text -> pendingChats.add(text));
        KaltsitBridge.start();

        // Optional non-admin /kaltsit status command
        CommandRegistrationCallback.EVENT.register((dispatcher, registryAccess, environment) -> {
            dispatcher.register(Commands.literal("kaltsit")
                .then(Commands.literal("status").executes(ctx -> {
                    ServerPlayer player = ctx.getSource().getPlayer();
                    if (player == null) return 0;
                    String status = "§b=== ESTADO DE KAL'TSIT COMPANION ===§r\n" +
                                   "§7Enlace Rust (127.0.0.1:25599): " + (KaltsitBridge.isConnected() ? "§aONLINE" : "§cOFFLINE") + "\n" +
                                   "§7Doctor vinculado: §f" + linkedDoctorMinecraftName + "§r\n" +
                                   "§7Estado activo: " + currentState.getColor() + currentState.getDisplayName() + "§r\n" +
                                   "§7Tensión hormonal: " + String.format("%.2f", currentTension) + " | Afecto: " + String.format("%.2f", currentAfecto) + "\n" +
                                   "§7Entidad en juego: " + (activeEntity != null && activeEntity.isAlive() ? "§aPRESENTE (HP: " + activeEntity.getHealth() + ")" : "§cVINCULANDO...");
                    player.sendSystemMessage(Component.literal(status));
                    return 1;
                }))
                .then(Commands.literal("inv").executes(ctx -> {
                    ServerPlayer player = ctx.getSource().getPlayer();
                    if (player == null) return 0;
                    player.openMenu(new SimpleMenuProvider(
                        (syncId, playerInv, p) -> new ChestMenu(MenuType.GENERIC_9x4, syncId, playerInv, KaltsitWorldInteractor.companionInventory, 4),
                        Component.literal("Inventario de Kal'tsit (36 slots)")
                    ));
                    return 1;
                }))
                .then(Commands.literal("construye").then(Commands.argument("que", StringArgumentType.greedyString()).executes(ctx -> {
                    ServerPlayer player = ctx.getSource().getPlayer();
                    if (player == null || !isLinkedDoctor(player)) return 0;
                    JsonObject o = new JsonObject();
                    o.addProperty("action", "build_structure");
                    o.addProperty("name", StringArgumentType.getString(ctx, "que"));
                    runOrder(ctx.getSource().getServer(), player, o);
                    return 1;
                })))
                .then(Commands.literal("obra")
                    .executes(ctx -> order(ctx.getSource().getPlayer(), "build_status"))
                    .then(Commands.literal("pausa").executes(ctx -> order(ctx.getSource().getPlayer(), "pause_build")))
                    .then(Commands.literal("sigue").executes(ctx -> order(ctx.getSource().getPlayer(), "resume_build")))
                    .then(Commands.literal("cancela").executes(ctx -> order(ctx.getSource().getPlayer(), "cancel_build"))))
                .then(Commands.literal("estructuras").executes(ctx -> order(ctx.getSource().getPlayer(), "list_structures")))
                .then(Commands.literal("quieta").executes(ctx -> order(ctx.getSource().getPlayer(), "stay")))
                .then(Commands.literal("sigueme").executes(ctx -> order(ctx.getSource().getPlayer(), "follow")))
                .then(Commands.literal("antorchas")
                    .then(Commands.literal("aqui_no").executes(ctx -> order(ctx.getSource().getPlayer(), "no_torch_zone"))
                        .then(Commands.argument("radio", IntegerArgumentType.integer(4, 128)).executes(ctx -> {
                            JsonObject o = new JsonObject();
                            o.addProperty("action", "no_torch_zone");
                            o.addProperty("radius", IntegerArgumentType.getInteger(ctx, "radio"));
                            ServerPlayer player = ctx.getSource().getPlayer();
                            if (player == null || !isLinkedDoctor(player)) return 0;
                            runOrder(ctx.getSource().getServer(), player, o);
                            return 1;
                        })))
                    .then(Commands.literal("aqui_si").executes(ctx -> order(ctx.getSource().getPlayer(), "allow_torches_here")))
                    .then(Commands.literal("auto").then(Commands.argument("activas", BoolArgumentType.bool()).executes(ctx -> {
                        JsonObject o = new JsonObject();
                        o.addProperty("action", "auto_torches");
                        o.addProperty("enabled", BoolArgumentType.getBool(ctx, "activas"));
                        ServerPlayer player = ctx.getSource().getPlayer();
                        if (player == null || !isLinkedDoctor(player)) return 0;
                        runOrder(ctx.getSource().getServer(), player, o);
                        return 1;
                    }))))
                .then(Commands.literal("inventario").executes(ctx -> {
                    ServerPlayer player = ctx.getSource().getPlayer();
                    if (player == null) return 0;
                    player.openMenu(new SimpleMenuProvider(
                        (syncId, playerInv, p) -> new ChestMenu(MenuType.GENERIC_9x4, syncId, playerInv, KaltsitWorldInteractor.companionInventory, 4),
                        Component.literal("Inventario de Kal'tsit (36 slots)")
                    ));
                    return 1;
                }))
            );
        });

        // Chat listener: Forwards chat from ANY player, tagging whether it comes from the trusted Doctor
        ServerMessageEvents.CHAT_MESSAGE.register((message, sender, params) -> {
            String text = message.signedContent();
            String name = sender.getName().getString();
            boolean isDoctor = isLinkedDoctor(sender);
            String json = "{\"type\":\"chat\",\"sender\":\"" + escape(name) + "\",\"is_doctor\":" + isDoctor + ",\"text\":\"" + escape(text) + "\"}";
            KaltsitBridge.send(json);
        });

        // Each world keeps what the Doctor told her (torch zones); a build never crosses worlds.
        ServerLifecycleEvents.SERVER_STARTED.register(server -> {
            KaltsitSettings.load(server);
            KaltsitBuilder.reset();
            KaltsitPreview.clear();
            KaltsitBuilder.sharedDir();
            lastCapabilities = null;
            activeEntity = null;
            respawnAt = -1;
            joinedTick = -1;
            // She stays where she was left (a build in progress cannot be resumed after a restart).
            String saved = KaltsitSettings.get().companionState;
            buildLostNotice = "BUILDING".equals(saved);
            currentState = "GUARDING".equals(saved) ? CompanionState.GUARDING
                : ("IDLE".equals(saved) || buildLostNotice) ? CompanionState.IDLE : CompanionState.FOLLOWING;
            previousState = currentState;
            forcedChunks.clear();
            forcedChunks.addAll(KaltsitSettings.get().forcedChunks);
            forcedLevel = KaltsitSettings.forcedLevel(server);
        });

        // Ghosts left by a preview of an older session go; a second Kal'tsit never stays.
        ServerEntityEvents.ENTITY_LOAD.register((entity, level) -> {
            KaltsitPreview.onEntityLoad(entity);
            if (entity instanceof KaltsitEntity other && activeEntity != null && activeEntity.isAlive() && activeEntity != other) {
                other.discard();
            }
        });

        // End Server Tick
        ServerTickEvents.END_SERVER_TICK.register(server -> {
            tickCounter++;
            if (tickCounter % 200 == 0) sendCapabilities(server);
            ServerPlayer primaryPlayer = null;
            for (ServerPlayer p : server.getPlayerList().getPlayers()) {
                if (isLinkedDoctor(p)) {
                    primaryPlayer = p;
                    break;
                }
            }
            if (primaryPlayer == null && !server.getPlayerList().getPlayers().isEmpty()) {
                primaryPlayer = server.getPlayerList().getPlayers().get(0);
            }
            cachedPrimaryPlayer = primaryPlayer;
            if (primaryPlayer == null) {
                joinedTick = -1;
                return;
            }
            if (joinedTick < 0) joinedTick = tickCounter;

            // Maintain Kal'tsit in Tab List (Player list) like a real player
            if (tickCounter % 100 == 1) {
                updateTabList(server, (ServerLevel) primaryPlayer.level());
            }

            // AUTOMATIC COMPANION LIFECYCLE:
            // Ensure Kal'tsit is alive and linked to her Doctor without any admin commands!
            if (activeEntity == null || !activeEntity.isAlive()) {
                activeEntity = findKaltsit(server);
                if (activeEntity == null) {
                    if (respawnAt >= 0) {
                        if (tickCounter >= respawnAt) {
                            respawnAt = -1;
                            respawn(server, primaryPlayer);
                        }
                    } else if (tickCounter - joinedTick > 60 && primaryPlayer.isAlive()) {
                        // Three seconds after joining: her chunk (if she stayed somewhere) has loaded.
                        spawnOrAttach((ServerLevel) primaryPlayer.level(), primaryPlayer);
                    }
                }
            }
            if (buildLostNotice && tickCounter - joinedTick == 100) {
                buildLostNotice = false;
                primaryPlayer.sendSystemMessage(Component.literal("§e[Kal'tsit]§r El mundo se cerró a mitad de la obra: me quedé junto a ella. Pídemela otra vez para terminarla (lo construido se respeta)."));
            }

            // Dimension synchronization: she follows the Doctor across dimensions only while following.
            if (activeEntity != null && activeEntity.isAlive() && currentState == CompanionState.FOLLOWING) {
                if (activeEntity.level() != primaryPlayer.level()) {
                    activeEntity.teleportTo((ServerLevel) primaryPlayer.level(), primaryPlayer.getX() + 1.2, primaryPlayer.getY(), primaryPlayer.getZ() + 1.2, Set.of(), activeEntity.getYRot(), activeEntity.getXRot(), false);
                }
            }

            // Flush pending chat messages from Kal'tsit's mind
            String chat;
            while ((chat = pendingChats.poll()) != null) {
                String clean = chat.replace("\\n", "\n").replace("\\\"", "\"").replace("\\\\", "\\");
                for (String line : clean.split("\n")) {
                    String t = line.trim();
                    if (!t.isEmpty()) {
                        server.getPlayerList().broadcastSystemMessage(Component.literal("§b<Kal'tsit>§r " + t), false);
                    }
                }
            }

            // Process commands decided and emitted by Kal'tsit's mind
            String cmdJson;
            while ((cmdJson = pendingCommands.poll()) != null) {
                executeAICommand(server, primaryPlayer, cmdJson);
            }

            if (tickCounter % 40 == 0) updateChunkLoading(primaryPlayer);
            if (tickCounter % 100 == 0) KaltsitBuilder.reportProgress();
            if (activeEntity == null || !activeEntity.isAlive()) return;

            // Autonomous State Machine with realistic Steve actions modulated by hormones
            runAutonomousStateMachine(primaryPlayer);

            // Telemetry & instincts every 2 ticks
            if (tickCounter % 2 == 0) {
                sendTelemetry(activeEntity, primaryPlayer);
                executeReflexAction(activeEntity, primaryPlayer, latestAction);
            }
        });
    }

    /** A /kaltsit command that is a plain order (only the Doctor gives them). */
    private static int order(ServerPlayer player, String action) {
        if (player == null || !isLinkedDoctor(player)) return 0;
        JsonObject o = new JsonObject();
        o.addProperty("action", action);
        runOrder(player.level().getServer(), player, o);
        return 1;
    }

    public static void updateTabList(MinecraftServer server, ServerLevel level) {
        if (server == null || level == null) return;
        try {
            if (tabDummyPlayer == null) {
                GameProfile profile = new GameProfile(KALTSIT_UUID, "Kal'tsit");
                tabDummyPlayer = new ServerPlayer(server, level, profile, ClientInformation.createDefault());
            }
            ClientboundPlayerInfoUpdatePacket packet = ClientboundPlayerInfoUpdatePacket.createPlayerInitializing(List.of(tabDummyPlayer));
            server.getPlayerList().broadcastAll(packet);
        } catch (Exception ignored) {}
    }

    private static void say(ServerPlayer player, Component c) {
        if (player != null && c != null) player.sendSystemMessage(c);
    }

    private static int intOf(JsonObject o, String k, int def) {
        try {
            return o.has(k) ? o.get(k).getAsInt() : def;
        } catch (Exception e) {
            return def;
        }
    }

    private static String strOf(JsonObject o, String k) {
        try {
            return o.has(k) && !o.get(k).isJsonNull() ? o.get(k).getAsString() : "";
        } catch (Exception e) {
            return "";
        }
    }

    /** Starts building what an order asks for (a template, a blueprint or a saved structure). */
    public static void startBuilding(MinecraftServer server, ServerPlayer player, JsonObject o) {
        if (KaltsitBuilder.current() != null) {
            say(player, Component.literal("§e[Kal'tsit]§r Ya estoy con «" + KaltsitBuilder.current().plan.name + "»: dime que la cancele antes de empezar otra."));
            return;
        }
        try {
            KaltsitBuilder.Plan plan = KaltsitBuilder.planFor(server, o);
            // First the blueprint, free and touching nothing; she builds when the Doctor says so.
            if (!o.has("preview") || o.get("preview").getAsBoolean()) {
                say(player, KaltsitPreview.show(player, plan));
                return;
            }
            Component problem = KaltsitBuilder.start(player, plan);
            if (problem != null) {
                say(player, problem);
                return;
            }
            setState(CompanionState.BUILDING, "Obra «" + plan.name + "»");
        } catch (Exception e) {
            say(player, Component.literal("§c[Kal'tsit]§r No puedo construir eso: " + e.getMessage()));
        }
    }

    private static void executeAICommand(MinecraftServer server, ServerPlayer player, String json) {
        if (player == null) return;
        ServerLevel level = (ServerLevel) player.level();

        if (activeEntity == null || !activeEntity.isAlive()) {
            spawnOrAttach(level, player);
        }
        if (activeEntity == null) return;

        JsonObject o;
        try {
            o = JsonParser.parseString(json).getAsJsonObject();
        } catch (Exception e) {
            System.out.println("[Kaltsit] Orden ilegible: " + json);
            return;
        }
        runOrder(server, player, o);
    }

    /** An order from her mind (or a /kaltsit command), as the JSON her model writes. */
    public static void runOrder(MinecraftServer server, ServerPlayer player, JsonObject o) {
        if (player == null) return;
        ServerLevel level = (ServerLevel) player.level();
        if (activeEntity == null || !activeEntity.isAlive()) spawnOrAttach(level, player);
        if (activeEntity == null) return;
        String action = strOf(o, "action");
        int count = intOf(o, "count", 0);
        String block = strOf(o, "block");

        switch (action) {
            case "place_bed" -> {
                if (KaltsitWorldInteractor.placeBed(level, activeEntity, player)) {
                    say(player, Component.literal("§a[Kal'tsit]§r Cama colocada. Será la mía: si caigo, reapareceré en ella."));
                }
            }
            case "set_bed" -> say(player, claimBed(player));
            case "chop_wood" -> {
                taskTargetCount = Math.max(0, count);
                taskCurrentCount = 0;
                taskTargetBlockName = block;
                stopBreaking(level, activeTargetLog);
                activeTargetLog = null;
                choppingProgress = 0;
                totalChoppedThisSession = 0;
                resetTask();
                setState(CompanionState.CHOPPING, "Orden de tala" + (taskTargetCount > 0 ? " (cuota: " + taskTargetCount + ")" : ""));
                say(player, Component.literal("§b[Kal'tsit]§r Procediendo a talar " + (taskTargetCount > 0 ? taskTargetCount + " bloques de madera..." : "madera en las inmediaciones...")));
            }
            case "store_in_chest" -> {
                int stored = KaltsitWorldInteractor.storeItemsInNearbyChest(level, activeEntity, player);
                say(player, Component.literal(stored > 0 ? "§a[Kal'tsit]§r He guardado " + stored + " pilas de materiales en el cofre." : "§c[Kal'tsit]§r No encontré ningún cofre accesible cerca o no tengo materiales que guardar."));
            }
            case "take_from_chest" -> {
                int taken = KaltsitWorldInteractor.takeItemsFromNearbyChest(level, activeEntity, player, block, count > 0 ? count : 16);
                say(player, Component.literal(taken > 0 ? "§a[Kal'tsit]§r Retirados " + taken + " objetos del cofre." : "§c[Kal'tsit]§r No encontré el material solicitado en los cofres cercanos."));
            }
            case "place_crafting_table" -> {
                if (KaltsitWorldInteractor.placeCraftingTable(level, activeEntity, player)) {
                    say(player, Component.literal("§a[Kal'tsit]§r Mesa de trabajo instalada."));
                }
            }
            case "place_torch" -> {
                if (!KaltsitSettings.torchAllowed(level, player.blockPosition())) {
                    say(player, Component.literal("§e[Kal'tsit]§r Aquí me pediste no poner antorchas. Dime «ya puedes poner antorchas aquí» si cambiaste de idea."));
                } else if (KaltsitWorldInteractor.placeTorch(level, activeEntity, player)) {
                    say(player, Component.literal("§a[Kal'tsit]§r Antorcha colocada."));
                }
            }
            case "no_torch_zone" -> {
                int r = count > 0 ? count : intOf(o, "radius", 24);
                KaltsitSettings.addNoTorchZone(level, player.blockPosition(), r);
                BlockPos p = player.blockPosition();
                say(player, Component.literal("§b[Kal'tsit]§r Entendido: no pondré antorchas a menos de " + Math.max(4, Math.min(128, r)) + " bloques de aquí (" + p.getX() + ", " + p.getY() + ", " + p.getZ() + ")."));
            }
            case "allow_torches_here" -> {
                int n = KaltsitSettings.removeZonesAt(level, player.blockPosition());
                say(player, Component.literal(n > 0 ? "§b[Kal'tsit]§r Hecho: aquí vuelvo a poder poner antorchas." : "§7[Kal'tsit]§r Aquí no había ninguna zona sin antorchas."));
            }
            case "auto_torches" -> {
                boolean on = !o.has("enabled") || o.get("enabled").getAsBoolean();
                KaltsitSettings.setAutoTorches(on);
                say(player, Component.literal(on ? "§b[Kal'tsit]§r Volveré a iluminar los sitios oscuros por mi cuenta." : "§b[Kal'tsit]§r De acuerdo: ya no pondré antorchas por mi cuenta, solo cuando me lo pidas."));
            }
            case "mine" -> {
                taskTargetCount = count > 0 ? count : 1;
                taskCurrentCount = 0;
                taskTargetBlockName = block;
                stopBreaking(level, activeTargetMining);
                resetTask();
                BlockPos target = KaltsitWorldInteractor.getMiningTarget(level, player, activeEntity, taskTargetBlockName);
                if (target != null) {
                    activeTargetMining = target;
                    activeMiningStandPos = KaltsitWorldInteractor.findWalkableStandingPos(level, target, activeEntity.blockPosition());
                    miningProgress = 0;
                    lastBrokenBlockType = null;
                    setState(CompanionState.MINING, "Orden de minería (cuota: " + taskTargetCount + ")");
                    String name = level.getBlockState(target).getBlock().getName().getString();
                    say(player, Component.literal("§b[Kal'tsit]§r Procediendo a extraer " + (taskTargetCount > 1 ? taskTargetCount + " bloques de " : "") + name + "..."));
                } else {
                    say(player, Component.literal("§c[Kal'tsit]§r No diviso ningún bloque de '" + (block.isEmpty() ? "tipo objetivo" : block) + "' en las inmediaciones."));
                }
            }
            case "follow" -> setState(CompanionState.FOLLOWING, "Orden de seguimiento");
            case "guard" -> setState(CompanionState.GUARDING, "Orden de guardia");
            case "stay" -> {
                setState(CompanionState.IDLE, "Me quedo aquí");
                say(player, Component.literal("§b[Kal'tsit]§r Me quedo aquí. Dime «sígueme» cuando quieras que vuelva contigo."));
            }
            case "attack" -> setState(CompanionState.COMBAT, "Orden de combate");
            case "build_structure", "build" -> startBuilding(server, player, o);
            case "build_confirm" -> {
                Component problem = KaltsitPreview.confirm(player);
                if (problem != null) say(player, problem);
                else setState(CompanionState.BUILDING, "Obra en marcha");
            }
            case "build_discard" -> say(player, KaltsitPreview.discard());
            case "preview_move" -> say(player, KaltsitPreview.move(player, intOf(o, "right", 0), intOf(o, "up", 0), intOf(o, "forward", 0)));
            case "preview_rotate" -> say(player, KaltsitPreview.rotate(player, intOf(o, "turns", 1)));
            case "pause_build" -> say(player, KaltsitBuilder.pause());
            case "resume_build" -> {
                say(player, KaltsitBuilder.resume());
                if (KaltsitBuilder.current() != null) setState(CompanionState.BUILDING, "Reanudo la obra");
            }
            case "cancel_build" -> {
                say(player, KaltsitBuilder.cancel(activeEntity instanceof KaltsitEntity k ? k : null));
                if (currentState == CompanionState.BUILDING) setState(CompanionState.FOLLOWING, "Obra cancelada");
            }
            case "build_status" -> say(player, KaltsitBuilder.status());
            case "list_structures" -> say(player, structuresMessage(server));
            default -> System.out.println("[Kaltsit] Acción desconocida: " + action);
        }
    }

    private static Component structuresMessage(MinecraftServer server) {
        List<String> saved = KaltsitBuilder.structureNames(server);
        return Component.literal("§b[Kal'tsit]§r Sé construir: " + String.join(", ", KaltsitBuilder.TEMPLATES)
            + (saved.isEmpty() ? ". No veo estructuras guardadas: guarda una con un Bloque de Estructura (modo Guardar) y pídemela por su nombre."
            : ". Estructuras guardadas: " + String.join(", ", saved) + "."));
    }

    /** Tells the app what she can build, so her mind knows the names (when they change). */
    private static void sendCapabilities(MinecraftServer server) {
        if (!KaltsitBridge.isConnected()) {
            lastCapabilities = null; // Sent again when the app connects
            return;
        }
        List<String> saved = KaltsitBuilder.structureNames(server);
        String json = "{\"type\":\"capabilities\",\"templates\":[" + String.join(",", KaltsitBuilder.TEMPLATES.stream().map(t -> "\"" + escape(t) + "\"").toList())
            + "],\"structures\":[" + String.join(",", saved.stream().map(t -> "\"" + escape(t) + "\"").toList()) + "]}";
        if (!json.equals(lastCapabilities)) {
            lastCapabilities = json;
            KaltsitBridge.send(json);
        }
    }

    private static boolean isLog(Level level, BlockPos pos) {
        return level != null && pos != null && level.getBlockState(pos).is(BlockTags.LOGS);
    }

    /** The nearest log of a natural tree she has not given up on (the base of a trunk first). */
    private static BlockPos findNearestTreeLog(Level level, BlockPos origin, int radius) {
        BlockPos best = null;
        double bestDist = Double.MAX_VALUE;
        for (int dx = -radius; dx <= radius; dx++) {
            for (int dy = -2; dy <= 5; dy++) {
                for (int dz = -radius; dz <= radius; dz++) {
                    BlockPos p = origin.offset(dx, dy, dz);
                    if (!isLog(level, p) || unreachable.contains(p)) continue;
                    boolean isTrunkBase = !isLog(level, p.below());
                    double d = p.distSqr(origin) - (isTrunkBase ? 20.0 : 0.0);
                    if (d < bestDist && KaltsitWorldInteractor.isNaturalTreeLog(level, p)) {
                        bestDist = d;
                        best = p.immutable();
                    }
                }
            }
        }
        return best;
    }

    /** The Doctor this tick (null when nobody is in the world). */
    public static ServerPlayer cachedDoctor() {
        return cachedPrimaryPlayer;
    }

    /** Starts a new piece of work: forgets the blocks she gave up on and the drops she skipped. */
    private static void resetTask() {
        unreachable.clear();
        approachTicks = 0;
        collectTicks = 0;
        collectPause = 0;
    }

    /**
     * Walks to what lies on the ground nearby, like a player gathering what she broke. True
     * while she is on her way to one; gives up on drops she cannot get to for a while.
     */
    private static boolean collectDrops(Mob mob, double radius) {
        if (collectPause > 0) {
            collectPause--;
            return false;
        }
        ItemEntity drop = KaltsitWorldInteractor.nearestDrop(mob, radius);
        if (drop == null) {
            collectTicks = 0;
            return false;
        }
        if (++collectTicks > 100) {
            collectTicks = 0;
            collectPause = 200;
            return false;
        }
        mob.getNavigation().moveTo(drop, 1.1);
        return true;
    }

    /** Walks towards a block until it is within reach; false (and gives up) after 8 s stuck. */
    private static boolean approach(Mob mob, Level level, BlockPos target, BlockPos stand) {
        if (KaltsitWorldInteractor.canReach(mob, target)) {
            approachTicks = 0;
            return true;
        }
        if (++approachTicks > 160) {
            unreachable.add(target.immutable());
            approachTicks = 0;
            return false;
        }
        if (stand != null) {
            mob.getNavigation().moveTo(stand.getX() + 0.5, stand.getY(), stand.getZ() + 0.5, 1.05);
        } else {
            mob.getNavigation().moveTo(target.getX() + 0.5, target.getY(), target.getZ() + 0.5, 1.05);
        }
        return false;
    }

    /** One tick of breaking `target` (cracks, swings, hit sounds). True when it broke. */
    static boolean breakTick(Mob mob, ServerLevel level, BlockPos target, int progress) {
        mob.getNavigation().stop();
        mob.getLookControl().setLookAt(target.getX() + 0.5, target.getY() + 0.5, target.getZ() + 0.5);
        if (progress % 4 == 0) {
            mob.swing(InteractionHand.MAIN_HAND);
            BlockState bs = level.getBlockState(target);
            level.playSound(null, target, bs.getSoundType().getHitSound(), net.minecraft.sounds.SoundSource.BLOCKS, 0.5f, 0.8f);
        }
        int required = KaltsitWorldInteractor.breakTicks(mob, level, target);
        if (required < 0) {
            unreachable.add(target.immutable());
            return false;
        }
        int stage = Math.min(9, (int) ((progress / (float) required) * 10.0f));
        level.destroyBlockProgress(mob.getId(), target, stage);
        if (progress + 1 >= required) {
            level.destroyBlockProgress(mob.getId(), target, -1);
            KaltsitWorldInteractor.breakBlockLikePlayer(level, mob, target);
            return true;
        }
        return false;
    }

    private static void stopBreaking(ServerLevel level, BlockPos pos) {
        if (pos != null && activeEntity != null) level.destroyBlockProgress(activeEntity.getId(), pos, -1);
    }

    private static CompanionState restingState() {
        if (previousState == CompanionState.GUARDING || previousState == CompanionState.IDLE) return previousState;
        if (previousState == CompanionState.BUILDING && KaltsitBuilder.current() != null) return CompanionState.BUILDING;
        return CompanionState.FOLLOWING;
    }

    private static void runAutonomousStateMachine(ServerPlayer player) {
        Level level = activeEntity.level();
        if (!(level instanceof ServerLevel serverLevel) || !(activeEntity instanceof Mob mob)) return;
        KaltsitEntity self = activeEntity instanceof KaltsitEntity k ? k : null;

        boolean inDanger = activeEntity.isInLava() || (activeEntity.isInWater() && activeEntity.getAirSupply() < 60) || activeEntity.isOnFire();
        if (inDanger && currentState != CompanionState.ALERT) {
            stopBreaking(serverLevel, activeTargetLog);
            stopBreaking(serverLevel, activeTargetMining);
            setState(CompanionState.ALERT, "Supervivencia ambiental");
            return;
        } else if (!inDanger && currentState == CompanionState.ALERT) {
            setState(previousState != null ? previousState : CompanionState.FOLLOWING, "Peligro evadido");
            return;
        }

        // A creeper about to explode: she runs, whatever she was doing.
        Creeper boom = KaltsitWorldInteractor.swellingCreeper(activeEntity, 5.0);
        if (boom != null) {
            net.minecraft.world.phys.Vec3 away = activeEntity.position().subtract(boom.position()).normalize().scale(8.0);
            mob.getNavigation().moveTo(activeEntity.getX() + away.x, activeEntity.getY(), activeEntity.getZ() + away.z, 1.4);
            return;
        }

        // The Doctor is in another dimension: she carries on with what she was left doing.
        if (level != player.level()) {
            runAway(self, mob);
            return;
        }

        switch (currentState) {
            case COMBAT: {
                LivingEntity target = KaltsitWorldInteractor.findThreat(activeEntity, player, 12.0);
                if (target == null) {
                    setState(restingState(), "Área despejada");
                    break;
                }
                if (activeEntity.distanceTo(player) > 24.0) {
                    setState(CompanionState.FOLLOWING, "Vuelvo junto al Doctor");
                    break;
                }
                mob.setAggressive(true);
                mob.getLookControl().setLookAt(target, 30.0f, 30.0f);
                double d = activeEntity.distanceTo(target);
                if (d > 2.4) {
                    mob.getNavigation().moveTo(target, 1.15 + (currentTension * 0.25));
                } else {
                    mob.getNavigation().stop();
                }
                if (self != null) {
                    KaltsitWorldInteractor.attackLikePlayer(self, target);
                }
                break;
            }
            case CHOPPING: {
                if (activeEntity.distanceTo(player) > 40.0) {
                    stopBreaking(serverLevel, activeTargetLog);
                    activeTargetLog = null;
                    setState(CompanionState.FOLLOWING, "El Doctor se alejó de la zona de tala");
                    break;
                }
                if (activeTargetLog != null && !isLog(level, activeTargetLog)) {
                    stopBreaking(serverLevel, activeTargetLog);
                    activeTargetLog = null;
                    choppingProgress = 0;
                }
                // Between logs she gathers what fell, like a player.
                if (choppingProgress == 0 && collectDrops(mob, 6.0)) break;

                if (activeTargetLog == null) {
                    activeTargetLog = findNearestTreeLog(level, activeEntity.blockPosition(), 24);
                    choppingProgress = 0;
                    if (activeTargetLog == null) {
                        setState(CompanionState.FOLLOWING, "Tala completada");
                        player.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Tala completada (" + totalChoppedThisSession + " troncos). No veo más árboles a mi alcance."));
                        totalChoppedThisSession = 0;
                        break;
                    }
                }

                KaltsitWorldInteractor.updateEquippedGear(activeEntity, CompanionState.CHOPPING);
                BlockPos stand = KaltsitWorldInteractor.findWalkableStandingPos(level, activeTargetLog, activeEntity.blockPosition());
                if (!approach(mob, level, activeTargetLog, stand)) {
                    if (unreachable.contains(activeTargetLog)) {
                        stopBreaking(serverLevel, activeTargetLog);
                        activeTargetLog = null;
                        choppingProgress = 0;
                    }
                    break;
                }
                if (breakTick(mob, serverLevel, activeTargetLog, choppingProgress)) {
                    totalChoppedThisSession++;
                    taskCurrentCount++;
                    choppingProgress = 0;
                    if (taskTargetCount > 0 && taskCurrentCount >= taskTargetCount) {
                        activeTargetLog = null;
                        setState(CompanionState.FOLLOWING, "Cuota de tala alcanzada");
                        player.sendSystemMessage(Component.literal("§a[Kal'tsit]§r Cuota alcanzada: " + taskCurrentCount + "/" + taskTargetCount + " troncos."));
                        break;
                    }
                    BlockPos above = activeTargetLog.above();
                    activeTargetLog = isLog(level, above) ? above : null;
                } else if (unreachable.contains(activeTargetLog)) {
                    activeTargetLog = null;
                    choppingProgress = 0;
                } else {
                    choppingProgress++;
                }
                break;
            }
            case BUILDING: {
                // Something comes for her or the Doctor: the site waits (COMBAT returns to it).
                LivingEntity threat = KaltsitWorldInteractor.findThreat(activeEntity, player, 8.0);
                if (threat != null && threat instanceof Mob m && (m.getTarget() == activeEntity || m.getTarget() == player || threat == activeEntity.getLastHurtByMob())) {
                    setState(CompanionState.COMBAT, "Defiendo la obra");
                    break;
                }
                if (self == null || !KaltsitBuilder.tick(self, player)) {
                    finishedBuilding(player);
                }
                break;
            }
            case MINING: {
                if (activeEntity.distanceTo(player) > 40.0) {
                    stopBreaking(serverLevel, activeTargetMining);
                    activeTargetMining = null;
                    setState(CompanionState.FOLLOWING, "El Doctor se alejó de la zona de minería");
                    break;
                }
                if (miningProgress == 0 && collectDrops(mob, 5.0)) break;

                if (activeTargetMining == null || level.getBlockState(activeTargetMining).isAir() || unreachable.contains(activeTargetMining)) {
                    BlockPos next = null;
                    if (lastBrokenBlockType != null) {
                        if (activeTargetMining != null) next = KaltsitWorldInteractor.findAdjacentMatchingBlock(level, activeTargetMining, lastBrokenBlockType);
                        if (next == null) next = KaltsitWorldInteractor.findNearbyBlock(level, activeEntity.blockPosition(), lastBrokenBlockType, 8);
                        if (next != null && unreachable.contains(next)) next = null;
                    }
                    stopBreaking(serverLevel, activeTargetMining);
                    miningProgress = 0;
                    if (next == null) {
                        activeTargetMining = null;
                        setState(CompanionState.FOLLOWING, "Extracción completada");
                        if (taskCurrentCount > 0) {
                            player.sendSystemMessage(Component.literal("§a[Kal'tsit]§r Extraje " + taskCurrentCount + " bloques; no veo más a mi alcance."));
                        }
                        break;
                    }
                    activeTargetMining = next.immutable();
                    activeMiningStandPos = KaltsitWorldInteractor.findWalkableStandingPos(level, next, activeEntity.blockPosition());
                }

                KaltsitWorldInteractor.updateEquippedGear(activeEntity, CompanionState.MINING);
                if (activeMiningStandPos == null || !KaltsitWorldInteractor.isWalkable(level, activeMiningStandPos)) {
                    activeMiningStandPos = KaltsitWorldInteractor.findWalkableStandingPos(level, activeTargetMining, activeEntity.blockPosition());
                }
                if (!approach(mob, level, activeTargetMining, activeMiningStandPos)) break;

                BlockState before = level.getBlockState(activeTargetMining);
                if (breakTick(mob, serverLevel, activeTargetMining, miningProgress)) {
                    lastBrokenBlockType = before.getBlock();
                    miningProgress = 0;
                    taskCurrentCount++;
                    if (taskTargetCount > 0 && taskCurrentCount >= taskTargetCount) {
                        activeTargetMining = null;
                        setState(CompanionState.FOLLOWING, "Cuota de minería alcanzada");
                        player.sendSystemMessage(Component.literal("§a[Kal'tsit]§r Cuota alcanzada: " + taskCurrentCount + "/" + taskTargetCount + " bloques de " + before.getBlock().getName().getString() + "."));
                    }
                } else if (!unreachable.contains(activeTargetMining)) {
                    miningProgress++;
                }
                break;
            }
            case GUARDING: {
                LivingEntity threat = KaltsitWorldInteractor.findThreat(activeEntity, player, 8.0);
                if (threat != null) {
                    setState(CompanionState.COMBAT, "Intruso en mi puesto");
                    break;
                }
                mob.setAggressive(false);
                mob.getNavigation().stop();
                lowHealthCare(self, player);
                break;
            }
            case FOLLOWING: {
                LivingEntity threat = KaltsitWorldInteractor.findThreat(activeEntity, player, 10.0);
                if (threat != null) {
                    setState(CompanionState.COMBAT, "Protegiendo al Doctor");
                    break;
                }
                mob.setAggressive(false);
                // Near the Doctor she picks up what is lying around; otherwise she follows.
                boolean collecting = activeEntity.distanceTo(player) < 10.0 && collectDrops(mob, 5.0);
                if (!collecting) {
                    double followSpeed = 0.95 + (currentTension * 0.35);
                    double stopDist = currentAfecto > 0.4 ? 2.5 : 3.8;
                    KaltsitWorldInteractor.followPlayer(activeEntity, player, followSpeed, stopDist);
                }
                lowHealthCare(self, player);
                autoCheckCooldown++;
                if (autoCheckCooldown % 120 == 0) {
                    boolean placed = KaltsitWorldInteractor.autoPlaceTorchInDarkness(level, activeEntity, player);
                    if (placed) {
                        player.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Está demasiado oscuro aquí; dejé una antorcha."), true);
                    }
                }
                break;
            }
            case ALERT: {
                // Lava or fire: out to safe ground, or into water if burning and there is some near.
                if (activeEntity.isInLava() || activeEntity.isOnFire()) {
                    BlockPos way = activeEntity.isOnFire() && !activeEntity.isInLava() ? KaltsitWorldInteractor.nearbyWater(level, activeEntity.blockPosition(), 6) : null;
                    if (way == null) way = KaltsitWorldInteractor.safeGround(level, activeEntity.blockPosition(), 8);
                    if (way != null && (activeEntity.tickCount % 10 == 0 || mob.getNavigation().isDone())) {
                        mob.getNavigation().moveTo(way.getX() + 0.5, way.getY(), way.getZ() + 0.5, 1.4);
                    }
                }
                // Drowning: her swimming takes her to the nearest air (KaltsitEntity).
                break;
            }
            case IDLE: {
                // Stays where she was told; defends herself (and the Doctor) if attacked.
                LivingEntity threat = KaltsitWorldInteractor.findThreat(activeEntity, player, 6.0);
                if (threat != null && (threat == activeEntity.getLastHurtByMob() || (threat instanceof Mob m && (m.getTarget() == activeEntity || m.getTarget() == player)))) {
                    setState(CompanionState.COMBAT, "Me defiendo");
                    break;
                }
                mob.setAggressive(false);
                mob.getNavigation().stop();
                lowHealthCare(self, player);
                break;
            }
            default:
                break;
        }
    }

    /** Eats when hurt and out of a fight, with the animation (she checks every 2 s). */
    private static void lowHealthCare(KaltsitEntity self, ServerPlayer player) {
        if (self == null || tickCounter % 40 != 0) return;
        if (self.getHealth() < self.getMaxHealth() * 0.75f && self.startEating()) {
            player.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Comiendo algo para recuperarme (" + (int) self.getHealth() + "/" + (int) self.getMaxHealth() + ")."), true);
        }
    }

    /** Her body, wherever it is loaded (any dimension). */
    private static LivingEntity findKaltsit(MinecraftServer server) {
        for (ServerLevel l : server.getAllLevels()) {
            List<? extends KaltsitEntity> found = l.getEntities(KALTSIT_ENTITY, e -> e.isAlive());
            if (!found.isEmpty()) return found.get(0);
        }
        return null;
    }

    /** She died: back in five seconds, at her bed (survival) or next to the Doctor. */
    public static void onDeath() {
        activeEntity = null;
        respawnAt = tickCounter + 100;
        KaltsitBuilder.Job j = KaltsitBuilder.current();
        if (j != null) j.paused = true;
    }

    private static void respawn(MinecraftServer server, ServerPlayer doctor) {
        if (doctor.isCreative()) {
            spawnOrAttach((ServerLevel) doctor.level(), doctor);
            return;
        }
        ServerLevel level;
        Vec3 at;
        String where;
        KaltsitSettings.BedSpot bed = KaltsitSettings.bed(server);
        if (bed != null) {
            level = bed.level();
            BlockState bs = level.getBlockState(bed.pos());
            at = BedBlock.findStandUpPosition(KALTSIT_ENTITY, level, bed.pos(), bs.getValue(BedBlock.FACING), 0f)
                .orElse(Vec3.atBottomCenterOf(bed.pos().above()));
            where = "en mi cama (" + bed.pos().getX() + ", " + bed.pos().getY() + ", " + bed.pos().getZ() + ")";
        } else {
            // Like a player without a bed: the world's spawn point.
            var spawn = server.overworld().getRespawnData();
            level = server.getLevel(spawn.dimension()) != null ? server.getLevel(spawn.dimension()) : server.overworld();
            BlockPos sp = spawn.pos();
            int y = level.getHeight(Heightmap.Types.WORLD_SURFACE, sp.getX(), sp.getZ());
            at = new Vec3(sp.getX() + 0.5, y, sp.getZ() + 0.5);
            where = "en el punto de aparición del mundo (" + sp.getX() + ", " + y + ", " + sp.getZ() + ")"
                + (KaltsitSettings.get().bedPos != null ? ": mi cama ya no está" : ": no tengo cama propia")
                + ". Ponme una o dime «esta es tu cama» junto a una";
        }
        KaltsitEntity companion = new KaltsitEntity(KALTSIT_ENTITY, level);
        companion.setPos(at.x, at.y, at.z);
        companion.setCustomName(Component.literal("§bKal'tsit§r"));
        companion.setCustomNameVisible(true);
        level.addFreshEntity(companion);
        activeEntity = companion;
        setState(CompanionState.IDLE, "Reaparecida");
        doctor.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Reaparecí " + where + ". Ven a buscarme o dime «sígueme»."));
    }

    /** «Esta es tu cama»: the bed the Doctor looks at, or the nearest one to him. */
    private static Component claimBed(ServerPlayer player) {
        ServerLevel level = (ServerLevel) player.level();
        BlockPos found = null;
        if (player.pick(6.0, 0f, false) instanceof BlockHitResult hit && level.getBlockState(hit.getBlockPos()).getBlock() instanceof BedBlock) {
            found = hit.getBlockPos();
        }
        for (int r = 1; found == null && r <= 5; r++) {
            for (BlockPos p : BlockPos.betweenClosed(player.blockPosition().offset(-r, -2, -r), player.blockPosition().offset(r, 2, r))) {
                if (level.getBlockState(p).getBlock() instanceof BedBlock) {
                    found = p.immutable();
                    break;
                }
            }
        }
        if (found == null) return Component.literal("§e[Kal'tsit]§r No veo ninguna cama cerca. Mira hacia una (o ponte al lado) y repítemelo.");
        KaltsitSettings.setBed(level, found);
        return Component.literal("§b[Kal'tsit]§r Esta será mi cama (" + found.getX() + ", " + found.getY() + ", " + found.getZ() + "). Si caigo en supervivencia, reapareceré aquí.");
    }

    /** The build ended: next to the Doctor she follows him; left alone, she waits by the site. */
    private static void finishedBuilding(ServerPlayer player) {
        boolean away = activeEntity == null || player == null || activeEntity.level() != player.level() || activeEntity.distanceTo(player) > 24.0;
        if (away) {
            setState(CompanionState.IDLE, "Te espero junto a la obra");
            if (player != null) player.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Me quedo junto a la obra hasta que vengas o me digas «sígueme»."));
        } else {
            setState(CompanionState.FOLLOWING, "Obra terminada");
        }
    }

    /** What she does while the Doctor is in another dimension: only what she was left doing. */
    private static void runAway(KaltsitEntity self, Mob mob) {
        switch (currentState) {
            case BUILDING -> {
                if (self == null || !KaltsitBuilder.tick(self, null)) finishedBuilding(null);
            }
            case COMBAT -> {
                LivingEntity t = KaltsitWorldInteractor.findThreat(activeEntity, null, 12.0);
                if (t == null) setState(restingState(), "Área despejada");
                else {
                    mob.getNavigation().moveTo(t, 1.15);
                    if (self != null) KaltsitWorldInteractor.attackLikePlayer(self, t);
                }
            }
            case IDLE, GUARDING -> {
                LivingEntity t = KaltsitWorldInteractor.findThreat(activeEntity, null, currentState == CompanionState.GUARDING ? 8.0 : 5.0);
                if (t != null) setState(CompanionState.COMBAT, "Me defiendo");
                else mob.getNavigation().stop();
            }
            default -> mob.getNavigation().stop();
        }
    }

    private static long chunkKey(int x, int z) {
        return ((long) x << 32) | (z & 0xffffffffL);
    }

    /**
     * Where she stays on her own (building, told to stay, on guard, at her bed after dying) her
     * chunks keep running, like a chunk loader, so a build goes on with the Doctor far away.
     */
    private static void updateChunkLoading(ServerPlayer doctor) {
        Set<Long> want = new HashSet<>();
        ServerLevel level = null;
        if (activeEntity != null && activeEntity.isAlive() && currentState != CompanionState.FOLLOWING && activeEntity.level() instanceof ServerLevel l) {
            boolean away = doctor == null || l != doctor.level() || activeEntity.distanceTo(doctor) > 24.0;
            if (away || currentState == CompanionState.BUILDING) {
                level = l;
                List<BlockPos> centres = new ArrayList<>(List.of(activeEntity.blockPosition()));
                KaltsitBuilder.Job j = KaltsitBuilder.current();
                if (currentState == CompanionState.BUILDING && j != null && j.level == l) centres.add(j.center);
                for (BlockPos c : centres) {
                    int cx = c.getX() >> 4, cz = c.getZ() >> 4;
                    for (int dx = -1; dx <= 1; dx++)
                        for (int dz = -1; dz <= 1; dz++) want.add(chunkKey(cx + dx, cz + dz));
                }
            }
        }
        if (forcedLevel != null && forcedLevel != level) {
            for (long k : forcedChunks) forcedLevel.setChunkForced((int) (k >> 32), (int) k, false);
            forcedChunks.clear();
        }
        if (level != null) {
            for (long k : new ArrayList<>(forcedChunks)) {
                if (!want.contains(k)) {
                    level.setChunkForced((int) (k >> 32), (int) k, false);
                    forcedChunks.remove(k);
                }
            }
            for (long k : want) {
                if (forcedChunks.add(k)) level.setChunkForced((int) (k >> 32), (int) k, true);
            }
        }
        boolean changed = forcedLevel != level || !new HashSet<>(KaltsitSettings.get().forcedChunks).equals(forcedChunks);
        forcedLevel = level;
        if (changed) KaltsitSettings.setForced(level, forcedChunks);
    }

    private static LivingEntity findNearbyKaltsit(ServerPlayer player) {
        AABB box = player.getBoundingBox().inflate(128.0);
        List<KaltsitEntity> entities = player.level().getEntitiesOfClass(KaltsitEntity.class, box);
        if (!entities.isEmpty()) return entities.get(0);

        List<LivingEntity> any = player.level().getEntitiesOfClass(LivingEntity.class, box, e -> {
            if (e == player) return false;
            String name = e.getCustomName() != null ? e.getCustomName().getString() : "";
            return name.contains("Kaltsit") || name.contains("Kal'tsit");
        });
        return any.isEmpty() ? null : any.get(0);
    }

    private static void spawnOrAttach(ServerLevel level, ServerPlayer player) {
        LivingEntity existing = findNearbyKaltsit(player);
        if (existing != null) {
            activeEntity = existing;
            existing.teleportTo(player.getX() + 1.2, player.getY(), player.getZ() + 1.2);
            return;
        }

        KaltsitEntity companion = new KaltsitEntity(KALTSIT_ENTITY, level);
        companion.setPos(player.getX() + 1.2, player.getY(), player.getZ() + 1.2);
        companion.setCustomName(Component.literal("§bKal'tsit§r"));
        companion.setCustomNameVisible(true);
        companion.setPersistenceRequired();

        companion.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
        companion.setItemSlot(EquipmentSlot.OFFHAND, ItemStack.EMPTY);

        level.addFreshEntity(companion);
        activeEntity = companion;
        updateTabList(level.getServer(), level);
    }

    private static void sendTelemetry(LivingEntity entity, ServerPlayer player) {
        float health = entity.getHealth();
        float maxHealth = entity.getMaxHealth();
        float oxygen = entity.getAirSupply() / (float) Math.max(1, entity.getMaxAirSupply());
        boolean inWater = entity.isInWater();
        boolean inLava = entity.isInLava();
        boolean onFire = entity.isOnFire();
        float playerDist = (float) entity.distanceTo(player);

        String json = "{\"type\":\"telemetry\"," +
            "\"health\":" + health + "," +
            "\"max_health\":" + maxHealth + "," +
            "\"oxygen_ratio\":" + oxygen + "," +
            "\"in_water\":" + inWater + "," +
            "\"in_lava\":" + inLava + "," +
            "\"on_fire\":" + onFire + "," +
            "\"player_distance\":" + playerDist + "," +
            "\"state\":\"" + currentState.name() + "\"}";
        KaltsitBridge.send(json);
    }

    private static void executeReflexAction(LivingEntity entity, ServerPlayer player, String action) {
        Level level = entity.level();
        switch (action) {
            case "EscapeLava":
            case "EscapeFire":
                // Up, without losing the way out she is walking (the ALERT state steers it).
                entity.setDeltaMovement(entity.getDeltaMovement().x, Math.max(entity.getDeltaMovement().y, 0.3), entity.getDeltaMovement().z);
                break;
            case "SwimUp":
                // Her own swimming already takes her to air (towards it, not straight up into a ceiling).
                if (!(entity instanceof KaltsitEntity)) entity.setDeltaMovement(entity.getDeltaMovement().x, 0.22, entity.getDeltaMovement().z);
                break;
            case "EatFood":
                // With the animation, from her inventory (never out of thin air).
                if (entity instanceof KaltsitEntity k && tickCounter % 40 == 0) k.startEating();
                break;
            default:
                break;
        }
    }

    private static String escape(String s) {
        if (s == null) return "";
        return s.replace("\\", "\\\\").replace("\"", "\\\"").replace("\n", "\\n").replace("\r", "");
    }
}
