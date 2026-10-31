package ai.kaltsit.companion;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import net.fabricmc.loader.api.FabricLoader;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.ListTag;
import net.minecraft.nbt.NbtAccounter;
import net.minecraft.nbt.NbtIo;
import net.minecraft.nbt.NbtUtils;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundSource;
import net.minecraft.world.Container;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.DoorBlock;
import net.minecraft.world.level.block.HorizontalDirectionalBlock;
import net.minecraft.world.level.block.LadderBlock;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.StairBlock;
import net.minecraft.world.level.block.WallTorchBlock;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BedPart;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.block.state.properties.DoorHingeSide;
import net.minecraft.world.level.block.state.properties.DoubleBlockHalf;
import net.minecraft.world.level.block.state.properties.Half;
import net.minecraft.world.level.storage.LevelResource;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;

import java.nio.file.Files;
import java.nio.file.Path;
import java.text.Normalizer;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.Deque;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Stream;

/**
 * How Kal'tsit builds, the way a player (and Baritone) does it. A plan comes from a template
 * (house, shelter, tower, wall, bridge, pen, platform), from a layered blueprint the model sends,
 * or from a structure the Doctor saved with a Structure Block (.nbt). She works out the
 * materials first and gathers them (her inventory, chests near the site, then the Doctor's
 * inventory), clears the site top-down, lays a foundation where the ground is missing, places
 * blocks bottom-up only where they have something to stand on, climbs on a pillar of her own
 * when a block is out of reach, takes the pillar down, and checks the result once more.
 */
public class KaltsitBuilder {

    // ── plans ──

    /**
     * A design in its own space: x to the right, z away from the Doctor (z = 0 is the front,
     * facing north), y up, y = 0 being the ground layer. Air cells are space to clear; cells
     * not in the plan are left as they are.
     */
    public static class Plan {
        public final String name;
        final Map<BlockPos, BlockState> cells = new LinkedHashMap<>();
        final Set<BlockPos> optional = new HashSet<>();
        boolean foundation = true;
        int width = 1;

        Plan(String name) {
            this.name = name;
        }

        void put(int x, int y, int z, BlockState s) {
            BlockPos p = new BlockPos(x, y, z);
            cells.put(p, s);
            optional.remove(p);
        }

        void put(int x, int y, int z, Block b) {
            put(x, y, z, b.defaultBlockState());
        }

        void air(int x, int y, int z) {
            put(x, y, z, Blocks.AIR.defaultBlockState());
        }

        /** Decoration skipped when she has none of it (torches inside, lanterns…). */
        void opt(int x, int y, int z, BlockState s) {
            put(x, y, z, s);
            optional.add(new BlockPos(x, y, z));
        }

        long solid() {
            return cells.values().stream().filter(s -> !s.isAir()).count();
        }

        /** How far it goes back from the front (z = 0). */
        int depth() {
            int d = 1;
            for (BlockPos p : cells.keySet()) d = Math.max(d, p.getZ() + 1);
            return d;
        }
    }

    public static final List<String> TEMPLATES = List.of("casa", "refugio", "torre", "muro", "puente", "corral", "plataforma");

    private static int clamp(int v, int lo, int hi) {
        return Math.max(lo, Math.min(hi, v));
    }

    /** The registry id for a name: itself when it is a block already, else a Spanish alias. */
    static String blockId(String name) {
        String n = name.trim().toLowerCase(Locale.ROOT).replace(' ', '_');
        Identifier exact = Identifier.tryParse(n.contains(":") ? n : "minecraft:" + n);
        if (exact != null && BuiltInRegistries.BLOCK.containsKey(exact)) return exact.toString();
        return KaltsitWorldInteractor.resolveBlockName(name, n);
    }

    /** A block by any name she understands (Spanish aliases included), or the fallback. */
    static Block block(String name, Block fallback) {
        if (name == null || name.isBlank()) return fallback;
        Identifier id = Identifier.tryParse(blockId(name));
        if (id == null) return fallback;
        Block b = BuiltInRegistries.BLOCK.getValue(id);
        return b == null || b == Blocks.AIR ? fallback : b;
    }

    private static Block byId(String path) {
        Identifier id = Identifier.tryParse("minecraft:" + path);
        if (id == null || !BuiltInRegistries.BLOCK.containsKey(id)) return null;
        return BuiltInRegistries.BLOCK.getValue(id);
    }

    /** The stairs (or slab, fence…) of a material: oak_planks → oak_stairs, bricks → brick_stairs. */
    static Block variant(Block base, String suffix, Block fallback) {
        String id = BuiltInRegistries.BLOCK.getKey(base).getPath();
        String single = suffix.substring(1);
        for (String c : new String[] {
            id.replace("_planks", suffix), id.replaceAll("bricks$", "brick" + suffix), id.replaceAll("tiles$", "tile" + suffix),
            id.replaceAll("_log$", suffix), id + suffix, id.replaceAll("_block$", "") + suffix, id.equals("stone") ? "stone_" + single : "",
        }) {
            if (c.isEmpty() || c.equals(id)) continue;
            Block b = byId(c);
            if (b != null) return b;
        }
        return fallback;
    }

    /** "oak" for oak_planks / oak_log, null for stone and friends. */
    private static String wood(Block b) {
        String id = BuiltInRegistries.BLOCK.getKey(b).getPath();
        for (String s : new String[] {"_planks", "_log", "_wood", "_stairs", "_slab"}) {
            if (id.endsWith(s)) return id.substring(0, id.length() - s.length());
        }
        return null;
    }

    private static Block woodPart(Block material, String part, Block fallback) {
        String w = wood(material);
        Block b = w == null ? null : byId(w + part);
        return b == null ? fallback : b;
    }

    private static BlockState stairs(Block b, Direction facing) {
        BlockState s = b.defaultBlockState();
        if (s.hasProperty(StairBlock.FACING)) s = s.setValue(StairBlock.FACING, facing).setValue(StairBlock.HALF, Half.BOTTOM);
        return s;
    }

    private static void door(Plan p, int x, int y, int z, Block door) {
        BlockState d = door.defaultBlockState();
        if (!d.hasProperty(DoorBlock.HALF)) {
            p.air(x, y, z);
            p.air(x, y + 1, z);
            return;
        }
        d = d.setValue(DoorBlock.FACING, Direction.SOUTH).setValue(DoorBlock.HINGE, DoorHingeSide.LEFT);
        p.put(x, y, z, d.setValue(DoorBlock.HALF, DoubleBlockHalf.LOWER));
        p.put(x, y + 1, z, d.setValue(DoorBlock.HALF, DoubleBlockHalf.UPPER));
    }

    private static BlockState wallTorch(Direction pointing) {
        return Blocks.WALL_TORCH.defaultBlockState().setValue(WallTorchBlock.FACING, pointing);
    }

    /** A house: floor, log corners, door, windows, inside lights and a pitched roof of stairs. */
    static Plan house(String name, int w, int l, int h, Block wall, Block floor, Block roof, Block pillar, boolean pitched, boolean windows) {
        w = clamp(w, 5, 21);
        l = clamp(l, 5, 21);
        h = clamp(h, 3, 6);
        Plan p = new Plan(name);
        p.width = w;
        int roofRise = pitched ? (w + 2) / 2 + 1 : 1;
        for (int x = -1; x <= w; x++)
            for (int z = -1; z <= l; z++)
                for (int y = 1; y <= h + roofRise; y++) p.air(x, y, z);
        for (int x = 0; x < w; x++)
            for (int z = 0; z < l; z++) p.put(x, 0, z, floor);
        for (int y = 1; y <= h; y++) {
            for (int x = 0; x < w; x++)
                for (int z = 0; z < l; z++) {
                    boolean edgeX = x == 0 || x == w - 1, edgeZ = z == 0 || z == l - 1;
                    if (!edgeX && !edgeZ) continue;
                    p.put(x, y, z, edgeX && edgeZ ? pillar : wall);
                }
        }
        if (windows) {
            for (int x = 2; x <= w - 3; x++) {
                if ((x - 2) % 3 != 0) continue;
                if (Math.abs(x - w / 2) > 1) p.put(x, 2, 0, Blocks.GLASS_PANE);
                p.put(x, 2, l - 1, Blocks.GLASS_PANE);
            }
            for (int z = 2; z <= l - 3; z++) {
                if ((z - 2) % 3 != 0) continue;
                p.put(0, 2, z, Blocks.GLASS_PANE);
                p.put(w - 1, 2, z, Blocks.GLASS_PANE);
            }
        }
        door(p, w / 2, 1, 0, woodPart(wall, "_door", woodPart(roof, "_door", Blocks.OAK_DOOR)));
        int ty = Math.min(3, h);
        p.opt(1, ty, l / 2, wallTorch(Direction.EAST));
        p.opt(w - 2, ty, l / 2, wallTorch(Direction.WEST));
        if (pitched) {
            Block st = variant(roof, "_stairs", Blocks.OAK_STAIRS);
            for (int s = 0; ; s++) {
                int xl = -1 + s, xr = w - s, y = h + 1 + s;
                if (xl > xr) break;
                if (xl == xr) {
                    for (int z = -1; z <= l; z++) p.put(xl, y, z, variant(roof, "_slab", roof));
                    break;
                }
                for (int z = -1; z <= l; z++) {
                    p.put(xl, y, z, stairs(st, Direction.EAST));
                    p.put(xr, y, z, stairs(st, Direction.WEST));
                }
                // The gable ends close the space between the two slopes, front and back.
                for (int x = xl + 1; x <= xr - 1; x++) {
                    p.put(x, y, 0, wall);
                    p.put(x, y, l - 1, wall);
                }
            }
        } else {
            for (int x = -1; x <= w; x++)
                for (int z = -1; z <= l; z++) p.put(x, h + 1, z, roof);
        }
        return p;
    }

    /** A watchtower: walls, ladder inside, a floor on top and battlements. */
    static Plan tower(String name, int h, Block wall) {
        h = clamp(h, 6, 24);
        Plan p = new Plan(name);
        p.width = 5;
        for (int x = -1; x <= 5; x++)
            for (int z = -1; z <= 5; z++)
                for (int y = 1; y <= h + 3; y++) p.air(x, y, z);
        for (int x = 0; x < 5; x++)
            for (int z = 0; z < 5; z++) p.put(x, 0, z, wall);
        for (int y = 1; y <= h; y++)
            for (int x = 0; x < 5; x++)
                for (int z = 0; z < 5; z++)
                    if (x == 0 || x == 4 || z == 0 || z == 4) p.put(x, y, z, wall);
        for (int y = 3; y < h; y += 4) {
            p.put(2, y, 4, Blocks.GLASS_PANE);
            p.put(0, y, 2, Blocks.GLASS_PANE);
            p.put(4, y, 2, Blocks.GLASS_PANE);
        }
        door(p, 2, 1, 0, woodPart(wall, "_door", Blocks.SPRUCE_DOOR));
        for (int x = 0; x < 5; x++)
            for (int z = 0; z < 5; z++) p.put(x, h + 1, z, wall);
        BlockState ladder = Blocks.LADDER.defaultBlockState().setValue(LadderBlock.FACING, Direction.NORTH);
        for (int y = 1; y <= h + 1; y++) p.put(2, y, 3, ladder);
        for (int x = 0; x < 5; x++)
            for (int z = 0; z < 5; z++)
                if ((x == 0 || x == 4 || z == 0 || z == 4) && (x + z) % 2 == 0) p.put(x, h + 2, z, wall);
        p.opt(1, 3, 2, wallTorch(Direction.EAST));
        return p;
    }

    /** A straight wall with battlements, across the Doctor's view. */
    static Plan wall(String name, int len, int h, Block mat) {
        len = clamp(len, 3, 48);
        h = clamp(h, 2, 8);
        Plan p = new Plan(name);
        p.width = len;
        for (int x = 0; x < len; x++) {
            for (int y = 1; y <= h + 1; y++) p.air(x, y, 0);
            for (int y = 0; y <= h; y++) p.put(x, y, 0, mat);
            if (x % 2 == 0) p.put(x, h + 1, 0, mat);
        }
        return p;
    }

    /** A bridge going away from the Doctor, three wide, with railings. */
    static Plan bridge(String name, int len, Block deck) {
        len = clamp(len, 3, 64);
        Plan p = new Plan(name);
        p.width = 3;
        p.foundation = false;
        Block fence = woodPart(deck, "_fence", Blocks.OAK_FENCE);
        for (int z = 0; z < len; z++) {
            for (int x = 0; x < 3; x++) {
                for (int y = 1; y <= 3; y++) p.air(x, y, z);
                p.put(x, 0, z, deck);
            }
            if (z > 0 && z < len - 1) {
                p.put(0, 1, z, fence);
                p.put(2, 1, z, fence);
            }
        }
        return p;
    }

    /** A pen for animals: a fence around and a gate at the front. */
    static Plan corral(String name, int w, int l, Block material) {
        w = clamp(w, 4, 32);
        l = clamp(l, 4, 32);
        Plan p = new Plan(name);
        p.width = w;
        p.foundation = false;
        Block fence = woodPart(material, "_fence", Blocks.OAK_FENCE);
        Block gate = woodPart(material, "_fence_gate", Blocks.OAK_FENCE_GATE);
        for (int x = 0; x < w; x++)
            for (int z = 0; z < l; z++)
                if (x == 0 || x == w - 1 || z == 0 || z == l - 1) {
                    p.air(x, 2, z);
                    p.put(x, 1, z, fence);
                }
        BlockState g = gate.defaultBlockState();
        if (g.hasProperty(HorizontalDirectionalBlock.FACING)) g = g.setValue(HorizontalDirectionalBlock.FACING, Direction.SOUTH);
        p.put(w / 2, 1, 0, g);
        return p;
    }

    static Plan platform(String name, int w, int l, Block mat) {
        w = clamp(w, 2, 32);
        l = clamp(l, 2, 32);
        Plan p = new Plan(name);
        p.width = w;
        for (int x = 0; x < w; x++)
            for (int z = 0; z < l; z++) {
                for (int y = 1; y <= 3; y++) p.air(x, y, z);
                p.put(x, 0, z, mat);
            }
        return p;
    }

    // ── plans the model writes ──

    private static BlockState parseState(String s, HolderLookup<Block> lookup) throws Exception {
        String t = s.trim();
        int br = t.indexOf('[');
        String base = br >= 0 ? t.substring(0, br) : t;
        String props = br >= 0 ? t.substring(br) : "";
        return BlockStateParser.parseForBlock(lookup, blockId(base) + props, false).blockState();
    }

    /**
     * {"palette": {"P": "oak_planks", "D": "oak_door[half=lower]"}, "layers": [["PPP", "P.P"], …]}:
     * one layer per height from the ground layer up, its first row the front, '.' air to clear
     * and ' ' or '_' left as it is. Also Mindcraft's {"levels": [{"placement": [["stone", "air"]]}]}.
     */
    static Plan fromBlueprint(String name, JsonObject o, HolderLookup<Block> lookup) throws Exception {
        Plan p = new Plan(name.isBlank() ? "plano" : name);
        if (o.has("foundation")) p.foundation = o.get("foundation").getAsBoolean();
        if (o.has("layers")) {
            Map<Character, BlockState> palette = new HashMap<>();
            JsonObject pal = o.getAsJsonObject("palette");
            if (pal != null) {
                for (Map.Entry<String, JsonElement> e : pal.entrySet()) {
                    if (!e.getKey().isEmpty()) palette.put(e.getKey().charAt(0), parseState(e.getValue().getAsString(), lookup));
                }
            }
            JsonArray layers = o.getAsJsonArray("layers");
            for (int y = 0; y < layers.size(); y++) {
                JsonArray rows = layers.get(y).getAsJsonArray();
                for (int z = 0; z < rows.size(); z++) {
                    String row = rows.get(z).getAsString();
                    p.width = Math.max(p.width, row.length());
                    for (int x = 0; x < row.length(); x++) {
                        char c = row.charAt(x);
                        if (c == ' ' || c == '_') continue;
                        if (c == '.') {
                            p.air(x, y, z);
                            continue;
                        }
                        BlockState s = palette.get(c);
                        if (s == null) throw new IllegalArgumentException("el símbolo «" + c + "» no está en la paleta");
                        p.put(x, y, z, s);
                    }
                }
            }
        } else if (o.has("levels")) {
            JsonArray levels = o.getAsJsonArray("levels");
            for (int y = 0; y < levels.size(); y++) {
                JsonArray rows = levels.get(y).getAsJsonObject().getAsJsonArray("placement");
                for (int z = 0; z < rows.size(); z++) {
                    JsonArray row = rows.get(z).getAsJsonArray();
                    p.width = Math.max(p.width, row.size());
                    for (int x = 0; x < row.size(); x++) {
                        String n = row.get(x).getAsString();
                        if (n.isBlank()) continue;
                        if (n.equals("air")) p.air(x, y, z);
                        else p.put(x, y, z, parseState(n, lookup));
                    }
                }
            }
        }
        if (p.solid() == 0) throw new IllegalArgumentException("el plano no tiene bloques");
        if (p.cells.size() > 30000) throw new IllegalArgumentException("el plano es demasiado grande");
        return p;
    }

    // ── structures saved with a Structure Block ──

    /** config/kaltsit/structures: .nbt files to share between worlds. */
    public static Path sharedDir() {
        Path d = FabricLoader.getInstance().getConfigDir().resolve("kaltsit").resolve("structures");
        try {
            Files.createDirectories(d);
        } catch (Exception ignored) {}
        return d;
    }

    static String norm(String s) {
        String t = Normalizer.normalize(s.toLowerCase(Locale.ROOT), Normalizer.Form.NFD).replaceAll("\\p{M}", "");
        return t.replaceAll("[^a-z0-9]+", "_").replaceAll("^_+|_+$", "");
    }

    /** Every saved structure she can see: this world's (Structure Block) and the shared ones. */
    public static List<Path> structureFiles(MinecraftServer server) {
        List<Path> out = new ArrayList<>();
        List<Path> roots = new ArrayList<>(List.of(sharedDir()));
        if (server != null) roots.add(server.getWorldPath(LevelResource.GENERATED_DIR));
        for (Path root : roots) {
            if (!Files.isDirectory(root)) continue;
            try (Stream<Path> s = Files.walk(root, 4)) {
                s.filter(f -> f.toString().endsWith(".nbt") && Files.isRegularFile(f)).forEach(out::add);
            } catch (Exception ignored) {}
        }
        return out;
    }

    static String stem(Path f) {
        String n = f.getFileName().toString();
        return n.substring(0, n.length() - 4);
    }

    public static List<String> structureNames(MinecraftServer server) {
        return structureFiles(server).stream().map(KaltsitBuilder::stem).distinct().sorted().toList();
    }

    /** The saved structure a request names ("construye mi_casa", "haz la torre del puerto"). */
    static Path findStructure(MinecraftServer server, String query) {
        String q = "_" + norm(query) + "_";
        Path best = null;
        int bestLen = 0;
        for (Path f : structureFiles(server)) {
            String s = norm(stem(f));
            if (s.length() < 3) continue;
            if ((q.contains("_" + s + "_") || q.equals("_" + s + "_")) && s.length() > bestLen) {
                best = f;
                bestLen = s.length();
            }
        }
        return best;
    }

    static Plan fromStructure(Path file, HolderLookup<Block> lookup) throws Exception {
        CompoundTag t = NbtIo.readCompressed(file, NbtAccounter.unlimitedHeap());
        ListTag palette = t.getListOrEmpty("palette");
        if (palette.size() == 0) {
            ListTag palettes = t.getListOrEmpty("palettes");
            if (palettes.size() > 0 && palettes.get(0) instanceof ListTag first) palette = first;
        }
        BlockState[] states = new BlockState[palette.size()];
        for (int i = 0; i < states.length; i++) states[i] = NbtUtils.readBlockState(lookup, palette.getCompoundOrEmpty(i));
        Plan p = new Plan(stem(file));
        p.width = Math.max(1, t.getListOrEmpty("size").getIntOr(0, 1));
        ListTag blocks = t.getListOrEmpty("blocks");
        for (int i = 0; i < blocks.size(); i++) {
            CompoundTag b = blocks.getCompoundOrEmpty(i);
            ListTag pos = b.getListOrEmpty("pos");
            int si = b.getIntOr("state", -1);
            if (si < 0 || si >= states.length || pos.size() < 3) continue;
            BlockState s = states[si];
            if (s.is(Blocks.STRUCTURE_VOID) || s.is(Blocks.JIGSAW) || s.is(Blocks.STRUCTURE_BLOCK)) continue;
            if (s.isAir()) p.air(pos.getIntOr(0, 0), pos.getIntOr(1, 0), pos.getIntOr(2, 0));
            else p.put(pos.getIntOr(0, 0), pos.getIntOr(1, 0), pos.getIntOr(2, 0), s);
        }
        if (p.solid() == 0) throw new IllegalArgumentException(file.getFileName() + " no tiene bloques");
        if (p.cells.size() > 30000) throw new IllegalArgumentException(file.getFileName() + " es demasiado grande (máx. 30 000 bloques)");
        return p;
    }

    // ── from an order to a plan ──

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

    /** A material named in free text ("de ladrillo", "de abeto", "piedra"), or null. */
    private static Block materialIn(String text, Block fallback) {
        String t = norm(text);
        String[][] words = {
            {"ladrillo", "bricks"}, {"brick", "bricks"}, {"piedra_lisa", "smooth_stone"}, {"ladrillos_de_piedra", "stone_bricks"},
            {"piedra", "stone_bricks"}, {"adoquin", "cobblestone"}, {"cobble", "cobblestone"}, {"abeto", "spruce_planks"},
            {"abedul", "birch_planks"}, {"jungla", "jungle_planks"}, {"acacia", "acacia_planks"}, {"roble_oscuro", "dark_oak_planks"},
            {"cerezo", "cherry_planks"}, {"mangle", "mangrove_planks"}, {"bambu", "bamboo_planks"}, {"roble", "oak_planks"},
            {"madera", "oak_planks"}, {"arenisca", "sandstone"}, {"cuarzo", "quartz_block"}, {"pizarra", "deepslate_bricks"},
            {"deepslate", "deepslate_bricks"}, {"tierra", "dirt"}, {"barro", "mud_bricks"}, {"terracota", "terracotta"},
        };
        for (String[] w : words) {
            if (("_" + t + "_").contains("_" + w[0] + "_") || t.contains(w[0])) {
                Block b = byId(w[1]);
                if (b != null) return b;
            }
        }
        return fallback;
    }

    /** Builds the plan an order asks for: a blueprint, a saved structure or a template. */
    public static Plan planFor(MinecraftServer server, JsonObject o) throws Exception {
        HolderLookup<Block> lookup = server.registryAccess().lookupOrThrow(Registries.BLOCK);
        String name = strOf(o, "name");
        String template = norm(strOf(o, "template") + " " + strOf(o, "type"));
        JsonObject bp = o.has("plan") && o.get("plan").isJsonObject() ? o.getAsJsonObject("plan") : (o.has("layers") || o.has("levels") ? o : null);
        if (bp != null) return fromBlueprint(name, bp, lookup);

        String saved = strOf(o, "structure");
        Path f = findStructure(server, saved.isBlank() ? name : saved);
        if (f != null) return fromStructure(f, lookup);
        if (!saved.isBlank()) throw new IllegalArgumentException("no encuentro la estructura guardada «" + saved + "»");

        String text = (template + " " + norm(name)).trim();
        int w = intOf(o, "width", 0), l = intOf(o, "length", 0), h = intOf(o, "height", 0);
        Matcher m = Pattern.compile("(\\d+)\\s*[x×por]+\\s*(\\d+)").matcher(name.toLowerCase(Locale.ROOT));
        if (w <= 0 && m.find()) {
            w = Integer.parseInt(m.group(1));
            l = Integer.parseInt(m.group(2));
        }
        Matcher n = Pattern.compile("(\\d+)").matcher(name);
        int firstNumber = n.find() ? Integer.parseInt(n.group(1)) : 0;
        String label = name.isBlank() ? template : name;

        if (text.contains("torre") || text.contains("tower") || text.contains("atalaya")) {
            Block wall = block(strOf(o, "wall"), materialIn(text, Blocks.COBBLESTONE));
            return tower(label, h > 0 ? h : (firstNumber > 0 ? firstNumber : 10), wall);
        }
        if (text.contains("muro") || text.contains("muralla") || text.contains("pared") || text.contains("wall")) {
            Block mat = block(strOf(o, "wall"), materialIn(text, Blocks.COBBLESTONE));
            return wall(label, w > 0 ? w : (firstNumber > 0 ? firstNumber : 9), h > 0 ? h : 3, mat);
        }
        if (text.contains("puente") || text.contains("bridge")) {
            Block deck = block(strOf(o, "floor"), materialIn(text, Blocks.OAK_PLANKS));
            return bridge(label, l > 0 ? l : (firstNumber > 0 ? firstNumber : 10), deck);
        }
        if (text.contains("corral") || text.contains("cerca") || text.contains("valla") || text.contains("redil") || text.contains("establo") || text.contains("pen")) {
            Block mat = block(strOf(o, "wall"), materialIn(text, Blocks.OAK_PLANKS));
            return corral(label, w > 0 ? w : 9, l > 0 ? l : (w > 0 ? w : 9), mat);
        }
        if (text.contains("plataforma") || text.contains("suelo") || text.contains("piso") || text.contains("platform")) {
            Block mat = block(strOf(o, "floor"), materialIn(text, Blocks.COBBLESTONE));
            return platform(label, w > 0 ? w : 7, l > 0 ? l : (w > 0 ? w : 7), mat);
        }
        boolean shelter = text.contains("refugio") || text.contains("choza") || text.contains("shelter") || text.contains("emergencia");
        if (shelter) {
            Block mat = materialIn(text, Blocks.COBBLESTONE);
            return house(label, 5, 5, 3, mat, mat, mat, mat, false, false);
        }
        // A house by default.
        Block wall = block(strOf(o, "wall"), materialIn(text, Blocks.OAK_PLANKS));
        Block floor = block(strOf(o, "floor"), Blocks.COBBLESTONE);
        Block roof = block(strOf(o, "roof"), wood(wall) != null ? wall : Blocks.SPRUCE_PLANKS);
        String woodName = wood(wall) != null ? wood(wall) : (wood(roof) != null ? wood(roof) : "oak");
        Block pillar = block(strOf(o, "pillar"), byId(woodName + "_log") != null ? byId(woodName + "_log") : Blocks.OAK_LOG);
        boolean flat = strOf(o, "roof_type").equalsIgnoreCase("flat") || text.contains("plano") || text.contains("flat");
        int width = w > 0 ? w : 7, length = l > 0 ? l : (w > 0 ? w : 7);
        return house(label.isBlank() ? "casa" : label, width, length, h > 0 ? h : 4, wall, floor, roof, pillar, !flat, true);
    }

    // ── the work ──

    enum Phase { CLEAR, BUILD, CLEANUP, CHECK }

    public static class Job {
        final Plan plan;
        final ServerLevel level;
        final Map<BlockPos, BlockState> target = new LinkedHashMap<>();
        final Set<BlockPos> optional = new HashSet<>();
        final Set<BlockPos> skipped = new HashSet<>();
        final Deque<BlockPos> scaffolds = new ArrayDeque<>();
        final Map<Item, Integer> takenFromDoctor = new LinkedHashMap<>();
        final BlockPos center;
        final Set<BlockPos> foundationCells = new HashSet<>();
        boolean creative = false;
        int toClear = 0;
        String problem = null;
        Phase phase = Phase.CLEAR;
        boolean paused = false;
        boolean waiting = false;
        boolean checked = false;
        int placed = 0, broken = 0, cooldown = 0, waitTicks = 0;
        BlockPos current = null, stand = null, pillarFeet = null;
        int currentTicks = 0, progress = 0, pillarTries = 0;

        Job(Plan plan, ServerLevel level, BlockPos center) {
            this.plan = plan;
            this.level = level;
            this.center = center;
        }

        int remaining() {
            int n = 0;
            for (Map.Entry<BlockPos, BlockState> e : target.entrySet()) {
                if (!e.getValue().isAir() && !skipped.contains(e.getKey()) && !done(level, e.getKey(), e.getValue())) n++;
            }
            return n;
        }
    }

    private static Job job = null;

    public static Job current() {
        return job;
    }

    public static void reset() {
        job = null;
    }

    private static Rotation facing(Direction front) {
        for (Rotation r : Rotation.values()) {
            if (r.rotate(Direction.NORTH) == front) return r;
        }
        return Rotation.NONE;
    }

    static boolean done(Level level, BlockPos pos, BlockState want) {
        BlockState have = level.getBlockState(pos);
        if (want.isAir()) return have.isAir() || !have.getFluidState().isEmpty();
        return have.getBlock() == want.getBlock();
    }

    /** What a player spends to place it: none for the other half of a door or bed. */
    static Item itemFor(BlockState s) {
        if (s.isAir()) return null;
        if (s.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF) && s.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == DoubleBlockHalf.UPPER) return null;
        if (s.getBlock() instanceof BedBlock && s.getValue(BedBlock.PART) == BedPart.HEAD) return null;
        if (s.is(Blocks.FARMLAND) || s.is(Blocks.DIRT_PATH)) return Items.DIRT;
        Item it = s.getBlock().asItem();
        return it == Items.AIR ? null : it;
    }

    private static boolean isPartnerHalf(BlockState s) {
        return (s.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF) && s.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == DoubleBlockHalf.UPPER)
            || (s.getBlock() instanceof BedBlock && s.getValue(BedBlock.PART) == BedPart.HEAD);
    }

    private static boolean isTorch(BlockState s) {
        return s.is(Blocks.TORCH) || s.is(Blocks.WALL_TORCH) || s.is(Blocks.SOUL_TORCH) || s.is(Blocks.SOUL_WALL_TORCH);
    }

    // ── materials ──

    private static List<Container> chestsNear(ServerLevel level, BlockPos c, int r) {
        List<Container> out = new ArrayList<>();
        for (int dx = -r; dx <= r; dx++)
            for (int dy = -4; dy <= 4; dy++)
                for (int dz = -r; dz <= r; dz++) {
                    BlockEntity be = level.getBlockEntity(c.offset(dx, dy, dz));
                    if (be instanceof Container cont) out.add(cont);
                }
        return out;
    }

    private static int count(Container c, Item item) {
        int n = 0;
        for (int i = 0; i < c.getContainerSize(); i++) {
            ItemStack s = c.getItem(i);
            if (s.is(item)) n += s.getCount();
        }
        return n;
    }

    private static int countDoctor(ServerPlayer doctor, Item item) {
        if (doctor == null) return 0;
        int n = 0;
        for (int i = 0; i < doctor.getInventory().getContainerSize(); i++) {
            ItemStack s = doctor.getInventory().getItem(i);
            if (s.is(item)) n += s.getCount();
        }
        return n;
    }

    /** Moves up to `want` of an item from `from` into her inventory; how many moved. */
    private static int move(Container from, Item item, int want) {
        int moved = 0;
        for (int i = 0; i < from.getContainerSize() && moved < want; i++) {
            ItemStack s = from.getItem(i);
            if (!s.is(item)) continue;
            int take = Math.min(s.getCount(), want - moved);
            ItemStack part = s.copyWithCount(take);
            ItemStack rest = KaltsitWorldInteractor.companionInventory.addItem(part);
            int got = take - rest.getCount();
            s.shrink(got);
            moved += got;
            if (got < take) break; // her inventory is full
        }
        if (moved > 0) from.setChanged();
        return moved;
    }

    private static Map<Item, Integer> needed(Job j) {
        Map<Item, Integer> need = new LinkedHashMap<>();
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            if (j.skipped.contains(e.getKey()) || done(j.level, e.getKey(), e.getValue())) continue;
            Item it = itemFor(e.getValue());
            if (it != null) need.merge(it, 1, Integer::sum);
        }
        return need;
    }

    private static Map<Item, Integer> missing(Job j, ServerPlayer doctor, Map<Item, Integer> need) {
        Map<Item, Integer> miss = new LinkedHashMap<>();
        if (j.creative) return miss;
        List<Container> chests = chestsNear(j.level, j.center, 10);
        for (Map.Entry<Item, Integer> e : need.entrySet()) {
            int have = count(KaltsitWorldInteractor.companionInventory, e.getKey()) + countDoctor(doctor, e.getKey());
            for (Container c : chests) have += count(c, e.getKey());
            if (have < e.getValue()) miss.put(e.getKey(), e.getValue() - have);
        }
        return miss;
    }

    private static MutableComponent list(Map<Item, Integer> items) {
        MutableComponent c = Component.empty();
        int i = 0;
        for (Map.Entry<Item, Integer> e : items.entrySet()) {
            if (i > 0) c.append(", ");
            if (i == 10) {
                c.append("y " + (items.size() - 10) + " más");
                break;
            }
            c.append(e.getValue() + "× ").append(new ItemStack(e.getKey()).getHoverName());
            i++;
        }
        return c;
    }

    /**
     * Makes sure she holds one of `item`: from her inventory, else what the rest of the build
     * needs from chests near the site and then from the Doctor's inventory.
     */
    private static boolean obtain(Job j, ServerPlayer doctor, Item item) {
        if (j.creative) return true;
        if (count(KaltsitWorldInteractor.companionInventory, item) > 0) return true;
        int want = Math.max(1, needed(j).getOrDefault(item, 1));
        int got = 0;
        for (Container c : chestsNear(j.level, j.center, 10)) {
            got += move(c, item, want - got);
            if (got >= want) break;
        }
        if (got < want && doctor != null) {
            int fromDoctor = move(doctor.getInventory(), item, want - got);
            if (fromDoctor > 0) {
                j.takenFromDoctor.merge(item, fromDoctor, Integer::sum);
                doctor.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Tomé de tu inventario: ").append(list(Map.of(item, fromDoctor))).append("."));
                got += fromDoctor;
            }
        }
        return got > 0;
    }

    private static void spend(Job j, Item item) {
        if (item == null || j.creative) return;
        var inv = KaltsitWorldInteractor.companionInventory;
        for (int i = 0; i < inv.getContainerSize(); i++) {
            if (inv.getItem(i).is(item)) {
                inv.getItem(i).shrink(1);
                inv.setChanged();
                return;
            }
        }
    }

    // ── starting, pausing, cancelling ──

    /** Where a plan goes: the cell under the front centre (ground layer) and its turn. */
    public record Site(ServerLevel level, BlockPos anchor, Rotation rot) {
        /** In front of the Doctor, three blocks ahead, its front facing him. */
        static Site before(ServerPlayer doctor) {
            Direction d = doctor.getDirection();
            return new Site((ServerLevel) doctor.level(), doctor.blockPosition().relative(d, 3).below(), facing(d.getOpposite()));
        }

        BlockPos origin(Plan plan) {
            return anchor.subtract(new BlockPos(plan.width / 2, 0, 0).rotate(rot));
        }
    }

    /**
     * Lays a plan out on a site and works out what it takes, without touching the world: the
     * same layout for the preview and for the build. A problem (chests of the Doctor's or
     * unbreakable blocks in the way) is in `problem`.
     */
    public static Job prepare(ServerPlayer doctor, Plan plan, Site site) {
        ServerLevel level = site.level();
        BlockPos origin = site.origin(plan);
        Rotation rot = site.rot();
        Job j = new Job(plan, level, origin.offset(new BlockPos(plan.width / 2, 0, plan.depth() / 2).rotate(rot)));
        j.creative = doctor != null && doctor.isCreative();
        for (Map.Entry<BlockPos, BlockState> e : plan.cells.entrySet()) {
            BlockPos wp = origin.offset(e.getKey().rotate(rot));
            BlockState s = e.getValue().rotate(rot);
            if (isTorch(s) && !KaltsitSettings.torchAllowed(level, wp)) continue;
            if (!s.isAir() && itemFor(s) == null && !isPartnerHalf(s) && !j.creative) continue; // water, fire…: not by hand
            j.target.put(wp, s);
            if (plan.optional.contains(e.getKey())) j.optional.add(wp);
        }
        // Nothing of the Doctor's (chests, furnaces, beds…) and nothing unbreakable in the way.
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            BlockState have = level.getBlockState(e.getKey());
            if (done(level, e.getKey(), e.getValue())) continue;
            if (!e.getValue().isAir() && have.canBeReplaced()) continue;
            if (have.isAir() || !have.getFluidState().isEmpty()) continue;
            j.toClear++;
            if (j.problem == null && have.hasBlockEntity()) {
                j.problem = "hay cosas tuyas (" + have.getBlock().getName().getString() + ") donde iría";
            } else if (j.problem == null && have.getDestroySpeed(level, e.getKey()) < 0) {
                j.problem = "hay bloques irrompibles donde iría";
            }
        }
        // A foundation where the ground is missing under the ground layer.
        if (plan.foundation) {
            Block fill = Blocks.COBBLESTONE;
            int best = -1;
            for (Block b : new Block[] {Blocks.COBBLESTONE, Blocks.DIRT, Blocks.COBBLED_DEEPSLATE, Blocks.STONE}) {
                int n = count(KaltsitWorldInteractor.companionInventory, b.asItem()) + countDoctor(doctor, b.asItem());
                if (n > best) {
                    best = n;
                    fill = b;
                }
            }
            for (Map.Entry<BlockPos, BlockState> e : plan.cells.entrySet()) {
                if (e.getKey().getY() != 0 || e.getValue().isAir()) continue;
                BlockPos b = origin.offset(e.getKey().rotate(rot));
                for (int k = 1; k <= 6; k++) {
                    BlockPos p = b.below(k);
                    BlockState have = level.getBlockState(p);
                    if (!(have.isAir() || have.canBeReplaced()) || j.target.containsKey(p)) break;
                    j.target.put(p, fill.defaultBlockState());
                    j.foundationCells.add(p);
                }
            }
        }
        return j;
    }

    /** What the job needs that is not standing yet. */
    public static Map<Item, Integer> needs(Job j) {
        return needed(j);
    }

    /** What of it cannot be found (her inventory, chests near the site, the Doctor's). */
    public static Map<Item, Integer> lacks(Job j, ServerPlayer doctor) {
        return missing(j, doctor, needed(j));
    }

    /** Starts a prepared job (the preview was accepted). Null when it started. */
    public static Component begin(ServerPlayer doctor, Job j) {
        if (j.problem != null) {
            return Component.literal("§c[Kal'tsit]§r No empiezo «" + j.plan.name + "»: " + j.problem + ". Muévela o elige otro sitio.");
        }
        job = j;
        Map<Item, Integer> need = needed(j);
        Map<Item, Integer> miss = missing(j, doctor, need);
        long blocks = need.values().stream().mapToInt(Integer::intValue).sum();
        MutableComponent msg = Component.literal("§b[Kal'tsit]§r Empiezo «" + j.plan.name + "»: " + blocks + " bloques");
        if (!j.creative && !need.isEmpty()) msg.append(". Necesito ").append(list(need));
        msg.append(".");
        if (!miss.isEmpty()) msg.append(Component.literal(" §eMe faltan§r ").append(list(miss)).append(": empiezo con lo que hay y te aviso cuando me pare."));
        if (doctor != null) doctor.sendSystemMessage(msg);
        return null;
    }

    /** Lays the plan out in front of the Doctor and starts at once (no preview). */
    public static Component start(ServerPlayer doctor, Plan plan) {
        return begin(doctor, prepare(doctor, plan, Site.before(doctor)));
    }

    public static Component pause() {
        if (job == null) return Component.literal("§7[Kal'tsit]§r No estoy construyendo nada.");
        job.paused = true;
        return Component.literal("§b[Kal'tsit]§r Obra «" + job.plan.name + "» en pausa (" + job.remaining() + " bloques por poner). Dime que siga cuando quieras.");
    }

    public static Component resume() {
        if (job == null) return Component.literal("§7[Kal'tsit]§r No hay ninguna obra a medias.");
        job.paused = false;
        job.waiting = false;
        return Component.literal("§b[Kal'tsit]§r Sigo con «" + job.plan.name + "» (" + job.remaining() + " bloques por poner).");
    }

    public static Component cancel(KaltsitEntity self) {
        if (job == null) return Component.literal("§7[Kal'tsit]§r No estoy construyendo nada.");
        String name = job.plan.name;
        int left = job.remaining();
        if (self != null && job.current != null) job.level.destroyBlockProgress(self.getId(), job.current, -1);
        report(job, "cancelled");
        job = null;
        return Component.literal("§b[Kal'tsit]§r Obra «" + name + "» cancelada; quedaron " + left + " bloques sin poner. Lo construido se queda.");
    }

    /** How the build goes, for the app's «Plano» tab (every few seconds and at the end). */
    public static void reportProgress() {
        if (job != null) report(job, job.paused ? "paused" : job.waiting ? "waiting" : job.phase.name().toLowerCase(Locale.ROOT));
    }

    private static void report(Job j, String phase) {
        if (!KaltsitBridge.isConnected()) return;
        JsonObject o = new JsonObject();
        o.addProperty("type", "build_progress");
        o.addProperty("name", j.plan.name);
        o.addProperty("phase", phase);
        o.addProperty("placed", j.placed);
        o.addProperty("broken", j.broken);
        o.addProperty("remaining", j.remaining());
        o.addProperty("skipped", j.skipped.size());
        if (j.waiting) {
            JsonArray miss = new JsonArray();
            for (Map.Entry<Item, Integer> e : missing(j, KaltsitMod.cachedDoctor(), needed(j)).entrySet()) {
                JsonArray m = new JsonArray();
                m.add(BuiltInRegistries.ITEM.getKey(e.getKey()).getPath());
                m.add(e.getValue());
                miss.add(m);
            }
            o.add("missing", miss);
        }
        KaltsitBridge.send(o.toString());
    }

    public static Component status() {
        if (job == null) return Component.literal("§7[Kal'tsit]§r No estoy construyendo nada.");
        String what = job.paused ? "en pausa" : job.waiting ? "esperando materiales" : switch (job.phase) {
            case CLEAR -> "despejando el terreno";
            case BUILD -> "colocando bloques";
            case CLEANUP -> "quitando el andamio";
            case CHECK -> "revisando";
        };
        return Component.literal("§b[Kal'tsit]§r «" + job.plan.name + "»: " + what + " · " + job.placed + " puestos, " + job.remaining() + " por poner" + (job.skipped.isEmpty() ? "" : ", " + job.skipped.size() + " imposibles") + ".");
    }

    // ── one tick of work ──

    /** True while there is work (she stays in BUILDING); false when the job ended. */
    public static boolean tick(KaltsitEntity self, ServerPlayer doctor) {
        Job j = job;
        if (j == null) return false;
        if (self.level() != j.level) return true; // The Doctor took her to another dimension: the site waits.
        if (j.paused || self.isUsingItem()) {
            self.getNavigation().stop();
            return true;
        }
        if (j.cooldown > 0) {
            j.cooldown--;
            return true;
        }
        if (j.waiting) {
            self.getNavigation().stop();
            if (++j.waitTicks % 40 != 0) return true;
            j.waiting = false; // Look again: the Doctor may have brought something.
        }
        switch (j.phase) {
            case CLEAR -> {
                BlockPos next = nextBreak(j, self);
                if (next == null) {
                    j.phase = Phase.BUILD;
                    return true;
                }
                work(self, j, next, true, doctor);
            }
            case BUILD -> {
                BlockPos next = nextPlace(j, self, doctor);
                if (next == null) {
                    if (!j.waiting) j.phase = Phase.CLEANUP;
                    return true;
                }
                work(self, j, next, false, doctor);
            }
            case CLEANUP -> {
                if (j.scaffolds.isEmpty()) {
                    j.phase = Phase.CHECK;
                    return true;
                }
                BlockPos top = j.scaffolds.peekLast();
                if (j.level.getBlockState(top).isAir()) {
                    j.scaffolds.pollLast();
                    return true;
                }
                work(self, j, top, true, doctor);
            }
            case CHECK -> {
                int left = j.remaining();
                if (left > 0 && !j.checked) {
                    // Something fell, was trampled or broken while she worked: once more.
                    j.checked = true;
                    j.phase = Phase.CLEAR;
                    return true;
                }
                MutableComponent msg = Component.literal("§a[Kal'tsit]§r «" + j.plan.name + "» terminada: " + j.placed + " bloques puestos");
                if (j.broken > 0) msg.append(", " + j.broken + " retirados");
                if (!j.skipped.isEmpty()) msg.append(", §e" + j.skipped.size() + " que no pude poner§r");
                if (!j.takenFromDoctor.isEmpty()) msg.append(". De tu inventario usé ").append(list(j.takenFromDoctor));
                msg.append(".");
                // Also when he is far away (or in another dimension): he hears it in the chat.
                ServerPlayer told = doctor != null ? doctor : KaltsitMod.cachedDoctor();
                if (told != null) told.sendSystemMessage(msg);
                report(j, "done");
                job = null;
                return false;
            }
        }
        return true;
    }

    private static double dist(KaltsitEntity self, BlockPos p) {
        return self.distanceToSqr(p.getX() + 0.5, p.getY() + 0.5, p.getZ() + 0.5);
    }

    private static BlockPos nextBreak(Job j, KaltsitEntity self) {
        BlockPos best = null;
        double bestKey = Double.MAX_VALUE;
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            BlockPos p = e.getKey();
            if (j.skipped.contains(p) || j.scaffolds.contains(p)) continue;
            BlockState have = j.level.getBlockState(p);
            if (have.isAir() || !have.getFluidState().isEmpty()) continue;
            BlockState want = e.getValue();
            boolean wrong = want.isAir() || (have.getBlock() != want.getBlock() && !have.canBeReplaced());
            if (!wrong || have.getDestroySpeed(j.level, p) < 0) continue;
            // Top-down, then the nearest.
            double key = -p.getY() * 10000.0 + dist(self, p);
            if (key < bestKey) {
                bestKey = key;
                best = p;
            }
        }
        return best;
    }

    private static boolean hasSupport(Level level, BlockPos p) {
        for (Direction d : Direction.values()) {
            BlockState n = level.getBlockState(p.relative(d));
            if (!n.isAir() && n.getFluidState().isEmpty()) return true;
        }
        return false;
    }

    private static BlockPos nextPlace(Job j, KaltsitEntity self, ServerPlayer doctor) {
        List<BlockPos> ready = new ArrayList<>();
        boolean anyLeft = false;
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            BlockPos p = e.getKey();
            BlockState want = e.getValue();
            if (want.isAir() || j.skipped.contains(p) || done(j.level, p, want)) continue;
            if (isPartnerHalf(want)) {
                // Placed with its other half; alone only when that half already stands.
                BlockPos other = want.getBlock() instanceof BedBlock ? p.relative(want.getValue(BedBlock.FACING).getOpposite()) : p.below();
                if (!j.level.getBlockState(other).is(want.getBlock())) continue;
            }
            anyLeft = true;
            BlockState have = j.level.getBlockState(p);
            if (!have.isAir() && !have.canBeReplaced()) continue;
            if (!hasSupport(j.level, p) || !want.canSurvive(j.level, p)) continue;
            ready.add(p);
        }
        if (ready.isEmpty()) {
            if (anyLeft) {
                // What is left cannot stand anywhere (nothing to rest on): not by hand.
                for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
                    if (!e.getValue().isAir() && !done(j.level, e.getKey(), e.getValue())) j.skipped.add(e.getKey());
                }
            }
            return null;
        }
        // Bottom-up, then the nearest; the first whose material she has or can get.
        ready.sort(Comparator.comparingInt((BlockPos p) -> p.getY()).thenComparingDouble(p -> dist(self, p)));
        Set<Item> unavailable = new HashSet<>();
        for (BlockPos p : ready) {
            Item it = itemFor(j.target.get(p));
            if (it == null || unavailable.contains(it)) {
                if (it == null) return p;
                continue;
            }
            if (obtain(j, doctor, it)) return p;
            unavailable.add(it);
            if (j.optional.contains(p)) j.skipped.add(p);
        }
        // Nothing she can place with what there is: wait for the Doctor.
        Map<Item, Integer> miss = missing(j, doctor, needed(j));
        if (miss.isEmpty()) return null;
        if (!j.waiting && doctor != null && (j.waitTicks == 0 || j.waitTicks > 1200)) {
            doctor.sendSystemMessage(Component.literal("§e[Kal'tsit]§r Me paro: me faltan ").append(list(miss))
                .append(". Dámelos (clic derecho o tíralos cerca), déjalos en un cofre junto a la obra, o dime que la cancele."));
            j.waitTicks = 1;
        }
        j.waiting = true;
        return null;
    }

    /** Walks, climbs or digs her way to `pos` and breaks or places it. */
    private static void work(KaltsitEntity self, Job j, BlockPos pos, boolean breaking, ServerPlayer doctor) {
        if (!pos.equals(j.current)) {
            if (j.current != null && breaking) j.level.destroyBlockProgress(self.getId(), j.current, -1);
            j.current = pos;
            j.currentTicks = 0;
            j.progress = 0;
            j.stand = null;
        }
        if (++j.currentTicks > 500) {
            // Eight seconds of trying is enough: she leaves it and says so at the end.
            j.skipped.add(pos);
            j.current = null;
            j.level.destroyBlockProgress(self.getId(), pos, -1);
            return;
        }
        if (!KaltsitWorldInteractor.canReach(self, pos)) {
            approach(self, j, pos);
            return;
        }
        self.getNavigation().stop();
        self.getLookControl().setLookAt(pos.getX() + 0.5, pos.getY() + 0.5, pos.getZ() + 0.5);
        if (breaking) {
            if (KaltsitMod.breakTick(self, j.level, pos, j.progress)) {
                j.broken++;
                j.progress = 0;
                j.current = null;
                if (j.scaffolds.contains(pos)) j.scaffolds.remove(pos);
            } else {
                j.progress++;
            }
            return;
        }
        BlockState want = j.target.get(pos);
        // Never inside her own body or the Doctor's.
        if (want.blocksMotion()) {
            AABB box = new AABB(pos);
            if (self.getBoundingBox().intersects(box)) {
                BlockPos away = KaltsitWorldInteractor.findWalkableStandingPos(j.level, pos, self.blockPosition());
                if (away != null) self.getNavigation().moveTo(away.getX() + 0.5, away.getY(), away.getZ() + 0.5, 1.0);
                return;
            }
            if (doctor != null && doctor.getBoundingBox().intersects(box)) return;
        }
        Item it = itemFor(want);
        if (it != null && !obtain(j, doctor, it)) return;
        place(j, pos, want);
        spend(j, it);
        self.swing(InteractionHand.MAIN_HAND);
        j.placed++;
        j.current = null;
        j.cooldown = 3; // A player places about five blocks a second
    }

    private static void place(Job j, BlockPos pos, BlockState want) {
        ServerLevel level = j.level;
        level.setBlock(pos, Block.updateFromNeighbourShapes(want, level, pos), Block.UPDATE_ALL);
        // Doors and beds go up whole.
        BlockPos other = null;
        if (want.hasProperty(BlockStateProperties.DOUBLE_BLOCK_HALF) && want.getValue(BlockStateProperties.DOUBLE_BLOCK_HALF) == DoubleBlockHalf.LOWER) other = pos.above();
        if (want.getBlock() instanceof BedBlock && want.getValue(BedBlock.PART) == BedPart.FOOT) other = pos.relative(want.getValue(BedBlock.FACING));
        if (other != null && j.target.containsKey(other) && j.target.get(other).is(want.getBlock())) {
            level.setBlock(other, j.target.get(other), Block.UPDATE_ALL);
        }
        level.playSound(null, pos, want.getSoundType().getPlaceSound(), SoundSource.BLOCKS, 0.9f, 0.9f);
    }

    /**
     * Gets within reach: walks to a spot from where her arms reach the block, and when there is
     * none (too high), climbs a pillar of her own right below it, like a player would.
     */
    private static void approach(KaltsitEntity self, Job j, BlockPos pos) {
        // Up on her pillar, the block is elsewhere: down first, one block at a time.
        BlockPos feet = self.blockPosition();
        if (!j.scaffolds.isEmpty() && feet.below().equals(j.scaffolds.peekLast()) && Math.abs(pos.getX() - feet.getX()) + Math.abs(pos.getZ() - feet.getZ()) > 2) {
            BlockPos top = j.scaffolds.peekLast();
            if (KaltsitMod.breakTick(self, j.level, top, j.progress)) {
                j.scaffolds.pollLast();
                j.progress = 0;
            } else {
                j.progress++;
            }
            return;
        }
        if (j.stand == null || j.currentTicks % 60 == 0) j.stand = findStand(j, pos, self);
        if (j.stand != null) {
            if (!feet.equals(j.stand)) {
                self.getNavigation().moveTo(j.stand.getX() + 0.5, j.stand.getY(), j.stand.getZ() + 0.5, 1.05);
                return;
            }
        }
        // No spot reaches it: a pillar under herself, if she is roughly below it.
        int dx = Math.abs(pos.getX() - feet.getX()), dz = Math.abs(pos.getZ() - feet.getZ());
        if (pos.getY() > feet.getY() + 2 && dx <= 2 && dz <= 2) {
            pillar(self, j);
            return;
        }
        BlockPos base = findPillarBase(j, pos, self);
        if (base != null && !feet.equals(base)) {
            self.getNavigation().moveTo(base.getX() + 0.5, base.getY(), base.getZ() + 0.5, 1.05);
        } else if (base == null) {
            self.getNavigation().moveTo(pos.getX() + 0.5, pos.getY(), pos.getZ() + 0.5, 1.05);
        }
    }

    /** A free spot (not one where a block still has to go) from where her eyes reach `pos`. */
    private static BlockPos findStand(Job j, BlockPos pos, KaltsitEntity self) {
        BlockPos best = null;
        double bestD = Double.MAX_VALUE;
        Vec3 c = Vec3.atCenterOf(pos);
        for (int dx = -3; dx <= 3; dx++)
            for (int dz = -3; dz <= 3; dz++)
                for (int dy = -5; dy <= 1; dy++) {
                    BlockPos p = pos.offset(dx, dy, dz);
                    if (p.equals(pos) || !KaltsitWorldInteractor.isWalkable(j.level, p)) continue;
                    if (pending(j, p) || pending(j, p.above())) continue;
                    if (new Vec3(p.getX() + 0.5, p.getY() + 1.62, p.getZ() + 0.5).distanceToSqr(c) > 4.3 * 4.3) continue;
                    double d = self.distanceToSqr(p.getX() + 0.5, p.getY(), p.getZ() + 0.5);
                    if (d < bestD) {
                        bestD = d;
                        best = p;
                    }
                }
        return best;
    }

    /** Ground right below (or next to) a high block, with room above to climb. */
    private static BlockPos findPillarBase(Job j, BlockPos pos, KaltsitEntity self) {
        BlockPos best = null;
        double bestD = Double.MAX_VALUE;
        for (int dx = -2; dx <= 2; dx++)
            for (int dz = -2; dz <= 2; dz++)
                for (int dy = -12; dy <= -2; dy++) {
                    BlockPos p = pos.offset(dx, dy, dz);
                    if (!KaltsitWorldInteractor.isWalkable(j.level, p) || pending(j, p)) continue;
                    double d = Math.abs(dx) + Math.abs(dz) + self.distanceToSqr(p.getX() + 0.5, p.getY(), p.getZ() + 0.5) * 0.01;
                    if (d < bestD) {
                        bestD = d;
                        best = p;
                    }
                }
        return best;
    }

    private static boolean pending(Job j, BlockPos p) {
        BlockState want = j.target.get(p);
        return want != null && want.blocksMotion() && !done(j.level, p, want) && !j.skipped.contains(p);
    }

    /** One step of climbing: jump, and put a block under her feet at the top of the jump. */
    private static void pillar(KaltsitEntity self, Job j) {
        self.getNavigation().stop();
        if (j.scaffolds.size() >= 12) {
            j.skipped.add(j.current);
            j.current = null;
            return;
        }
        if (self.onGround()) {
            j.pillarFeet = self.blockPosition();
            if (!j.level.getBlockState(j.pillarFeet.above(2)).isAir()) {
                j.skipped.add(j.current); // A ceiling over her head
                j.current = null;
                return;
            }
            self.getJumpControl().jump();
            j.pillarTries++;
            return;
        }
        if (j.pillarFeet != null && self.getY() >= j.pillarFeet.getY() + 1.0 && j.level.getBlockState(j.pillarFeet).isAir()) {
            Item fill = scaffoldItem();
            if (fill == null) {
                j.skipped.add(j.current);
                j.current = null;
                ServerPlayer doc = KaltsitMod.cachedDoctor();
                if (doc != null) doc.sendSystemMessage(Component.literal("§e[Kal'tsit]§r No llego tan alto y no tengo tierra ni piedra para subirme."), true);
                return;
            }
            BlockState s = Block.byItem(fill).defaultBlockState();
            j.level.setBlock(j.pillarFeet, s, Block.UPDATE_ALL);
            j.level.playSound(null, j.pillarFeet, s.getSoundType().getPlaceSound(), SoundSource.BLOCKS, 0.8f, 1.0f);
            spend(j, fill);
            self.swing(InteractionHand.MAIN_HAND);
            j.scaffolds.addLast(j.pillarFeet);
            j.pillarFeet = null;
        }
    }

    /** What she climbs on: dirt, cobblestone… (never the materials of the build itself). */
    private static Item scaffoldItem() {
        for (Item it : new Item[] {Items.DIRT, Items.COBBLESTONE, Items.COBBLED_DEEPSLATE, Items.NETHERRACK, Items.SCAFFOLDING}) {
            if ((job != null && job.creative) || count(KaltsitWorldInteractor.companionInventory, it) > 0) return it;
        }
        return null;
    }
}
