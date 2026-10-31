package ai.kaltsit.companion;

import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.tags.BlockTags;
import net.minecraft.tags.ItemTags;
import net.minecraft.tags.TagKey;
import net.minecraft.world.Container;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.SimpleContainer;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.entity.NeutralMob;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.entity.monster.Creeper;
import net.minecraft.world.entity.monster.Enemy;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.block.LeavesBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BedPart;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;

import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * World Interaction Engine: executes block mining with quota, tree chopping,
 * chest storage, auto-eating, proactive lighting, bed placement, and procedural architecture.
 * Fully integrates the 250 canonical block dictionary from blocks.id.txt with Spanish aliases.
 */
public class KaltsitWorldInteractor {

    /** Companion 36-slot internal inventory for gathered materials */
    public static final SimpleContainer companionInventory = new SimpleContainer(36);

    /** Canonical block dictionary mapping names and Spanish aliases to Minecraft identifiers */
    private static final Map<String, String> BLOCK_DICT = new HashMap<>();

    static {
        // Register raw IDs from catalog
        BLOCK_DICT.put("air", "minecraft:air");
        BLOCK_DICT.put("stone", "minecraft:stone");
        BLOCK_DICT.put("granite", "minecraft:granite");
        BLOCK_DICT.put("polished_granite", "minecraft:polished_granite");
        BLOCK_DICT.put("diorite", "minecraft:diorite");
        BLOCK_DICT.put("polished_diorite", "minecraft:polished_diorite");
        BLOCK_DICT.put("andesite", "minecraft:andesite");
        BLOCK_DICT.put("polished_andesite", "minecraft:polished_andesite");
        BLOCK_DICT.put("grass_block", "minecraft:grass_block");
        BLOCK_DICT.put("dirt", "minecraft:dirt");
        BLOCK_DICT.put("coarse_dirt", "minecraft:coarse_dirt");
        BLOCK_DICT.put("podzol", "minecraft:podzol");
        BLOCK_DICT.put("cobblestone", "minecraft:cobblestone");
        BLOCK_DICT.put("oak_planks", "minecraft:oak_planks");
        BLOCK_DICT.put("spruce_planks", "minecraft:spruce_planks");
        BLOCK_DICT.put("birch_planks", "minecraft:birch_planks");
        BLOCK_DICT.put("jungle_planks", "minecraft:jungle_planks");
        BLOCK_DICT.put("acacia_planks", "minecraft:acacia_planks");
        BLOCK_DICT.put("dark_oak_planks", "minecraft:dark_oak_planks");
        BLOCK_DICT.put("mangrove_planks", "minecraft:mangrove_planks");
        BLOCK_DICT.put("crimson_planks", "minecraft:crimson_planks");
        BLOCK_DICT.put("warped_planks", "minecraft:warped_planks");
        BLOCK_DICT.put("bamboo_planks", "minecraft:bamboo_planks");
        BLOCK_DICT.put("oak_sapling", "minecraft:oak_sapling");
        BLOCK_DICT.put("spruce_sapling", "minecraft:spruce_sapling");
        BLOCK_DICT.put("birch_sapling", "minecraft:birch_sapling");
        BLOCK_DICT.put("jungle_sapling", "minecraft:jungle_sapling");
        BLOCK_DICT.put("acacia_sapling", "minecraft:acacia_sapling");
        BLOCK_DICT.put("dark_oak_sapling", "minecraft:dark_oak_sapling");
        BLOCK_DICT.put("mangrove_propagule", "minecraft:mangrove_propagule");
        BLOCK_DICT.put("bedrock", "minecraft:bedrock");
        BLOCK_DICT.put("sand", "minecraft:sand");
        BLOCK_DICT.put("red_sand", "minecraft:red_sand");
        BLOCK_DICT.put("gravel", "minecraft:gravel");
        BLOCK_DICT.put("gold_ore", "minecraft:gold_ore");
        BLOCK_DICT.put("deepslate_gold_ore", "minecraft:deepslate_gold_ore");
        BLOCK_DICT.put("iron_ore", "minecraft:iron_ore");
        BLOCK_DICT.put("deepslate_iron_ore", "minecraft:deepslate_iron_ore");
        BLOCK_DICT.put("coal_ore", "minecraft:coal_ore");
        BLOCK_DICT.put("deepslate_coal_ore", "minecraft:deepslate_coal_ore");
        BLOCK_DICT.put("nether_gold_ore", "minecraft:nether_gold_ore");
        BLOCK_DICT.put("oak_log", "minecraft:oak_log");
        BLOCK_DICT.put("spruce_log", "minecraft:spruce_log");
        BLOCK_DICT.put("birch_log", "minecraft:birch_log");
        BLOCK_DICT.put("jungle_log", "minecraft:jungle_log");
        BLOCK_DICT.put("acacia_log", "minecraft:acacia_log");
        BLOCK_DICT.put("dark_oak_log", "minecraft:dark_oak_log");
        BLOCK_DICT.put("mangrove_log", "minecraft:mangrove_log");
        BLOCK_DICT.put("crimson_stem", "minecraft:crimson_stem");
        BLOCK_DICT.put("warped_stem", "minecraft:warped_stem");
        BLOCK_DICT.put("bamboo_block", "minecraft:bamboo_block");
        BLOCK_DICT.put("oak_leaves", "minecraft:oak_leaves");
        BLOCK_DICT.put("spruce_leaves", "minecraft:spruce_leaves");
        BLOCK_DICT.put("birch_leaves", "minecraft:birch_leaves");
        BLOCK_DICT.put("jungle_leaves", "minecraft:jungle_leaves");
        BLOCK_DICT.put("acacia_leaves", "minecraft:acacia_leaves");
        BLOCK_DICT.put("dark_oak_leaves", "minecraft:dark_oak_leaves");
        BLOCK_DICT.put("mangrove_leaves", "minecraft:mangrove_leaves");
        BLOCK_DICT.put("azalea_leaves", "minecraft:azalea_leaves");
        BLOCK_DICT.put("flowering_azalea_leaves", "minecraft:flowering_azalea_leaves");
        BLOCK_DICT.put("sponge", "minecraft:sponge");
        BLOCK_DICT.put("wet_sponge", "minecraft:wet_sponge");
        BLOCK_DICT.put("glass", "minecraft:glass");
        BLOCK_DICT.put("tinted_glass", "minecraft:tinted_glass");
        BLOCK_DICT.put("lapis_ore", "minecraft:lapis_ore");
        BLOCK_DICT.put("deepslate_lapis_ore", "minecraft:deepslate_lapis_ore");
        BLOCK_DICT.put("lapis_block", "minecraft:lapis_block");
        BLOCK_DICT.put("dispenser", "minecraft:dispenser");
        BLOCK_DICT.put("sandstone", "minecraft:sandstone");
        BLOCK_DICT.put("chiseled_sandstone", "minecraft:chiseled_sandstone");
        BLOCK_DICT.put("cut_sandstone", "minecraft:cut_sandstone");
        BLOCK_DICT.put("note_block", "minecraft:note_block");
        BLOCK_DICT.put("white_bed", "minecraft:white_bed");
        BLOCK_DICT.put("orange_bed", "minecraft:orange_bed");
        BLOCK_DICT.put("magenta_bed", "minecraft:magenta_bed");
        BLOCK_DICT.put("light_blue_bed", "minecraft:light_blue_bed");
        BLOCK_DICT.put("yellow_bed", "minecraft:yellow_bed");
        BLOCK_DICT.put("lime_bed", "minecraft:lime_bed");
        BLOCK_DICT.put("pink_bed", "minecraft:pink_bed");
        BLOCK_DICT.put("gray_bed", "minecraft:gray_bed");
        BLOCK_DICT.put("light_gray_bed", "minecraft:light_gray_bed");
        BLOCK_DICT.put("cyan_bed", "minecraft:cyan_bed");
        BLOCK_DICT.put("purple_bed", "minecraft:purple_bed");
        BLOCK_DICT.put("blue_bed", "minecraft:blue_bed");
        BLOCK_DICT.put("brown_bed", "minecraft:brown_bed");
        BLOCK_DICT.put("green_bed", "minecraft:green_bed");
        BLOCK_DICT.put("red_bed", "minecraft:red_bed");
        BLOCK_DICT.put("black_bed", "minecraft:black_bed");
        BLOCK_DICT.put("powered_rail", "minecraft:powered_rail");
        BLOCK_DICT.put("detector_rail", "minecraft:detector_rail");
        BLOCK_DICT.put("sticky_piston", "minecraft:sticky_piston");
        BLOCK_DICT.put("cobweb", "minecraft:cobweb");
        BLOCK_DICT.put("grass", "minecraft:grass");
        BLOCK_DICT.put("fern", "minecraft:fern");
        BLOCK_DICT.put("dead_bush", "minecraft:dead_bush");
        BLOCK_DICT.put("seagrass", "minecraft:seagrass");
        BLOCK_DICT.put("tall_seagrass", "minecraft:tall_seagrass");
        BLOCK_DICT.put("sea_pickle", "minecraft:sea_pickle");
        BLOCK_DICT.put("piston", "minecraft:piston");
        BLOCK_DICT.put("white_wool", "minecraft:white_wool");
        BLOCK_DICT.put("orange_wool", "minecraft:orange_wool");
        BLOCK_DICT.put("magenta_wool", "minecraft:magenta_wool");
        BLOCK_DICT.put("light_blue_wool", "minecraft:light_blue_wool");
        BLOCK_DICT.put("yellow_wool", "minecraft:yellow_wool");
        BLOCK_DICT.put("lime_wool", "minecraft:lime_wool");
        BLOCK_DICT.put("pink_wool", "minecraft:pink_wool");
        BLOCK_DICT.put("gray_wool", "minecraft:gray_wool");
        BLOCK_DICT.put("light_gray_wool", "minecraft:light_gray_wool");
        BLOCK_DICT.put("cyan_wool", "minecraft:cyan_wool");
        BLOCK_DICT.put("purple_wool", "minecraft:purple_wool");
        BLOCK_DICT.put("blue_wool", "minecraft:blue_wool");
        BLOCK_DICT.put("brown_wool", "minecraft:brown_wool");
        BLOCK_DICT.put("green_wool", "minecraft:green_wool");
        BLOCK_DICT.put("red_wool", "minecraft:red_wool");
        BLOCK_DICT.put("black_wool", "minecraft:black_wool");
        BLOCK_DICT.put("moving_piston", "minecraft:moving_piston");
        BLOCK_DICT.put("dandelion", "minecraft:dandelion");
        BLOCK_DICT.put("poppy", "minecraft:poppy");
        BLOCK_DICT.put("blue_orchid", "minecraft:blue_orchid");
        BLOCK_DICT.put("allium", "minecraft:allium");
        BLOCK_DICT.put("azure_bluet", "minecraft:azure_bluet");
        BLOCK_DICT.put("red_tulip", "minecraft:red_tulip");
        BLOCK_DICT.put("orange_tulip", "minecraft:orange_tulip");
        BLOCK_DICT.put("white_tulip", "minecraft:white_tulip");
        BLOCK_DICT.put("pink_tulip", "minecraft:pink_tulip");
        BLOCK_DICT.put("oxeye_daisy", "minecraft:oxeye_daisy");
        BLOCK_DICT.put("cornflower", "minecraft:cornflower");
        BLOCK_DICT.put("lily_of_the_valley", "minecraft:lily_of_the_valley");
        BLOCK_DICT.put("wither_rose", "minecraft:wither_rose");
        BLOCK_DICT.put("spore_blossom", "minecraft:spore_blossom");
        BLOCK_DICT.put("brown_mushroom", "minecraft:brown_mushroom");
        BLOCK_DICT.put("red_mushroom", "minecraft:red_mushroom");
        BLOCK_DICT.put("crimson_fungus", "minecraft:crimson_fungus");
        BLOCK_DICT.put("warped_fungus", "minecraft:warped_fungus");
        BLOCK_DICT.put("crimson_roots", "minecraft:crimson_roots");
        BLOCK_DICT.put("warped_roots", "minecraft:warped_roots");
        BLOCK_DICT.put("nether_sprouts", "minecraft:nether_sprouts");
        BLOCK_DICT.put("weeping_vines", "minecraft:weeping_vines");
        BLOCK_DICT.put("twisting_vines", "minecraft:twisting_vines");
        BLOCK_DICT.put("sugar_cane", "minecraft:sugar_cane");
        BLOCK_DICT.put("kelp", "minecraft:kelp");
        BLOCK_DICT.put("moss_carpet", "minecraft:moss_carpet");
        BLOCK_DICT.put("moss_block", "minecraft:moss_block");
        BLOCK_DICT.put("flowering_azalea", "minecraft:flowering_azalea");
        BLOCK_DICT.put("azalea", "minecraft:azalea");
        BLOCK_DICT.put("hay_block", "minecraft:hay_block");
        BLOCK_DICT.put("terracotta", "minecraft:terracotta");
        BLOCK_DICT.put("white_terracotta", "minecraft:white_terracotta");
        BLOCK_DICT.put("orange_terracotta", "minecraft:orange_terracotta");
        BLOCK_DICT.put("magenta_terracotta", "minecraft:magenta_terracotta");
        BLOCK_DICT.put("light_blue_terracotta", "minecraft:light_blue_terracotta");
        BLOCK_DICT.put("yellow_terracotta", "minecraft:yellow_terracotta");
        BLOCK_DICT.put("lime_terracotta", "minecraft:lime_terracotta");
        BLOCK_DICT.put("pink_terracotta", "minecraft:pink_terracotta");
        BLOCK_DICT.put("gray_terracotta", "minecraft:gray_terracotta");
        BLOCK_DICT.put("light_gray_terracotta", "minecraft:light_gray_terracotta");
        BLOCK_DICT.put("cyan_terracotta", "minecraft:cyan_terracotta");
        BLOCK_DICT.put("purple_terracotta", "minecraft:purple_terracotta");
        BLOCK_DICT.put("blue_terracotta", "minecraft:blue_terracotta");
        BLOCK_DICT.put("brown_terracotta", "minecraft:brown_terracotta");
        BLOCK_DICT.put("green_terracotta", "minecraft:green_terracotta");
        BLOCK_DICT.put("red_terracotta", "minecraft:red_terracotta");
        BLOCK_DICT.put("black_terracotta", "minecraft:black_terracotta");
        BLOCK_DICT.put("barrel", "minecraft:barrel");
        BLOCK_DICT.put("smoker", "minecraft:smoker");
        BLOCK_DICT.put("blast_furnace", "minecraft:blast_furnace");
        BLOCK_DICT.put("cartography_table", "minecraft:cartography_table");
        BLOCK_DICT.put("fletching_table", "minecraft:fletching_table");
        BLOCK_DICT.put("grindstone", "minecraft:grindstone");
        BLOCK_DICT.put("lectern", "minecraft:lectern");
        BLOCK_DICT.put("loom", "minecraft:loom");
        BLOCK_DICT.put("smithing_table", "minecraft:smithing_table");
        BLOCK_DICT.put("stonecutter", "minecraft:stonecutter");
        BLOCK_DICT.put("bell", "minecraft:bell");
        BLOCK_DICT.put("lantern", "minecraft:lantern");
        BLOCK_DICT.put("soul_lantern", "minecraft:soul_lantern");
        BLOCK_DICT.put("campfire", "minecraft:campfire");
        BLOCK_DICT.put("soul_campfire", "minecraft:soul_campfire");
        BLOCK_DICT.put("shroomlight", "minecraft:shroomlight");
        BLOCK_DICT.put("bee_nest", "minecraft:bee_nest");
        BLOCK_DICT.put("beehive", "minecraft:beehive");
        BLOCK_DICT.put("honey_block", "minecraft:honey_block");
        BLOCK_DICT.put("honeycomb_block", "minecraft:honeycomb_block");
        BLOCK_DICT.put("lodestone", "minecraft:lodestone");
        BLOCK_DICT.put("crying_obsidian", "minecraft:crying_obsidian");
        BLOCK_DICT.put("respawn_anchor", "minecraft:respawn_anchor");
        BLOCK_DICT.put("ancient_debris", "minecraft:ancient_debris");
        BLOCK_DICT.put("netherite_block", "minecraft:netherite_block");
        BLOCK_DICT.put("blackstone", "minecraft:blackstone");
        BLOCK_DICT.put("blackstone_slab", "minecraft:blackstone_slab");
        BLOCK_DICT.put("blackstone_stairs", "minecraft:blackstone_stairs");
        BLOCK_DICT.put("gilded_blackstone", "minecraft:gilded_blackstone");
        BLOCK_DICT.put("polished_blackstone", "minecraft:polished_blackstone");
        BLOCK_DICT.put("polished_blackstone_slab", "minecraft:polished_blackstone_slab");
        BLOCK_DICT.put("polished_blackstone_stairs", "minecraft:polished_blackstone_stairs");
        BLOCK_DICT.put("chiseled_polished_blackstone", "minecraft:chiseled_polished_blackstone");
        BLOCK_DICT.put("polished_blackstone_bricks", "minecraft:polished_blackstone_bricks");
        BLOCK_DICT.put("polished_blackstone_brick_slab", "minecraft:polished_blackstone_brick_slab");
        BLOCK_DICT.put("polished_blackstone_brick_stairs", "minecraft:polished_blackstone_brick_stairs");
        BLOCK_DICT.put("cracked_polished_blackstone_bricks", "minecraft:cracked_polished_blackstone_bricks");
        BLOCK_DICT.put("respawn_anchor", "minecraft:respawn_anchor");
        BLOCK_DICT.put("crying_obsidian", "minecraft:crying_obsidian");
        BLOCK_DICT.put("ancient_debris", "minecraft:ancient_debris");
        BLOCK_DICT.put("netherite_block", "minecraft:netherite_block");
        BLOCK_DICT.put("blackstone", "minecraft:blackstone");
        BLOCK_DICT.put("blackstone_slab", "minecraft:blackstone_slab");
        BLOCK_DICT.put("blackstone_stairs", "minecraft:blackstone_stairs");
        BLOCK_DICT.put("gilded_blackstone", "minecraft:gilded_blackstone");
        BLOCK_DICT.put("polished_blackstone", "minecraft:polished_blackstone");
        BLOCK_DICT.put("polished_blackstone_slab", "minecraft:polished_blackstone_slab");
        BLOCK_DICT.put("polished_blackstone_stairs", "minecraft:polished_blackstone_stairs");
        BLOCK_DICT.put("chiseled_polished_blackstone", "minecraft:chiseled_polished_blackstone");
        BLOCK_DICT.put("polished_blackstone_bricks", "minecraft:polished_blackstone_bricks");
        BLOCK_DICT.put("polished_blackstone_brick_slab", "minecraft:polished_blackstone_brick_slab");
        BLOCK_DICT.put("polished_blackstone_brick_stairs", "minecraft:polished_blackstone_brick_stairs");
        BLOCK_DICT.put("cracked_polished_blackstone_bricks", "minecraft:cracked_polished_blackstone_bricks");
        BLOCK_DICT.put("soul_soil", "minecraft:soul_soil");
        BLOCK_DICT.put("budding_amethyst", "minecraft:budding_amethyst");
        BLOCK_DICT.put("amethyst_block", "minecraft:amethyst_block");
        BLOCK_DICT.put("amethyst_cluster", "minecraft:amethyst_cluster");
        BLOCK_DICT.put("small_amethyst_bud", "minecraft:small_amethyst_bud");
        BLOCK_DICT.put("medium_amethyst_bud", "minecraft:medium_amethyst_bud");
        BLOCK_DICT.put("large_amethyst_bud", "minecraft:large_amethyst_bud");
        BLOCK_DICT.put("calcite", "minecraft:calcite");
        BLOCK_DICT.put("tuff", "minecraft:tuff");
        BLOCK_DICT.put("deepslate", "minecraft:deepslate");
        BLOCK_DICT.put("cobbled_deepslate", "minecraft:cobbled_deepslate");
        BLOCK_DICT.put("polished_deepslate", "minecraft:polished_deepslate");
        BLOCK_DICT.put("deepslate_bricks", "minecraft:deepslate_bricks");
        BLOCK_DICT.put("cracked_deepslate_bricks", "minecraft:cracked_deepslate_bricks");
        BLOCK_DICT.put("deepslate_tiles", "minecraft:deepslate_tiles");
        BLOCK_DICT.put("cracked_deepslate_tiles", "minecraft:cracked_deepslate_tiles");
        BLOCK_DICT.put("chiseled_deepslate", "minecraft:chiseled_deepslate");
        BLOCK_DICT.put("infested_deepslate", "minecraft:infested_deepslate");
        BLOCK_DICT.put("cobbled_deepslate_stairs", "minecraft:cobbled_deepslate_stairs");
        BLOCK_DICT.put("cobbled_deepslate_slab", "minecraft:cobbled_deepslate_slab");
        BLOCK_DICT.put("cobbled_deepslate_wall", "minecraft:cobbled_deepslate_wall");
        BLOCK_DICT.put("polished_deepslate_stairs", "minecraft:polished_deepslate_stairs");
        BLOCK_DICT.put("polished_deepslate_slab", "minecraft:polished_deepslate_slab");
        BLOCK_DICT.put("polished_deepslate_wall", "minecraft:polished_deepslate_wall");
        BLOCK_DICT.put("deepslate_brick_stairs", "minecraft:deepslate_brick_stairs");
        BLOCK_DICT.put("deepslate_brick_slab", "minecraft:deepslate_brick_slab");
        BLOCK_DICT.put("deepslate_brick_wall", "minecraft:deepslate_brick_wall");
        BLOCK_DICT.put("deepslate_tile_stairs", "minecraft:deepslate_tile_stairs");
        BLOCK_DICT.put("deepslate_tile_slab", "minecraft:deepslate_tile_slab");
        BLOCK_DICT.put("deepslate_tile_wall", "minecraft:deepslate_tile_wall");
        BLOCK_DICT.put("chiseled_bookshelf", "minecraft:chiseled_bookshelf");
        BLOCK_DICT.put("bamboo_mosaic", "minecraft:bamboo_mosaic");
        BLOCK_DICT.put("bamboo_mosaic_slab", "minecraft:bamboo_mosaic_slab");
        BLOCK_DICT.put("bamboo_mosaic_stairs", "minecraft:bamboo_mosaic_stairs");

        // Register Spanish aliases and semantic shortcuts
        BLOCK_DICT.put("piedra", "minecraft:stone");
        BLOCK_DICT.put("roca", "minecraft:stone");
        BLOCK_DICT.put("adoquin", "minecraft:cobblestone");
        BLOCK_DICT.put("adoquín", "minecraft:cobblestone");
        BLOCK_DICT.put("pizarra", "minecraft:deepslate");
        BLOCK_DICT.put("pizarra profunda", "minecraft:deepslate");
        BLOCK_DICT.put("pizarra labrada", "minecraft:cobbled_deepslate");
        BLOCK_DICT.put("granito", "minecraft:granite");
        BLOCK_DICT.put("diorita", "minecraft:diorite");
        BLOCK_DICT.put("andesita", "minecraft:andesite");
        BLOCK_DICT.put("toba", "minecraft:tuff");
        BLOCK_DICT.put("calcita", "minecraft:calcite");
        BLOCK_DICT.put("arenisca", "minecraft:sandstone");
        BLOCK_DICT.put("amatista", "minecraft:amethyst_block");
        BLOCK_DICT.put("basalto", "minecraft:blackstone");
        BLOCK_DICT.put("piedra negra", "minecraft:blackstone");
        BLOCK_DICT.put("ladrillo", "minecraft:bricks");
        BLOCK_DICT.put("ladrillos", "minecraft:bricks");

        // Ores
        BLOCK_DICT.put("carbon", "minecraft:coal_ore");
        BLOCK_DICT.put("carbón", "minecraft:coal_ore");
        BLOCK_DICT.put("hierro", "minecraft:iron_ore");
        BLOCK_DICT.put("cobre", "minecraft:copper_ore");
        BLOCK_DICT.put("oro", "minecraft:gold_ore");
        BLOCK_DICT.put("diamante", "minecraft:diamond_ore");
        BLOCK_DICT.put("esmeralda", "minecraft:emerald_ore");
        BLOCK_DICT.put("redstone", "minecraft:redstone_ore");
        BLOCK_DICT.put("lapis", "minecraft:lapis_ore");
        BLOCK_DICT.put("lapiz", "minecraft:lapis_ore");
        BLOCK_DICT.put("lapislazuli", "minecraft:lapis_ore");
        BLOCK_DICT.put("lapislázuli", "minecraft:lapis_ore");
        BLOCK_DICT.put("netherita", "minecraft:ancient_debris");
        BLOCK_DICT.put("escombros", "minecraft:ancient_debris");

        // Woods & Planks
        BLOCK_DICT.put("madera", "minecraft:oak_log");
        BLOCK_DICT.put("arbol", "minecraft:oak_log");
        BLOCK_DICT.put("árbol", "minecraft:oak_log");
        BLOCK_DICT.put("tronco", "minecraft:oak_log");
        BLOCK_DICT.put("roble", "minecraft:oak_log");
        BLOCK_DICT.put("tablon", "minecraft:oak_planks");
        BLOCK_DICT.put("tablón", "minecraft:oak_planks");
        BLOCK_DICT.put("tablones", "minecraft:oak_planks");
        BLOCK_DICT.put("abeto", "minecraft:spruce_log");
        BLOCK_DICT.put("abedul", "minecraft:birch_log");
        BLOCK_DICT.put("jungla", "minecraft:jungle_log");
        BLOCK_DICT.put("acacia", "minecraft:acacia_log");
        BLOCK_DICT.put("roble oscuro", "minecraft:dark_oak_log");
        BLOCK_DICT.put("manglar", "minecraft:mangrove_log");
        BLOCK_DICT.put("bambu", "minecraft:bamboo_block");
        BLOCK_DICT.put("bambú", "minecraft:bamboo_block");
        BLOCK_DICT.put("carmesi", "minecraft:crimson_stem");
        BLOCK_DICT.put("carmesí", "minecraft:crimson_stem");
        BLOCK_DICT.put("distorsionado", "minecraft:warped_stem");

        // Infrastructure & Storage
        BLOCK_DICT.put("antorcha", "minecraft:torch");
        BLOCK_DICT.put("farol", "minecraft:lantern");
        BLOCK_DICT.put("linterna", "minecraft:lantern");
        BLOCK_DICT.put("hoguera", "minecraft:campfire");
        BLOCK_DICT.put("cofre", "minecraft:barrel"); // Default container
        BLOCK_DICT.put("barril", "minecraft:barrel");
        BLOCK_DICT.put("mesa de crafteo", "minecraft:crafting_table");
        BLOCK_DICT.put("mesa de trabajo", "minecraft:crafting_table");
        BLOCK_DICT.put("cama", "minecraft:red_bed");
        BLOCK_DICT.put("terracota", "minecraft:terracotta");
        BLOCK_DICT.put("cristal", "minecraft:glass");
        BLOCK_DICT.put("vidrio", "minecraft:glass");
        BLOCK_DICT.put("tierra", "minecraft:dirt");
        BLOCK_DICT.put("arena", "minecraft:sand");
        BLOCK_DICT.put("grava", "minecraft:gravel");
    }

    /**
     * Resolves human/LLM block names to canonical Minecraft resource identifiers.
     */
    public static String resolveBlockName(String name, String fallback) {
        if (name == null || name.trim().isEmpty()) return fallback;
        String clean = name.trim().toLowerCase().replace("minecraft:", "").replace(' ', '_');
        if (BLOCK_DICT.containsKey(clean)) {
            return BLOCK_DICT.get(clean);
        }
        // A real block id wins over any alias ("oak_stairs" must not become "air").
        Identifier exact = Identifier.tryParse("minecraft:" + clean);
        if (exact != null && BuiltInRegistries.BLOCK.containsKey(exact)) {
            return exact.toString();
        }
        // Else the longest alias inside the name ("bloque de piedra" → piedra).
        String best = null;
        int bestLen = 0;
        for (Map.Entry<String, String> entry : BLOCK_DICT.entrySet()) {
            String key = entry.getKey();
            if (key.length() >= 4 && !key.equals("air") && key.length() > bestLen && clean.contains(key)) {
                best = entry.getValue();
                bestLen = key.length();
            }
        }
        if (best != null) return best;
        return name.contains(":") ? name : "minecraft:" + clean;
    }

    public static boolean consumeItemOrTag(LivingEntity entity, ServerPlayer player, Item item, TagKey<Item> tag) {
        if (player != null && player.isCreative()) return true;

        for (int i = 0; i < companionInventory.getContainerSize(); i++) {
            ItemStack stack = companionInventory.getItem(i);
            if (!stack.isEmpty() && ((item != null && stack.is(item)) || (tag != null && stack.is(tag)))) {
                stack.shrink(1);
                return true;
            }
        }

        if (player != null) {
            for (int i = 0; i < player.getInventory().getContainerSize(); i++) {
                ItemStack stack = player.getInventory().getItem(i);
                if (!stack.isEmpty() && ((item != null && stack.is(item)) || (tag != null && stack.is(tag)))) {
                    stack.shrink(1);
                    return true;
                }
            }
        }
        return false;
    }

    public static void updateEquippedGear(LivingEntity entity, KaltsitMod.CompanionState state) {
        if (entity == null || entity.level().isClientSide()) return;
        // Eating or drinking: the hand keeps the food until she finishes.
        if (entity.isUsingItem()) return;

        // Auto-equip armor
        for (int i = 0; i < companionInventory.getContainerSize(); i++) {
            ItemStack stack = companionInventory.getItem(i);
            if (stack.isEmpty()) continue;
            EquipmentSlot slot = entity.getEquipmentSlotForItem(stack);
            if (slot.isArmor() && entity.getItemBySlot(slot).isEmpty()) {
                entity.setItemSlot(slot, stack.split(1));
            }
        }

        // Auto-equip shield in offhand
        if (!entity.getItemBySlot(EquipmentSlot.OFFHAND).is(Items.SHIELD)) {
            for (int i = 0; i < companionInventory.getContainerSize(); i++) {
                ItemStack stack = companionInventory.getItem(i);
                if (!stack.isEmpty() && stack.is(Items.SHIELD)) {
                    entity.setItemSlot(EquipmentSlot.OFFHAND, stack.split(1));
                    break;
                }
            }
        }

        ItemStack bestItem = ItemStack.EMPTY;
        int bestScore = -1;

        if (state == KaltsitMod.CompanionState.COMBAT || state == KaltsitMod.CompanionState.GUARDING) {
            for (int i = 0; i < companionInventory.getContainerSize(); i++) {
                ItemStack stack = companionInventory.getItem(i);
                if (stack.isEmpty()) continue;
                int score = getWeaponScore(stack.getItem());
                if (score > bestScore) {
                    bestScore = score;
                    bestItem = stack;
                }
            }
        } else if (state == KaltsitMod.CompanionState.CHOPPING) {
            for (int i = 0; i < companionInventory.getContainerSize(); i++) {
                ItemStack stack = companionInventory.getItem(i);
                if (stack.isEmpty()) continue;
                int score = getAxeScore(stack.getItem());
                if (score > bestScore) {
                    bestScore = score;
                    bestItem = stack;
                }
            }
        } else if (state == KaltsitMod.CompanionState.MINING) {
            for (int i = 0; i < companionInventory.getContainerSize(); i++) {
                ItemStack stack = companionInventory.getItem(i);
                if (stack.isEmpty()) continue;
                int score = getPickaxeScore(stack.getItem());
                if (score > bestScore) {
                    bestScore = score;
                    bestItem = stack;
                }
            }
        }

        if (bestItem != null && !bestItem.isEmpty()) {
            entity.setItemSlot(EquipmentSlot.MAINHAND, bestItem);
        } else {
            if (state == KaltsitMod.CompanionState.FOLLOWING || state == KaltsitMod.CompanionState.IDLE) {
                entity.setItemSlot(EquipmentSlot.MAINHAND, ItemStack.EMPTY);
            }
        }
    }

    private static int getWeaponScore(Item item) {
        if (item == Items.NETHERITE_SWORD) return 100;
        if (item == Items.DIAMOND_SWORD) return 90;
        if (item == Items.NETHERITE_AXE) return 85;
        if (item == Items.DIAMOND_AXE) return 80;
        if (item == Items.IRON_SWORD) return 70;
        if (item == Items.IRON_AXE) return 65;
        if (item == Items.STONE_SWORD) return 50;
        if (item == Items.STONE_AXE) return 45;
        if (item == Items.GOLDEN_SWORD) return 40;
        if (item == Items.WOODEN_SWORD) return 30;
        if (item == Items.WOODEN_AXE) return 25;
        if (item == Items.TRIDENT) return 85;
        if (item == Items.BOW || item == Items.CROSSBOW) return 60;
        return 0;
    }

    private static int getAxeScore(Item item) {
        if (item == Items.NETHERITE_AXE) return 60;
        if (item == Items.DIAMOND_AXE) return 50;
        if (item == Items.IRON_AXE) return 40;
        if (item == Items.STONE_AXE) return 30;
        if (item == Items.WOODEN_AXE) return 20;
        if (item == Items.GOLDEN_AXE) return 15;
        return 0;
    }

    private static int getPickaxeScore(Item item) {
        if (item == Items.NETHERITE_PICKAXE) return 60;
        if (item == Items.DIAMOND_PICKAXE) return 50;
        if (item == Items.IRON_PICKAXE) return 40;
        if (item == Items.STONE_PICKAXE) return 30;
        if (item == Items.WOODEN_PICKAXE) return 20;
        if (item == Items.GOLDEN_PICKAXE) return 15;
        return 0;
    }

    public static BlockPos findWalkableStandingPos(Level level, BlockPos target, BlockPos entityPos) {
        if (level == null || target == null) return null;

        BlockPos best = null;
        double bestDist = Double.MAX_VALUE;

        for (int dx = -2; dx <= 2; dx++) {
            for (int dz = -2; dz <= 2; dz++) {
                if (dx == 0 && dz == 0) continue;
                for (int dy = -1; dy <= 2; dy++) {
                    BlockPos p = target.offset(dx, dy, dz);
                    if (isWalkable(level, p)) {
                        double reachDistSqr = p.distSqr(target);
                        if (reachDistSqr <= 10.0) {
                            double distToEntity = (entityPos != null) ? p.distSqr(entityPos) : reachDistSqr;
                            if (distToEntity < bestDist) {
                                bestDist = distToEntity;
                                best = p;
                            }
                        }
                    }
                }
            }
        }

        BlockPos above = target.above();
        if (isWalkable(level, above)) {
            double distToEntity = (entityPos != null) ? above.distSqr(entityPos) : above.distSqr(target);
            if (distToEntity < bestDist) {
                best = above;
            }
        }
        return best;
    }

    public static boolean isWalkable(Level level, BlockPos pos) {
        if (level == null || pos == null) return false;
        BlockState feet = level.getBlockState(pos);
        if (feet.isSolid() || feet.liquid() || feet.is(Blocks.LAVA) || feet.is(Blocks.FIRE)) {
            return false;
        }
        BlockState head = level.getBlockState(pos.above());
        if (head.isSolid() || head.liquid() || head.is(Blocks.LAVA) || head.is(Blocks.FIRE)) {
            return false;
        }
        BlockPos below = pos.below();
        BlockState ground = level.getBlockState(below);
        return ground.isSolid() && !ground.liquid() && !ground.is(Blocks.LAVA) && !ground.is(Blocks.FIRE);
    }

    public static BlockPos findNearbyBlock(Level level, BlockPos origin, Block targetBlock, int radius) {
        if (level == null || origin == null || targetBlock == null) return null;
        BlockPos best = null;
        double bestDist = Double.MAX_VALUE;
        for (int dx = -radius; dx <= radius; dx++) {
            for (int dz = -radius; dz <= radius; dz++) {
                for (int dy = -radius / 2; dy <= radius / 2; dy++) {
                    BlockPos p = origin.offset(dx, dy, dz);
                    if (level.getBlockState(p).is(targetBlock)) {
                        boolean accessible = false;
                        for (Direction dir : Direction.values()) {
                            BlockPos adj = p.relative(dir);
                            if (!level.getBlockState(adj).isSolid()) {
                                accessible = true;
                                break;
                            }
                        }
                        if (accessible) {
                            double d = p.distSqr(origin);
                            if (d < bestDist) {
                                bestDist = d;
                                best = p;
                            }
                        }
                    }
                }
            }
        }
        return best;
    }

    public static BlockPos getMiningTarget(Level level, ServerPlayer player, LivingEntity entity, String preferredBlock) {
        if (level == null) return null;
        BlockPos origin = (player != null) ? player.blockPosition() : (entity != null ? entity.blockPosition() : null);

        // 1. If preferred block requested, search nearby
        if (preferredBlock != null && !preferredBlock.trim().isEmpty() && origin != null) {
            String canonical = resolveBlockName(preferredBlock, "");
            if (!canonical.isEmpty()) {
                Identifier id = Identifier.tryParse(canonical);
                if (id != null) {
                    Block b = BuiltInRegistries.BLOCK.getValue(id);
                    if (b != null && b != Blocks.AIR) {
                        BlockPos found = findNearbyBlock(level, origin, b, 12);
                        if (found != null) return found;
                    }
                }
            }
        }

        // 2. Crosshair raycast from player
        if (player != null) {
            HitResult hit = player.pick(7.0, 0, false);
            if (hit instanceof BlockHitResult bhr) {
                BlockPos p = bhr.getBlockPos();
                if (!level.getBlockState(p).isAir() && !level.getBlockState(p).liquid()) {
                    return p;
                }
            }
        }

        // 3. Entity gaze / front block
        if (entity != null) {
            HitResult hit = entity.pick(4.5, 0, false);
            if (hit instanceof BlockHitResult bhr) {
                BlockPos p = bhr.getBlockPos();
                if (!level.getBlockState(p).isAir() && !level.getBlockState(p).liquid()) {
                    return p;
                }
            }
            Direction dir = entity.getDirection();
            BlockPos front = entity.blockPosition().relative(dir);
            if (!level.getBlockState(front).isAir() && !level.getBlockState(front).liquid()) return front;
            if (!level.getBlockState(front.above()).isAir() && !level.getBlockState(front.above()).liquid()) return front.above();
            if (!level.getBlockState(front.below()).isAir() && !level.getBlockState(front.below()).liquid()) return front.below();
        }
        return null;
    }

    public static BlockPos findAdjacentMatchingBlock(Level level, BlockPos origin, Block targetBlock) {
        if (level == null || origin == null || targetBlock == null) return null;
        for (Direction dir : Direction.values()) {
            BlockPos p = origin.relative(dir);
            if (level.getBlockState(p).is(targetBlock)) {
                return p;
            }
        }
        return null;
    }

    public static int storeItemsInNearbyChest(Level level, LivingEntity entity, ServerPlayer player) {
        if (level == null || entity == null) return 0;
        BlockPos origin = (player != null) ? player.blockPosition() : entity.blockPosition();

        Container chest = null;
        BlockPos chestPos = null;

        for (int dx = -5; dx <= 5; dx++) {
            for (int dz = -5; dz <= 5; dz++) {
                for (int dy = -2; dy <= 2; dy++) {
                    BlockPos p = origin.offset(dx, dy, dz);
                    BlockEntity be = level.getBlockEntity(p);
                    if (be instanceof Container cont) {
                        chest = cont;
                        chestPos = p;
                        break;
                    }
                }
                if (chest != null) break;
            }
            if (chest != null) break;
        }

        if (chest == null) return 0;

        int transferredStacks = 0;
        for (int i = 0; i < companionInventory.getContainerSize(); i++) {
            ItemStack stack = companionInventory.getItem(i);
            if (stack.isEmpty()) continue;
            Item item = stack.getItem();
            String id = BuiltInRegistries.ITEM.getKey(item).toString();

            boolean isToolOrArmor = id.contains("sword") || id.contains("axe") || id.contains("pickaxe") ||
                    id.contains("shovel") || id.contains("hoe") || id.contains("helmet") ||
                    id.contains("chestplate") || id.contains("leggings") || id.contains("boots") ||
                    id.contains("shield") || id.contains("bow") || id.contains("torch") ||
                    item == Items.BREAD || item == Items.COOKED_BEEF || item == Items.APPLE || item == Items.GOLDEN_APPLE;

            if (!isToolOrArmor) {
                for (int c = 0; c < chest.getContainerSize(); c++) {
                    ItemStack chestStack = chest.getItem(c);
                    if (chestStack.isEmpty()) {
                        chest.setItem(c, stack.copy());
                        companionInventory.setItem(i, ItemStack.EMPTY);
                        transferredStacks++;
                        break;
                    } else if (ItemStack.isSameItemSameComponents(chestStack, stack) && chestStack.getCount() < chestStack.getMaxStackSize()) {
                        int space = chestStack.getMaxStackSize() - chestStack.getCount();
                        int move = Math.min(space, stack.getCount());
                        chestStack.grow(move);
                        stack.shrink(move);
                        if (stack.isEmpty()) {
                            companionInventory.setItem(i, ItemStack.EMPTY);
                            transferredStacks++;
                            break;
                        }
                    }
                }
            }
        }

        if (transferredStacks > 0) {
            entity.swing(InteractionHand.MAIN_HAND);
            level.playSound(null, chestPos, SoundEvents.CHEST_CLOSE, SoundSource.BLOCKS, 0.7f, 1.0f);
            chest.setChanged();
        }
        return transferredStacks;
    }

    public static int takeItemsFromNearbyChest(Level level, LivingEntity entity, ServerPlayer player, String blockName, int targetCount) {
        if (level == null || entity == null) return 0;
        BlockPos origin = (player != null) ? player.blockPosition() : entity.blockPosition();

        Container chest = null;
        BlockPos chestPos = null;

        for (int dx = -5; dx <= 5; dx++) {
            for (int dz = -5; dz <= 5; dz++) {
                for (int dy = -2; dy <= 2; dy++) {
                    BlockPos p = origin.offset(dx, dy, dz);
                    BlockEntity be = level.getBlockEntity(p);
                    if (be instanceof Container cont) {
                        chest = cont;
                        chestPos = p;
                        break;
                    }
                }
                if (chest != null) break;
            }
            if (chest != null) break;
        }

        if (chest == null) return 0;

        String targetCanonical = resolveBlockName(blockName, "minecraft:oak_planks");
        int gathered = 0;

        for (int c = 0; c < chest.getContainerSize(); c++) {
            ItemStack chestStack = chest.getItem(c);
            if (chestStack.isEmpty()) continue;
            String id = BuiltInRegistries.ITEM.getKey(chestStack.getItem()).toString();
            if (id.equals(targetCanonical) || id.contains(blockName.toLowerCase())) {
                int take = Math.min(chestStack.getCount(), targetCount - gathered);
                ItemStack split = chestStack.split(take);
                companionInventory.addItem(split);
                gathered += take;
                if (gathered >= targetCount) break;
            }
        }

        if (gathered > 0) {
            entity.swing(InteractionHand.MAIN_HAND);
            level.playSound(null, chestPos, SoundEvents.CHEST_OPEN, SoundSource.BLOCKS, 0.7f, 1.0f);
            chest.setChanged();
        }
        return gathered;
    }

    public static boolean autoEatAndHeal(LivingEntity entity) {
        if (entity == null || entity.getHealth() >= 32.0f) return false;
        for (int i = 0; i < companionInventory.getContainerSize(); i++) {
            ItemStack stack = companionInventory.getItem(i);
            if (stack.isEmpty()) continue;
            Item item = stack.getItem();
            boolean isFood = (item == Items.COOKED_BEEF || item == Items.COOKED_PORKCHOP ||
                    item == Items.COOKED_MUTTON || item == Items.COOKED_CHICKEN ||
                    item == Items.BREAD || item == Items.APPLE || item == Items.GOLDEN_APPLE ||
                    item == Items.BAKED_POTATO || item == Items.CARROT);
            if (isFood) {
                stack.shrink(1);
                float heal = (item == Items.GOLDEN_APPLE) ? 16.0f : 8.0f;
                entity.heal(heal);
                entity.swing(InteractionHand.MAIN_HAND);
                return true;
            }
        }
        return false;
    }

    public static boolean autoPlaceTorchInDarkness(Level level, LivingEntity entity, ServerPlayer player) {
        if (level == null || entity == null || player == null || !KaltsitSettings.get().autoTorches) return false;
        BlockPos pPos = player.blockPosition();
        if (!KaltsitSettings.torchAllowed(level, pPos)) return false;
        int light = level.getBrightness(LightLayer.BLOCK, pPos);
        if (light > 3) return false;

        if (!consumeItemOrTag(entity, player, Items.TORCH, null)) return false;

        for (int dx = -1; dx <= 1; dx++) {
            for (int dz = -1; dz <= 1; dz++) {
                BlockPos target = pPos.offset(dx, 0, dz);
                if (level.getBlockState(target).isAir() && level.getBlockState(target.below()).isSolid()) {
                    entity.swing(InteractionHand.MAIN_HAND);
                    level.setBlock(target, Blocks.TORCH.defaultBlockState(), 3);
                    level.playSound(null, target, SoundEvents.WOOD_PLACE, SoundSource.BLOCKS, 0.8f, 1.0f);
                    return true;
                }
            }
        }
        companionInventory.addItem(new ItemStack(Items.TORCH, 1));
        return false;
    }

    public static boolean placeBed(Level level, LivingEntity entity, ServerPlayer player) {
        if (level == null || entity == null) return false;

        if (!consumeItemOrTag(entity, player, null, ItemTags.BEDS)) {
            if (player != null) {
                player.sendSystemMessage(Component.literal("§c[Kal'tsit]§r No disponemos de una cama en el inventario para colocarla."));
            }
            return false;
        }

        BlockPos origin = (player != null) ? player.blockPosition() : entity.blockPosition();

        for (Direction dir : Direction.Plane.HORIZONTAL) {
            for (int dx = -2; dx <= 2; dx++) {
                for (int dz = -2; dz <= 2; dz++) {
                    BlockPos foot = origin.offset(dx, 0, dz);
                    BlockPos head = foot.relative(dir);

                    boolean footBaseSolid = level.getBlockState(foot.below()).isSolid();
                    boolean headBaseSolid = level.getBlockState(head.below()).isSolid();
                    boolean footClear = level.getBlockState(foot).isAir();
                    boolean headClear = level.getBlockState(head).isAir();

                    if (footBaseSolid && headBaseSolid && footClear && headClear) {
                        entity.swing(InteractionHand.MAIN_HAND);
                        BlockState footState = Blocks.BED.red().defaultBlockState()
                                .setValue(BedBlock.PART, BedPart.FOOT)
                                .setValue(BedBlock.FACING, dir);
                        BlockState headState = Blocks.BED.red().defaultBlockState()
                                .setValue(BedBlock.PART, BedPart.HEAD)
                                .setValue(BedBlock.FACING, dir);

                        boolean s1 = level.setBlock(foot, footState, 3);
                        boolean s2 = level.setBlock(head, headState, 3);
                        // The bed she places is hers: her respawn point in survival.
                        if (s1 && s2) KaltsitSettings.setBed(level, head);
                        return s1 && s2;
                    }
                }
            }
        }

        companionInventory.addItem(new ItemStack(Items.BED.red(), 1));
        if (player != null) {
            player.sendSystemMessage(Component.literal("§c[Kal'tsit]§r Espacio insuficiente o terreno irregular para colocar la cama."));
        }
        return false;
    }

    public static boolean placeCraftingTable(Level level, LivingEntity entity, ServerPlayer player) {
        if (level == null || entity == null) return false;

        if (!consumeItemOrTag(entity, player, Items.CRAFTING_TABLE, null)) {
            if (player != null) {
                player.sendSystemMessage(Component.literal("§c[Kal'tsit]§r No tenemos una mesa de trabajo en el inventario."));
            }
            return false;
        }

        BlockPos origin = (player != null) ? player.blockPosition() : entity.blockPosition();
        for (int dx = -2; dx <= 2; dx++) {
            for (int dz = -2; dz <= 2; dz++) {
                BlockPos target = origin.offset(dx, 0, dz);
                if (level.getBlockState(target).isAir() && level.getBlockState(target.below()).isSolid()) {
                    entity.swing(InteractionHand.MAIN_HAND);
                    return level.setBlock(target, Blocks.CRAFTING_TABLE.defaultBlockState(), 3);
                }
            }
        }
        companionInventory.addItem(new ItemStack(Items.CRAFTING_TABLE, 1));
        return false;
    }

    public static boolean placeTorch(Level level, LivingEntity entity, ServerPlayer player) {
        if (level == null || entity == null) return false;

        if (!consumeItemOrTag(entity, player, Items.TORCH, null)) {
            if (player != null) {
                player.sendSystemMessage(Component.literal("§c[Kal'tsit]§r Se agotaron las antorchas en el inventario."));
            }
            return false;
        }

        BlockPos origin = (player != null) ? player.blockPosition() : entity.blockPosition();
        for (int dx = -1; dx <= 1; dx++) {
            for (int dz = -1; dz <= 1; dz++) {
                BlockPos target = origin.offset(dx, 0, dz);
                if (level.getBlockState(target).isAir() && level.getBlockState(target.below()).isSolid()) {
                    entity.swing(InteractionHand.MAIN_HAND);
                    return level.setBlock(target, Blocks.TORCH.defaultBlockState(), 3);
                }
            }
        }
        companionInventory.addItem(new ItemStack(Items.TORCH, 1));
        return false;
    }

    // ── acting like a player ──

    /** A player's reach for blocks, from the eyes (4.5 blocks). */
    public static boolean canReach(LivingEntity entity, BlockPos pos) {
        return entity.getEyePosition().distanceToSqr(Vec3.atCenterOf(pos)) <= 4.5 * 4.5;
    }

    /**
     * Ticks a player holding what she holds needs to break the block: tool speed over hardness,
     * three times slower without the right tool, five times slower in water or in the air.
     * -1 for an unbreakable block.
     */
    public static int breakTicks(LivingEntity entity, Level level, BlockPos pos) {
        BlockState state = level.getBlockState(pos);
        float hardness = state.getDestroySpeed(level, pos);
        if (hardness < 0) return -1;
        if (hardness == 0) return 1;
        ItemStack tool = entity.getMainHandItem();
        float speed = Math.max(1.0f, tool.getDestroySpeed(state));
        if (entity.isEyeInFluid(net.minecraft.tags.FluidTags.WATER)) speed /= 5.0f;
        if (!entity.onGround() && !entity.isInWater()) speed /= 5.0f;
        boolean correct = !state.requiresCorrectToolForDrops() || tool.isCorrectToolForDrops(state);
        float perTick = speed / hardness / (correct ? 30.0f : 100.0f);
        return Math.min(600, (int) Math.ceil(1.0f / perTick));
    }

    /**
     * Breaks a block the way a player does: the loot of the tool in her hand (fortune, silk
     * touch), nothing from a block that needs a better tool, and one point of wear on the tool.
     */
    public static void breakBlockLikePlayer(ServerLevel level, LivingEntity entity, BlockPos pos) {
        BlockState state = level.getBlockState(pos);
        if (state.isAir()) return;
        ItemStack tool = entity.getMainHandItem();
        boolean drops = !state.requiresCorrectToolForDrops() || tool.isCorrectToolForDrops(state);
        List<ItemStack> loot = drops ? Block.getDrops(state, level, pos, level.getBlockEntity(pos), entity, tool) : List.of();
        float hardness = state.getDestroySpeed(level, pos);
        level.destroyBlock(pos, false, entity);
        for (ItemStack s : loot) {
            Block.popResource(level, pos, s);
        }
        if (hardness > 0 && !tool.isEmpty() && tool.isDamageableItem()) {
            tool.hurtAndBreak(1, entity, EquipmentSlot.MAINHAND);
        }
    }

    /** Food she never takes from the Doctor: precious, or bad for her. */
    private static boolean unsafeFood(ItemStack s) {
        return s.is(Items.GOLDEN_APPLE) || s.is(Items.ENCHANTED_GOLDEN_APPLE) || s.is(Items.GOLDEN_CARROT) || s.is(Items.ROTTEN_FLESH)
            || s.is(Items.SPIDER_EYE) || s.is(Items.POISONOUS_POTATO) || s.is(Items.PUFFERFISH) || s.is(Items.SUSPICIOUS_STEW)
            || s.is(Items.CHORUS_FRUIT) || s.is(Items.CHICKEN);
    }

    /**
     * When she has nothing to eat: one piece of the Doctor's most plentiful everyday food goes
     * into her inventory, and he is told. False when he has none to spare.
     */
    public static boolean takeFoodFromDoctor(ServerPlayer doctor) {
        if (doctor == null) return false;
        var inv = doctor.getInventory();
        Map<Item, Integer> amounts = new HashMap<>();
        for (int i = 0; i < inv.getContainerSize(); i++) {
            ItemStack s = inv.getItem(i);
            if (s.isEmpty() || unsafeFood(s) || !s.has(net.minecraft.core.component.DataComponents.FOOD) || !s.has(net.minecraft.core.component.DataComponents.CONSUMABLE)) continue;
            amounts.merge(s.getItem(), s.getCount(), Integer::sum);
        }
        Item pick = amounts.entrySet().stream().max(Map.Entry.comparingByValue()).map(Map.Entry::getKey).orElse(null);
        if (pick == null || !companionInventory.canAddItem(new ItemStack(pick))) return false;
        for (int i = 0; i < inv.getContainerSize(); i++) {
            ItemStack s = inv.getItem(i);
            if (s.is(pick)) {
                companionInventory.addItem(s.copyWithCount(1));
                if (!doctor.isCreative()) s.shrink(1);
                inv.setChanged();
                doctor.sendSystemMessage(Component.literal("§b[Kal'tsit]§r Estoy herida y sin comida: tomé 1 ").append(new ItemStack(pick).getHoverName()).append(" de tu inventario."), false);
                return true;
            }
        }
        return false;
    }

    /** Something lying on the ground she can carry, the nearest within `radius`. */
    public static ItemEntity nearestDrop(LivingEntity entity, double radius) {
        return entity.level().getEntitiesOfClass(ItemEntity.class, entity.getBoundingBox().inflate(radius, 3.0, radius),
                e -> e.isAlive() && !e.hasPickUpDelay() && companionInventory.canAddItem(e.getItem()))
            .stream().min(Comparator.comparingDouble(entity::distanceToSqr)).orElse(null);
    }

    /** Whether `mob` is a danger to her or the Doctor right now. */
    public static boolean isThreat(LivingEntity mob, LivingEntity self, ServerPlayer doctor) {
        if (mob == self || mob instanceof KaltsitEntity || !mob.isAlive()) return false;
        LivingEntity target = mob instanceof Mob m ? m.getTarget() : null;
        if (target != null && (target == self || target == doctor)) return true;
        if (doctor != null && doctor.getLastHurtByMob() == mob) return true;
        if (self.getLastHurtByMob() == mob) return true;
        // Endermen, zombified piglins…: only when they are angry.
        if (mob instanceof NeutralMob neutral) return neutral.isAngry();
        return mob instanceof Enemy;
    }

    /** The nearest danger she can see within `range`. */
    public static LivingEntity findThreat(LivingEntity self, ServerPlayer doctor, double range) {
        return self.level().getEntitiesOfClass(LivingEntity.class, self.getBoundingBox().inflate(range, 4.0, range),
                e -> isThreat(e, self, doctor) && self.hasLineOfSight(e))
            .stream().min(Comparator.comparingDouble(self::distanceToSqr)).orElse(null);
    }

    /** A creeper about to blow up within `range`. */
    public static Creeper swellingCreeper(LivingEntity self, double range) {
        return self.level().getEntitiesOfClass(Creeper.class, self.getBoundingBox().inflate(range),
                c -> c.isAlive() && c.getSwellDir() > 0)
            .stream().min(Comparator.comparingDouble(self::distanceToSqr)).orElse(null);
    }

    /** One swing, when her weapon is ready (cooldown of a player with that weapon). */
    public static boolean attackLikePlayer(KaltsitEntity self, LivingEntity target) {
        if (self.attackCooldown > 0 || self.isUsingItem()) return false;
        if (self.distanceTo(target) > 3.0f) return false;
        self.getLookControl().setLookAt(target, 30.0f, 30.0f);
        self.swing(InteractionHand.MAIN_HAND);
        self.doHurtTarget((ServerLevel) self.level(), target);
        self.attackCooldown = self.attackInterval();
        return true;
    }

    /** The nearest ground to stand on with no lava or fire, within `r`. */
    public static BlockPos safeGround(Level level, BlockPos from, int r) {
        BlockPos best = null;
        double bestD = Double.MAX_VALUE;
        for (int dx = -r; dx <= r; dx++)
            for (int dz = -r; dz <= r; dz++)
                for (int dy = -3; dy <= 3; dy++) {
                    BlockPos p = from.offset(dx, dy, dz);
                    if (!isWalkable(level, p) || !level.getFluidState(p).isEmpty()) continue;
                    boolean hot = false;
                    for (Direction d : Direction.values()) {
                        BlockState n = level.getBlockState(p.relative(d));
                        if (n.is(Blocks.LAVA) || n.is(Blocks.FIRE) || n.is(Blocks.SOUL_FIRE)) {
                            hot = true;
                            break;
                        }
                    }
                    if (hot) continue;
                    double dist = p.distSqr(from);
                    if (dist < bestD) {
                        bestD = dist;
                        best = p;
                    }
                }
        return best;
    }

    /** The nearest water within `r` (to put a fire out). */
    public static BlockPos nearbyWater(Level level, BlockPos from, int r) {
        BlockPos best = null;
        double bestD = Double.MAX_VALUE;
        for (int dx = -r; dx <= r; dx++)
            for (int dz = -r; dz <= r; dz++)
                for (int dy = -3; dy <= 2; dy++) {
                    BlockPos p = from.offset(dx, dy, dz);
                    if (!level.getFluidState(p).is(net.minecraft.tags.FluidTags.WATER)) continue;
                    double dist = p.distSqr(from);
                    if (dist < bestD) {
                        bestD = dist;
                        best = p;
                    }
                }
        return best;
    }

    /** A log of a tree that grew on its own: player houses (no natural leaves) are left alone. */
    public static boolean isNaturalTreeLog(Level level, BlockPos pos) {
        if (!level.getBlockState(pos).is(BlockTags.LOGS)) return false;
        BlockPos top = pos;
        for (int n = 0; n < 24 && level.getBlockState(top.above()).is(BlockTags.LOGS); n++) {
            top = top.above();
        }
        for (int dx = -2; dx <= 2; dx++) {
            for (int dy = -1; dy <= 2; dy++) {
                for (int dz = -2; dz <= 2; dz++) {
                    BlockState s = level.getBlockState(top.offset(dx, dy, dz));
                    if (s.is(BlockTags.LEAVES) && s.hasProperty(LeavesBlock.PERSISTENT) && !s.getValue(LeavesBlock.PERSISTENT)) {
                        return true;
                    }
                }
            }
        }
        return false;
    }

    public static void followPlayer(LivingEntity entity, ServerPlayer player, double speed, double stopDist) {
        if (!(entity instanceof Mob mob) || player == null) return;
        double dist = entity.distanceTo(player);
        if (dist >= 32.0 && player.onGround()) {
            // Too far behind (or stuck): she catches up at a free spot next to the Doctor.
            BlockPos base = player.blockPosition();
            for (int r = 1; r <= 3; r++) {
                for (int dx = -r; dx <= r; dx++) {
                    for (int dz = -r; dz <= r; dz++) {
                        BlockPos p = base.offset(dx, 0, dz);
                        if (isWalkable(entity.level(), p)) {
                            mob.getNavigation().stop();
                            entity.teleportTo(p.getX() + 0.5, p.getY(), p.getZ() + 0.5);
                            return;
                        }
                    }
                }
            }
        } else if (dist > stopDist) {
            // A new path twice a second (or when the old one ends), not every tick: in water a
            // path recomputed every tick keeps failing and she stops.
            if (mob.getNavigation().isDone() || entity.tickCount % 10 == 0) mob.getNavigation().moveTo(player, speed);
        } else if (mob.getNavigation().isInProgress()) {
            mob.getNavigation().stop();
        }
    }
}
