package ai.kaltsit.companion;

import net.minecraft.core.BlockPos;
import net.minecraft.core.component.DataComponents;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.InteractionResult;
import net.minecraft.world.SimpleMenuProvider;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.PathfinderMob;
import net.minecraft.world.entity.Pose;
import net.minecraft.world.entity.ai.attributes.AttributeSupplier;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.ai.goal.FloatGoal;
import net.minecraft.world.entity.ai.goal.LookAtPlayerGoal;
import net.minecraft.world.entity.ai.goal.RandomLookAroundGoal;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.food.FoodProperties;
import net.minecraft.world.inventory.ChestMenu;
import net.minecraft.world.inventory.MenuType;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.storage.ValueInput;
import net.minecraft.world.level.storage.ValueOutput;
import net.minecraft.world.phys.Vec3;

/**
 * Kal'tsit's body: a humanoid PathfinderMob that behaves like a player. She swims, looks at
 * whoever is near, sprints to catch up, crouches when the Doctor crouches, picks up what lies
 * on the ground into her 36-slot inventory (kept with the world), eats from it with the eating
 * animation, and drops everything where she falls, like a player.
 */
public class KaltsitEntity extends PathfinderMob {

    /** Ticks left until her next swing can hit (weapon cooldown, like a player's). */
    public int attackCooldown = 0;
    /** Healing the food in her hand will give when she finishes eating it. */
    private float pendingHeal = 0f;

    public KaltsitEntity(EntityType<? extends PathfinderMob> type, Level level) {
        super(type, level);
        this.setPersistenceRequired();
        this.setCanPickUpLoot(true);
        // Her equipment comes from her inventory: dying drops the inventory itself (see die).
        for (EquipmentSlot slot : EquipmentSlot.values()) {
            this.setDropChance(slot, 0.0f);
        }
    }

    public static AttributeSupplier.Builder createKaltsitAttributes() {
        return PathfinderMob.createMobAttributes()
            .add(Attributes.MAX_HEALTH, 40.0)
            .add(Attributes.MOVEMENT_SPEED, 0.28)
            .add(Attributes.ATTACK_DAMAGE, 1.0) // A fist, like a player; weapons add their own
            .add(Attributes.ATTACK_SPEED, 4.0) // A player's: a sword brings it to 1.6 per second
            .add(Attributes.ATTACK_KNOCKBACK, 0.4)
            .add(Attributes.FOLLOW_RANGE, 48.0)
            .add(Attributes.STEP_HEIGHT, 0.6)
            .add(Attributes.ARMOR, 2.0);
    }

    /** Swimming down on purpose (after the Doctor, along a path): the float goal lets her. */
    private boolean diving = false;

    @Override
    protected void registerGoals() {
        // Only head and swimming here: where she walks is decided by KaltsitMod's state machine.
        this.goalSelector.addGoal(0, new FloatGoal(this) {
            @Override
            public boolean canUse() {
                return super.canUse() && !diving;
            }
        });
        this.goalSelector.addGoal(7, new LookAtPlayerGoal(this, Player.class, 8.0f));
        this.goalSelector.addGoal(8, new RandomLookAroundGoal(this));
    }

    /** Ticks between two hits with what she holds (20 / attack speed). */
    public int attackInterval() {
        double speed = Math.max(0.5, this.getAttributeValue(Attributes.ATTACK_SPEED));
        return (int) Math.ceil(20.0 / speed);
    }

    @Override
    public void tick() {
        super.tick();
        if (this.level().isClientSide()) return;
        if (attackCooldown > 0) attackCooldown--;

        ServerPlayer doctor = KaltsitMod.cachedDoctor();
        boolean moving = this.getNavigation().isInProgress();
        if (this.isInWater()) {
            swimTick(doctor);
        } else {
            diving = false;
            if (this.getPose() == Pose.SWIMMING) this.setPose(Pose.STANDING);
            // Sprints when she has ground to make up, like a player running to catch up.
            boolean far = doctor != null && doctor.level() == this.level() && this.distanceTo(doctor) > 7.0;
            boolean urgent = KaltsitMod.getCurrentState() == KaltsitMod.CompanionState.COMBAT || KaltsitMod.getCurrentState() == KaltsitMod.CompanionState.ALERT;
            this.setSprinting(moving && (far || urgent) && !this.isUsingItem());
        }

        // Crouches with the Doctor when standing close by (a player's greeting, or sneaking along).
        boolean crouch = doctor != null && doctor.level() == this.level() && doctor.isShiftKeyDown() && this.distanceTo(doctor) < 6.0 && !moving && !this.isInWater();
        if (crouch != this.isShiftKeyDown()) {
            this.setShiftKeyDown(crouch);
            this.setPose(crouch ? Pose.CROUCHING : Pose.STANDING);
        }
    }

    // ── swimming like a player ──

    /**
     * In water she swims like a player: under water she sprint-swims (the horizontal swim and
     * its animation) towards air when her breath runs low, after the Doctor when he dives, or
     * along her path; at the surface the float goal keeps her up and the path pulls her along.
     */
    private void swimTick(ServerPlayer doctor) {
        boolean under = this.isUnderWater();
        float air = this.getAirSupply() / (float) Math.max(1, this.getMaxAirSupply());
        Vec3 target = null;
        boolean afterDoctor = false;
        if (under && air < 0.45f) {
            target = nearestAir();
        } else if (doctor != null && doctor.level() == this.level() && doctor.isUnderWater() && doctor.getY() < this.getY() + 1.0
            && KaltsitMod.getCurrentState() == KaltsitMod.CompanionState.FOLLOWING && this.distanceTo(doctor) > 2.5 && this.distanceTo(doctor) < 24.0) {
            // He dives: she goes down with him (her path finder only knows the surface).
            target = doctor.position().add(0, 0.4, 0);
            afterDoctor = true;
        } else if (this.getNavigation().getPath() != null && !this.getNavigation().isDone()) {
            target = Vec3.atBottomCenterOf(this.getNavigation().getPath().getNextNodePos()).add(0, 0.4, 0);
        }
        // Next to him under water she stays down with him, while her breath lasts.
        boolean besideHim = target == null && air >= 0.45f && doctor != null && doctor.level() == this.level() && doctor.isUnderWater() && this.distanceTo(doctor) <= 2.5;
        diving = (afterDoctor && target.y < this.getY() + 0.5) || besideHim;
        boolean swimming = target != null && (under || diving);
        if (target != null) {
            Vec3 d = target.subtract(this.position().add(0, 0.5, 0));
            double len = d.length();
            if (len > 0.35) {
                Vec3 dir = d.scale(1.0 / len);
                double max = swimming ? 0.24 : 0.13; // sprint-swimming under water, slower on top
                Vec3 v = this.getDeltaMovement().add(dir.scale(swimming ? 0.05 : 0.03));
                if (v.length() > max) v = v.scale(max / v.length());
                this.setDeltaMovement(v);
                float yaw = (float) (Math.atan2(d.z, d.x) * (180.0 / Math.PI)) - 90.0f;
                this.setYRot(yaw);
                this.yBodyRot = yaw;
                this.getLookControl().setLookAt(target.x, target.y, target.z);
            }
        }
        // Sprinting under water is what swimming is; the pose turns her horizontal.
        this.setSprinting(swimming);
        if (this.isSwimming() && under) {
            if (this.getPose() != Pose.SWIMMING) this.setPose(Pose.SWIMMING);
        } else if (this.getPose() == Pose.SWIMMING) {
            this.setPose(Pose.STANDING);
        }
    }

    /** The nearest place to take a breath: straight up if open, else air beside the water. */
    private Vec3 nearestAir() {
        BlockPos at = this.blockPosition();
        for (int dy = 1; dy <= 24; dy++) {
            BlockPos p = at.above(dy);
            var s = this.level().getBlockState(p);
            if (s.getFluidState().isEmpty() && !s.blocksMotion()) return Vec3.atBottomCenterOf(p).add(0, 0.3, 0);
            if (s.blocksMotion()) break; // A ceiling: look around instead
        }
        BlockPos best = null;
        double bestD = Double.MAX_VALUE;
        for (int dx = -8; dx <= 8; dx++)
            for (int dz = -8; dz <= 8; dz++)
                for (int dy = -2; dy <= 6; dy++) {
                    BlockPos p = at.offset(dx, dy, dz);
                    var s = this.level().getBlockState(p);
                    if (!s.getFluidState().isEmpty() || s.blocksMotion()) continue;
                    if (this.level().getFluidState(p.below()).isEmpty() && !this.level().getBlockState(p.below()).blocksMotion()) continue;
                    double d = p.distSqr(at);
                    if (d < bestD) {
                        bestD = d;
                        best = p;
                    }
                }
        return best != null ? Vec3.atBottomCenterOf(best).add(0, 0.3, 0) : this.position().add(0, 3, 0);
    }

    // ── items on the ground ──

    @Override
    public boolean wantsToPickUp(ServerLevel level, ItemStack stack) {
        return !stack.isEmpty() && KaltsitWorldInteractor.companionInventory.canAddItem(stack);
    }

    @Override
    protected void pickUpItem(ServerLevel level, ItemEntity item) {
        ItemStack stack = item.getItem();
        int before = stack.getCount();
        ItemStack rest = KaltsitWorldInteractor.companionInventory.addItem(stack.copy());
        int taken = before - rest.getCount();
        if (taken <= 0) return;
        this.onItemPickup(item);
        this.take(item, taken);
        if (rest.isEmpty()) {
            item.discard();
        } else {
            item.setItem(rest);
        }
        KaltsitWorldInteractor.updateEquippedGear(this, KaltsitMod.getCurrentState());
    }

    // ── eating, with the animation and sound of a player ──

    /** The slot of the most nourishing food in her inventory, or -1. */
    private int bestFood() {
        var inv = KaltsitWorldInteractor.companionInventory;
        int best = -1;
        int bestNutrition = 0;
        for (int i = 0; i < inv.getContainerSize(); i++) {
            FoodProperties food = inv.getItem(i).get(DataComponents.FOOD);
            if (food != null && food.nutrition() > bestNutrition && inv.getItem(i).has(DataComponents.CONSUMABLE)) {
                best = i;
                bestNutrition = food.nutrition();
            }
        }
        return best;
    }

    /** Starts eating the best food of her inventory (or a bite of the Doctor's); false if there is none or she is busy. */
    public boolean startEating() {
        if (this.isUsingItem()) return false;
        int best = bestFood();
        // Nothing of her own: a bite from the Doctor's inventory (he is told).
        if (best < 0 && KaltsitWorldInteractor.takeFoodFromDoctor(KaltsitMod.cachedDoctor())) best = bestFood();
        if (best < 0) return false;
        var inv = KaltsitWorldInteractor.companionInventory;
        int bestNutrition = inv.getItem(best).get(DataComponents.FOOD).nutrition();
        ItemStack bite = inv.getItem(best).split(1);
        inv.setChanged();
        this.pendingHeal = bestNutrition;
        this.setItemSlot(EquipmentSlot.MAINHAND, bite);
        this.startUsingItem(InteractionHand.MAIN_HAND);
        return true;
    }

    @Override
    protected void completeUsingItem() {
        boolean wasFood = this.getUseItem().has(DataComponents.FOOD);
        super.completeUsingItem();
        if (wasFood && pendingHeal > 0) {
            this.heal(pendingHeal);
            pendingHeal = 0f;
        }
        // Back to what she was holding for her task.
        this.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
        KaltsitWorldInteractor.updateEquippedGear(this, KaltsitMod.getCurrentState());
    }

    @Override
    public void stopUsingItem() {
        // Interrupted mid-bite (hit, new order): the food goes back to her inventory.
        if (!this.level().isClientSide() && this.isUsingItem() && this.getUseItem().has(DataComponents.FOOD)) {
            ItemStack food = this.getUseItem().copy();
            super.stopUsingItem();
            this.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
            KaltsitWorldInteractor.companionInventory.addItem(food);
            pendingHeal = 0f;
            KaltsitWorldInteractor.updateEquippedGear(this, KaltsitMod.getCurrentState());
            return;
        }
        super.stopUsingItem();
    }

    // ── dying like a player ──

    @Override
    public void die(DamageSource source) {
        if (this.level() instanceof ServerLevel server) {
            // The main hand only shows a stack of the inventory: drop it once, from there.
            this.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
            for (EquipmentSlot slot : EquipmentSlot.values()) {
                ItemStack worn = this.getItemBySlot(slot);
                if (!worn.isEmpty()) {
                    this.spawnAtLocation(server, worn.copy());
                    this.setItemSlot(slot, ItemStack.EMPTY);
                }
            }
            for (ItemStack stack : KaltsitWorldInteractor.companionInventory.removeAllItems()) {
                if (!stack.isEmpty()) this.spawnAtLocation(server, stack);
            }
            ServerPlayer doctor = KaltsitMod.cachedDoctor();
            if (doctor != null) {
                boolean survival = !doctor.isCreative();
                doctor.sendSystemMessage(Component.literal("§c[Kal'tsit]§r He caído. Mis cosas quedaron donde estaba; vuelvo en unos segundos"
                    + (survival ? (KaltsitSettings.get().bedPos != null ? ", en mi cama." : ", en el punto de aparición del mundo (no tengo cama).") : ", a tu lado.")));
            }
        }
        super.die(source);
        if (!this.level().isClientSide()) KaltsitMod.onDeath();
    }

    // ── her inventory travels with the world save ──

    @Override
    protected void addAdditionalSaveData(ValueOutput out) {
        super.addAdditionalSaveData(out);
        KaltsitWorldInteractor.companionInventory.storeAsItemList(out.list("KaltsitInventory", ItemStack.CODEC));
    }

    @Override
    protected void readAdditionalSaveData(ValueInput in) {
        super.readAdditionalSaveData(in);
        KaltsitWorldInteractor.companionInventory.clearContent();
        KaltsitWorldInteractor.companionInventory.fromItemList(in.listOrEmpty("KaltsitInventory", ItemStack.CODEC));
        // The saved main hand is a copy of a stack still in the inventory: it is chosen again.
        this.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
    }

    @Override
    protected InteractionResult mobInteract(Player player, InteractionHand hand) {
        if (!this.level().isClientSide() && hand == InteractionHand.MAIN_HAND) {
            ItemStack held = player.getItemInHand(hand);
            boolean shift = player.isShiftKeyDown();

            // Shift + clic derecho o clic derecho con la mano vacía: abre su inventario.
            if (shift || held.isEmpty()) {
                if (player instanceof ServerPlayer serverPlayer) {
                    serverPlayer.openMenu(new SimpleMenuProvider(
                        (syncId, playerInv, p) -> new ChestMenu(MenuType.GENERIC_9x4, syncId, playerInv, KaltsitWorldInteractor.companionInventory, 4),
                        Component.literal("Inventario de Kal'tsit (36 slots)")
                    ));
                    return InteractionResult.SUCCESS;
                }
            }

            // Clic derecho con un objeto: se lo das.
            if (!held.isEmpty()) {
                int before = held.getCount();
                ItemStack remaining = KaltsitWorldInteractor.companionInventory.addItem(held.copy());
                int transferred = before - remaining.getCount();
                if (transferred > 0) {
                    String name = held.getHoverName().getString();
                    held.shrink(transferred);
                    player.sendSystemMessage(Component.literal(
                        "§b[Kal'tsit]§r Recibido: §e" + name + (transferred > 1 ? " (x" + transferred + ")" : "") + "§r, guardado en mi inventario."
                    ));
                    this.swing(InteractionHand.MAIN_HAND);
                    KaltsitWorldInteractor.updateEquippedGear(this, KaltsitMod.getCurrentState());
                    return InteractionResult.SUCCESS;
                }
                player.sendSystemMessage(Component.literal("§c[Kal'tsit]§r No me cabe nada más en el inventario."));
                return InteractionResult.SUCCESS;
            }
        }
        return super.mobInteract(player, hand);
    }
}
