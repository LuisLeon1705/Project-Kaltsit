package ai.kaltsit.companion;

import net.fabricmc.api.ClientModInitializer;
import net.fabricmc.fabric.api.client.rendering.v1.EntityRendererRegistry;
import net.minecraft.client.model.HumanoidModel;
import net.minecraft.client.model.geom.ModelLayers;
import net.minecraft.client.renderer.entity.ArmorModelSet;
import net.minecraft.client.renderer.entity.EntityRendererProvider;
import net.minecraft.client.renderer.entity.HumanoidMobRenderer;
import net.minecraft.client.renderer.entity.layers.HumanoidArmorLayer;
import net.minecraft.client.renderer.entity.state.HumanoidRenderState;
import net.minecraft.resources.Identifier;

/**
 * Renders Kal'tsit with the player's body: the player mesh (its outer skin layers — hat,
 * jacket, sleeves, trousers — are parts of it), worn armour, the item in each hand, and the
 * humanoid animations driven by what the entity does (walking, sprinting, crouching, swimming,
 * swinging, eating, drawing a bow).
 *
 * Not with AvatarRenderState / PlayerModel: in 26.2 the EntityRenderDispatcher hands any
 * AvatarRenderState to the vanilla player renderer, which paints the state's default skin
 * (Steve) instead of hers.
 */
public class KaltsitClient implements ClientModInitializer {
    private static final Identifier KALTSIT_SKIN = Identifier.fromNamespaceAndPath("kaltsit", "textures/entity/kaltsit.png");
    /** Her skin has the classic (4 px) arms; true for a skin with Alex-style 3 px arms. */
    private static final boolean SLIM = false;

    @Override
    public void onInitializeClient() {
        System.out.println("[Kaltsit] Registrando renderizador de jugador para Kal'tsit...");
        EntityRendererRegistry.register(KaltsitMod.KALTSIT_ENTITY, KaltsitRenderer::new);
    }

    static class KaltsitRenderer extends HumanoidMobRenderer<KaltsitEntity, HumanoidRenderState, HumanoidModel<HumanoidRenderState>> {
        KaltsitRenderer(EntityRendererProvider.Context ctx) {
            super(ctx, new HumanoidModel<>(ctx.bakeLayer(SLIM ? ModelLayers.PLAYER_SLIM : ModelLayers.PLAYER)), 0.5f);
            this.addLayer(new HumanoidArmorLayer<>(this,
                ArmorModelSet.bake(SLIM ? ModelLayers.PLAYER_SLIM_ARMOR : ModelLayers.PLAYER_ARMOR, ctx.getModelSet(), HumanoidModel::new),
                ctx.getEquipmentRenderer()));
        }

        @Override
        public HumanoidRenderState createRenderState() {
            return new HumanoidRenderState();
        }

        @Override
        public Identifier getTextureLocation(HumanoidRenderState state) {
            return KALTSIT_SKIN;
        }
    }
}
