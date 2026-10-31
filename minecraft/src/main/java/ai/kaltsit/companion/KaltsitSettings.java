package ai.kaltsit.companion;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import net.minecraft.core.BlockPos;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.storage.LevelResource;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Set;

/**
 * What the Doctor told her about this world and that must outlive a restart: whether she lights
 * dark places on her own, and the zones where she must never put a torch. Kept next to the
 * world's level.dat as kaltsit.json.
 */
public class KaltsitSettings {
    public static class Zone {
        public String dim;
        public int x, y, z, r;

        Zone(String dim, BlockPos c, int r) {
            this.dim = dim;
            this.x = c.getX();
            this.y = c.getY();
            this.z = c.getZ();
            this.r = r;
        }

        boolean contains(String d, BlockPos p) {
            if (!dim.equals(d)) return false;
            long dx = p.getX() - x, dy = p.getY() - y, dz = p.getZ() - z;
            // A cylinder: a zone covers the cave below and the tower above too, within reason.
            return dx * dx + dz * dz <= (long) r * r && Math.abs(dy) <= Math.max(16, r);
        }
    }

    public boolean autoTorches = true;
    public List<Zone> noTorchZones = new ArrayList<>();
    /** FOLLOWING, IDLE, GUARDING…: where she stands is kept when the world closes. */
    public String companionState = "FOLLOWING";
    /** The chunks she keeps loaded while she stays somewhere on her own. */
    public String forcedDim = null;
    public List<Long> forcedChunks = new ArrayList<>();
    /** Her own bed: where she respawns in survival. */
    public String bedDim = null;
    public int[] bedPos = null;

    public record BedSpot(ServerLevel level, BlockPos pos) {}

    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private static KaltsitSettings current = new KaltsitSettings();
    private static Path file = null;

    public static KaltsitSettings get() {
        return current;
    }

    public static void load(MinecraftServer server) {
        file = server.getWorldPath(LevelResource.ROOT).resolve("kaltsit.json");
        current = new KaltsitSettings();
        try {
            if (Files.isRegularFile(file)) {
                KaltsitSettings s = GSON.fromJson(Files.readString(file, StandardCharsets.UTF_8), KaltsitSettings.class);
                if (s != null) {
                    if (s.noTorchZones == null) s.noTorchZones = new ArrayList<>();
                    current = s;
                }
            }
        } catch (Exception e) {
            System.out.println("[Kaltsit] kaltsit.json ilegible, uso valores por defecto: " + e);
        }
    }

    public static void save() {
        if (file == null) return;
        try {
            Files.writeString(file, GSON.toJson(current), StandardCharsets.UTF_8);
        } catch (Exception e) {
            System.out.println("[Kaltsit] No se pudo guardar kaltsit.json: " + e);
        }
    }

    public static String dimOf(Level level) {
        return level.dimension().identifier().toString();
    }

    /** Whether a torch may go at `pos` (outside every no-torch zone). */
    public static boolean torchAllowed(Level level, BlockPos pos) {
        String d = dimOf(level);
        for (Zone z : current.noTorchZones) {
            if (z.contains(d, pos)) return false;
        }
        return true;
    }

    public static void addNoTorchZone(Level level, BlockPos center, int radius) {
        current.noTorchZones.add(new Zone(dimOf(level), center, Math.max(4, Math.min(128, radius))));
        save();
    }

    /** Removes the zones covering `pos`; how many there were. */
    public static int removeZonesAt(Level level, BlockPos pos) {
        String d = dimOf(level);
        int before = current.noTorchZones.size();
        current.noTorchZones.removeIf(z -> z.contains(d, pos));
        save();
        return before - current.noTorchZones.size();
    }

    public static void setAutoTorches(boolean on) {
        current.autoTorches = on;
        save();
    }

    public static void setCompanionState(String state) {
        if (state.equals(current.companionState)) return;
        current.companionState = state;
        save();
    }

    private static ServerLevel levelOf(MinecraftServer server, String dim) {
        if (dim == null) return null;
        for (ServerLevel l : server.getAllLevels()) {
            if (dimOf(l).equals(dim)) return l;
        }
        return null;
    }

    public static ServerLevel forcedLevel(MinecraftServer server) {
        return levelOf(server, current.forcedDim);
    }

    public static void setForced(ServerLevel level, Set<Long> chunks) {
        current.forcedDim = level == null ? null : dimOf(level);
        current.forcedChunks = new ArrayList<>(chunks);
        save();
    }

    /** Her bed, if it still stands. */
    public static BedSpot bed(MinecraftServer server) {
        if (current.bedPos == null || current.bedPos.length != 3) return null;
        ServerLevel level = levelOf(server, current.bedDim);
        if (level == null) return null;
        BlockPos p = new BlockPos(current.bedPos[0], current.bedPos[1], current.bedPos[2]);
        return level.getBlockState(p).getBlock() instanceof BedBlock ? new BedSpot(level, p) : null;
    }

    public static void setBed(Level level, BlockPos pos) {
        current.bedDim = dimOf(level);
        current.bedPos = new int[] {pos.getX(), pos.getY(), pos.getZ()};
        save();
    }
}
