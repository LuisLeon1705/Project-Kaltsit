package ai.kaltsit.companion;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.mojang.math.Transformation;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.MutableComponent;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.util.Brightness;
import net.minecraft.world.entity.Display;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.item.Item;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.block.state.BlockState;
import org.joml.Quaternionf;
import org.joml.Vector3f;

import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.UUID;

/**
 * The blueprint before the build: the plan shown where it would stand, as glowing ghost blocks
 * that cost nothing and touch nothing, and sent to the app's «Plano» tab. The Doctor moves it
 * block by block, turns it, and then accepts it (she starts building) or discards it.
 */
public class KaltsitPreview {
    public static final String TAG = "kaltsit_preview";
    /** Ghost blocks at most (the ones on the surface are shown first). */
    private static final int MAX_GHOSTS = 3000;
    /** Blocks sent to the app at most. */
    private static final int MAX_APP_CELLS = 15000;

    static class Preview {
        final KaltsitBuilder.Plan plan;
        final ServerLevel level;
        /** Where the Doctor was looking when he asked: right, forward and up are his. */
        final Direction facing;
        final BlockPos baseAnchor;
        int right = 0, up = 0, forward = 0, turns = 0;
        final List<UUID> ghosts = new ArrayList<>();
        KaltsitBuilder.Job job;
        boolean partial = false;

        Preview(KaltsitBuilder.Plan plan, ServerLevel level, Direction facing, BlockPos baseAnchor) {
            this.plan = plan;
            this.level = level;
            this.facing = facing;
            this.baseAnchor = baseAnchor;
        }

        /** The site with the Doctor's moves and turns applied (turning about the plan's centre). */
        KaltsitBuilder.Site site() {
            Rotation base = rotationFacing(facing.getOpposite());
            Rotation rot = base.getRotated(turnRotation(turns));
            BlockPos front = new BlockPos(plan.width / 2, 0, 0);
            BlockPos centre = new BlockPos(plan.width / 2, 0, plan.depth() / 2);
            // The centre stays put when it turns.
            BlockPos origin0 = baseAnchor.subtract(front.rotate(base));
            BlockPos worldCentre = origin0.offset(centre.rotate(base));
            BlockPos origin = worldCentre.subtract(centre.rotate(rot));
            BlockPos shift = BlockPos.ZERO.relative(facing.getClockWise(), right).relative(facing, forward).above(up);
            BlockPos anchor = origin.offset(front.rotate(rot)).offset(shift);
            return new KaltsitBuilder.Site(level, anchor, rot);
        }
    }

    private static Preview current = null;

    public static boolean active() {
        return current != null;
    }

    static Rotation rotationFacing(Direction front) {
        for (Rotation r : Rotation.values()) {
            if (r.rotate(Direction.NORTH) == front) return r;
        }
        return Rotation.NONE;
    }

    static Rotation turnRotation(int turns) {
        return switch (Math.floorMod(turns, 4)) {
            case 1 -> Rotation.CLOCKWISE_90;
            case 2 -> Rotation.CLOCKWISE_180;
            case 3 -> Rotation.COUNTERCLOCKWISE_90;
            default -> Rotation.NONE;
        };
    }

    // ── showing it ──

    /** Shows a new plan in front of the Doctor (replacing any other preview). */
    public static Component show(ServerPlayer doctor, KaltsitBuilder.Plan plan) {
        clear();
        Direction d = doctor.getDirection();
        current = new Preview(plan, (ServerLevel) doctor.level(), d, doctor.blockPosition().relative(d, 3).below());
        return refresh(doctor, true);
    }

    public static Component move(ServerPlayer doctor, int right, int up, int forward) {
        if (current == null) return Component.literal("§7[Kal'tsit]§r No hay ningún plano a la vista.");
        current.right += right;
        current.up += up;
        current.forward += forward;
        return refresh(doctor, false);
    }

    public static Component rotate(ServerPlayer doctor, int turns) {
        if (current == null) return Component.literal("§7[Kal'tsit]§r No hay ningún plano a la vista.");
        current.turns += turns;
        return refresh(doctor, false);
    }

    /** She builds what the preview shows, exactly there. */
    public static Component confirm(ServerPlayer doctor) {
        if (current == null) return Component.literal("§7[Kal'tsit]§r No hay ningún plano que construir: pídeme uno primero.");
        KaltsitBuilder.Job j = KaltsitBuilder.prepare(doctor, current.plan, current.site());
        Component problem = KaltsitBuilder.begin(doctor, j);
        if (problem != null) return problem;
        clear();
        return null;
    }

    public static Component discard() {
        if (current == null) return Component.literal("§7[Kal'tsit]§r No hay ningún plano a la vista.");
        String name = current.plan.name;
        clear();
        KaltsitBridge.send("{\"type\":\"blueprint_clear\"}");
        return Component.literal("§b[Kal'tsit]§r Plano «" + name + "» descartado.");
    }

    private static Component refresh(ServerPlayer doctor, boolean first) {
        Preview p = current;
        removeGhosts();
        p.job = KaltsitBuilder.prepare(doctor, p.plan, p.site());
        spawnGhosts(p);
        sendToApp(doctor, p, "preview");

        Map<Item, Integer> need = KaltsitBuilder.needs(p.job);
        Map<Item, Integer> miss = KaltsitBuilder.lacks(p.job, doctor);
        int blocks = need.values().stream().mapToInt(Integer::intValue).sum();
        MutableComponent msg = Component.literal("§b[Kal'tsit]§r " + (first ? "Así quedaría" : "Ahora está así:") + " «" + p.plan.name + "» (" + blocks + " bloques");
        if (p.job.toClear > 0) msg.append(", despejaría " + p.job.toClear);
        if (!p.job.foundationCells.isEmpty()) msg.append(", " + p.job.foundationCells.size() + " de cimiento");
        msg.append(")");
        if (p.partial) msg.append(" §7(vista parcial)§r");
        msg.append(".");
        if (!p.job.creative && !miss.isEmpty()) msg.append(" §eFaltan§r ").append(listOf(miss)).append(".");
        if (p.job.problem != null) msg.append(" §c¡Ojo!§r " + p.job.problem + " esta posición.");
        if (first) {
            msg.append(" §7Di «constrúyela», «muévela 3 a la izquierda» (derecha, adelante, atrás, arriba, abajo), «gírala» o «descártala».");
        }
        return msg;
    }

    private static MutableComponent listOf(Map<Item, Integer> items) {
        MutableComponent c = Component.empty();
        int i = 0;
        for (Map.Entry<Item, Integer> e : items.entrySet()) {
            if (i > 0) c.append(", ");
            if (i == 8) {
                c.append("…");
                break;
            }
            c.append(e.getValue() + "× ").append(new net.minecraft.world.item.ItemStack(e.getKey()).getHoverName());
            i++;
        }
        return c;
    }

    // ── ghost blocks ──

    private static Method setBlockState, setTransformation, setGlowColor, setBrightness, setViewRange;

    private static void reflect() throws Exception {
        if (setBlockState != null) return;
        setBlockState = Display.BlockDisplay.class.getDeclaredMethod("setBlockState", BlockState.class);
        setTransformation = Display.class.getDeclaredMethod("setTransformation", Transformation.class);
        setGlowColor = Display.class.getDeclaredMethod("setGlowColorOverride", int.class);
        setBrightness = Display.class.getDeclaredMethod("setBrightnessOverride", Brightness.class);
        setViewRange = Display.class.getDeclaredMethod("setViewRange", float.class);
        for (Method m : new Method[] {setBlockState, setTransformation, setGlowColor, setBrightness, setViewRange}) m.setAccessible(true);
    }

    /** Blocks with at least one face in the open: the ones a ghost needs to show its shape. */
    private static List<Map.Entry<BlockPos, BlockState>> surface(KaltsitBuilder.Job j) {
        Set<BlockPos> solid = new HashSet<>();
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            if (!e.getValue().isAir()) solid.add(e.getKey());
        }
        List<Map.Entry<BlockPos, BlockState>> out = new ArrayList<>();
        for (Map.Entry<BlockPos, BlockState> e : j.target.entrySet()) {
            if (e.getValue().isAir()) continue;
            for (Direction d : Direction.values()) {
                if (!solid.contains(e.getKey().relative(d))) {
                    out.add(e);
                    break;
                }
            }
        }
        out.sort((a, b) -> Integer.compare(a.getKey().getY(), b.getKey().getY()));
        return out;
    }

    private static void spawnGhosts(Preview p) {
        try {
            reflect();
        } catch (Exception e) {
            System.out.println("[Kaltsit] Sin hologramas (no se pudo acceder a Display): " + e);
            return;
        }
        List<Map.Entry<BlockPos, BlockState>> cells = surface(p.job);
        p.partial = cells.size() > MAX_GHOSTS;
        // Slightly smaller than a block, so it never hides the ground it overlaps.
        Transformation shrink = new Transformation(new Vector3f(0.04f), new Quaternionf(), new Vector3f(0.92f), new Quaternionf());
        int n = 0;
        for (Map.Entry<BlockPos, BlockState> e : cells) {
            if (n++ >= MAX_GHOSTS) break;
            BlockPos pos = e.getKey();
            boolean blocked = !KaltsitBuilder.done(p.level, pos, e.getValue()) && !p.level.getBlockState(pos).isAir() && !p.level.getBlockState(pos).canBeReplaced();
            Display.BlockDisplay g = new Display.BlockDisplay(EntityTypes.BLOCK_DISPLAY, p.level);
            g.setPos(pos.getX(), pos.getY(), pos.getZ());
            try {
                setBlockState.invoke(g, e.getValue());
                setTransformation.invoke(g, shrink);
                // Blueprint cyan; amber where something is in the way and would be broken.
                setGlowColor.invoke(g, blocked ? 0xFFB040 : 0x55D0FF);
                setBrightness.invoke(g, new Brightness(15, 15));
                setViewRange.invoke(g, 2.0f);
            } catch (Exception ex) {
                return;
            }
            g.setGlowingTag(true);
            g.addTag(TAG);
            g.setNoGravity(true);
            // Known before it enters the world: the load event would take it for a stale ghost.
            p.ghosts.add(g.getUUID());
            p.level.addFreshEntity(g);
        }
    }

    private static void removeGhosts() {
        if (current == null) return;
        for (UUID id : current.ghosts) {
            Entity e = current.level.getEntity(id);
            if (e != null) e.discard();
        }
        current.ghosts.clear();
    }

    public static void clear() {
        removeGhosts();
        current = null;
    }

    /** A ghost saved in a chunk from an older session (the world closed mid-preview) goes. */
    public static void onEntityLoad(Entity e) {
        if (!e.entityTags().contains(TAG)) return;
        if (current != null && current.ghosts.contains(e.getUUID())) return;
        e.discard();
    }

    // ── the app's «Plano» tab ──

    private static String id(Object block) {
        if (block instanceof Item it) return BuiltInRegistries.ITEM.getKey(it).getPath();
        return BuiltInRegistries.BLOCK.getKey(((BlockState) block).getBlock()).getPath();
    }

    /** The plan (in its own space, as the Doctor sees it from the front) with its materials. */
    static void sendToApp(ServerPlayer doctor, Preview p, String status) {
        if (!KaltsitBridge.isConnected()) return;
        KaltsitBuilder.Plan plan = p.plan;
        JsonObject o = new JsonObject();
        o.addProperty("type", "blueprint");
        o.addProperty("status", status);
        o.addProperty("name", plan.name);
        // From 0 on every axis (a roof's eaves stick out before x = 0 and z = 0).
        int minX = Integer.MAX_VALUE, minY = Integer.MAX_VALUE, minZ = Integer.MAX_VALUE;
        for (Map.Entry<BlockPos, BlockState> e : plan.cells.entrySet()) {
            if (e.getValue().isAir()) continue;
            minX = Math.min(minX, e.getKey().getX());
            minY = Math.min(minY, e.getKey().getY());
            minZ = Math.min(minZ, e.getKey().getZ());
        }
        int w = 1, h = 1, d = 1;
        JsonArray blocks = new JsonArray();
        int n = 0;
        Rotation turn = turnRotation(p.turns);
        for (Map.Entry<BlockPos, BlockState> e : plan.cells.entrySet()) {
            if (e.getValue().isAir()) continue;
            BlockPos c = e.getKey();
            w = Math.max(w, c.getX() - minX + 1);
            h = Math.max(h, c.getY() - minY + 1);
            d = Math.max(d, c.getZ() - minZ + 1);
            if (n++ >= MAX_APP_CELLS) continue;
            JsonArray b = new JsonArray();
            b.add(c.getX() - minX);
            b.add(c.getY() - minY);
            b.add(c.getZ() - minZ);
            b.add(id(e.getValue().rotate(turn)));
            blocks.add(b);
        }
        JsonArray size = new JsonArray();
        size.add(w);
        size.add(h);
        size.add(d);
        o.add("size", size);
        o.add("blocks", blocks);
        o.addProperty("partial", n > MAX_APP_CELLS);
        o.addProperty("turns", Math.floorMod(p.turns, 4));
        JsonArray moved = new JsonArray();
        moved.add(p.right);
        moved.add(p.up);
        moved.add(p.forward);
        o.add("moved", moved);
        Map<Item, Integer> need = KaltsitBuilder.needs(p.job);
        Map<Item, Integer> miss = KaltsitBuilder.lacks(p.job, doctor);
        JsonArray mats = new JsonArray();
        for (Map.Entry<Item, Integer> e : need.entrySet()) {
            JsonArray m = new JsonArray();
            m.add(id(e.getKey()));
            m.add(e.getValue());
            m.add(miss.getOrDefault(e.getKey(), 0));
            mats.add(m);
        }
        o.add("materials", mats);
        o.addProperty("clear", p.job.toClear);
        o.addProperty("foundation", p.job.foundationCells.size());
        o.addProperty("creative", p.job.creative);
        if (p.job.problem != null) o.addProperty("problem", p.job.problem);
        KaltsitBridge.send(o.toString());
    }
}
