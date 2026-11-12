//! Which flags guard a click on a block, an item use or an entity, by vanilla id
//! (`minecraft:chest`). Ids may come with or without the namespace.
//!
//! Block and item classification follows WorldGuard's `Materials` and `Entities` helpers.

fn path(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// The id without a `minecraft:` namespace.
pub fn path_of(id: &str) -> &str {
    path(id)
}

const MODIFIED_ON_RIGHT: u8 = 1;
const MODIFIED_ON_LEFT: u8 = 2;
const MODIFIES_BLOCKS: u8 = 4;

/// `Materials.MATERIAL_FLAGS` (sorted): 0 = does nothing when clicked or used, 1 = a click with
/// the right hand changes it, 2 = left hand, 4 = an item that changes blocks (bone meal).
static MATERIAL_FLAGS: &[(&str, u8)] = &[
    ("acacia_stairs", 0), ("activator_rail", 0), ("air", 0), ("amethyst_block", 0),
    ("amethyst_cluster", 0), ("amethyst_shard", 0), ("ancient_debris", 0), ("andesite", 0),
    ("anvil", 1), ("apple", 0), ("armadillo_scute", 0), ("armor_stand", 0), ("arrow", 0),
    ("attached_melon_stem", 0), ("attached_pumpkin_stem", 0), ("axolotl_bucket", 0), ("azalea", 0),
    ("baked_potato", 0), ("bamboo", 0), ("bamboo_block", 0), ("bamboo_mosaic", 0),
    ("bamboo_sapling", 0), ("barrel", 1), ("barrier", 0), ("basalt", 0), ("beacon", 1),
    ("bedrock", 0), ("bee_nest", 1), ("beef", 0), ("beehive", 1), ("beetroot", 0),
    ("beetroot_seeds", 0), ("beetroot_soup", 0), ("beetroots", 0), ("bell", 1),
    ("big_dripleaf", 0), ("big_dripleaf_stem", 0), ("black_dye", 0), ("black_stained_glass", 0),
    ("black_stained_glass_pane", 0), ("black_terracotta", 0), ("blackstone", 0),
    ("blast_furnace", 1), ("blaze_powder", 0), ("blaze_rod", 0), ("blue_dye", 0), ("blue_egg", 0),
    ("blue_ice", 0), ("blue_stained_glass", 0), ("blue_stained_glass_pane", 0),
    ("blue_terracotta", 0), ("bone", 0), ("bone_block", 0), ("bone_meal", 4), ("book", 0),
    ("bookshelf", 1), ("bordure_indented_banner_pattern", 0), ("bow", 0), ("bowl", 0),
    ("bread", 0), ("breeze_rod", 0), ("brewing_stand", 1), ("brick", 0), ("brick_stairs", 0),
    ("bricks", 0), ("brown_dye", 0), ("brown_egg", 0), ("brown_mushroom", 0),
    ("brown_mushroom_block", 0), ("brown_stained_glass", 0), ("brown_stained_glass_pane", 0),
    ("brown_terracotta", 0), ("brush", 0), ("bubble_column", 0), ("bucket", 0),
    ("budding_amethyst", 0), ("bundle", 0), ("bush", 0), ("cactus", 0), ("cactus_flower", 0),
    ("cake", 1), ("calcite", 0), ("calibrated_sculk_sensor", 0), ("campfire", 3), ("carrot", 0),
    ("carrot_on_a_stick", 0), ("carrots", 0), ("cartography_table", 0), ("carved_pumpkin", 0),
    ("cave_air", 0), ("cave_vines", 1), ("cave_vines_plant", 1), ("chain_command_block", 1),
    ("chainmail_boots", 0), ("chainmail_chestplate", 0), ("chainmail_helmet", 0),
    ("chainmail_leggings", 0), ("charcoal", 0), ("chest", 1), ("chest_minecart", 0),
    ("chicken", 0), ("chipped_anvil", 1), ("chiseled_bookshelf", 1), ("chiseled_cinnabar", 0),
    ("chiseled_copper", 0), ("chiseled_deepslate", 0), ("chiseled_nether_bricks", 0),
    ("chiseled_polished_blackstone", 0), ("chiseled_quartz_block", 0),
    ("chiseled_red_sandstone", 0), ("chiseled_resin_bricks", 0), ("chiseled_sandstone", 0),
    ("chiseled_stone_bricks", 0), ("chiseled_sulfur", 0), ("chiseled_tuff", 0),
    ("chiseled_tuff_bricks", 0), ("chorus_flower", 0), ("chorus_fruit", 0), ("chorus_plant", 0),
    ("cinnabar", 0), ("cinnabar_bricks", 0), ("clay", 0), ("clay_ball", 0), ("clock", 0),
    ("coal", 0), ("coal_block", 0), ("coarse_dirt", 0), ("cobbled_deepslate", 0),
    ("cobblestone", 0), ("cobblestone_stairs", 0), ("cobweb", 0), ("cocoa", 0), ("cocoa_beans", 0),
    ("cod", 0), ("cod_bucket", 0), ("command_block", 1), ("command_block_minecart", 0),
    ("comparator", 1), ("compass", 0), ("composter", 1), ("conduit", 0), ("cooked_beef", 0),
    ("cooked_chicken", 0), ("cooked_cod", 0), ("cooked_mutton", 0), ("cooked_porkchop", 0),
    ("cooked_rabbit", 0), ("cooked_salmon", 0), ("cookie", 0), ("copper_axe", 0),
    ("copper_block", 0), ("copper_boots", 0), ("copper_bulb", 0), ("copper_chestplate", 0),
    ("copper_grate", 0), ("copper_helmet", 0), ("copper_hoe", 0), ("copper_horse_armor", 0),
    ("copper_ingot", 0), ("copper_leggings", 0), ("copper_nautilus_armor", 0),
    ("copper_nugget", 0), ("copper_pickaxe", 0), ("copper_shovel", 0), ("copper_sword", 0),
    ("copper_torch", 0), ("copper_wall_torch", 0), ("cracked_deepslate_bricks", 0),
    ("cracked_deepslate_tiles", 0), ("cracked_nether_bricks", 0),
    ("cracked_polished_blackstone_bricks", 0), ("cracked_stone_bricks", 0), ("crafter", 0),
    ("crafting_table", 0), ("creaking_heart", 0), ("creeper_banner_pattern", 0),
    ("creeper_head", 0), ("creeper_wall_head", 0), ("crimson_fungus", 0), ("crimson_nylium", 0),
    ("crimson_roots", 0), ("crossbow", 0), ("crying_obsidian", 0), ("cut_copper", 0),
    ("cut_red_sandstone", 0), ("cut_sandstone", 0), ("cyan_dye", 0), ("cyan_stained_glass", 0),
    ("cyan_stained_glass_pane", 0), ("cyan_terracotta", 0), ("damaged_anvil", 1),
    ("dark_oak_stairs", 0), ("dark_prismarine", 0), ("daylight_detector", 1), ("dead_bush", 0),
    ("debug_stick", 0), ("decorated_pot", 1), ("deepslate", 0), ("deepslate_bricks", 0),
    ("deepslate_tiles", 0), ("detector_rail", 0), ("diamond", 0), ("diamond_axe", 0),
    ("diamond_block", 0), ("diamond_boots", 0), ("diamond_chestplate", 0), ("diamond_helmet", 0),
    ("diamond_hoe", 0), ("diamond_horse_armor", 0), ("diamond_leggings", 0),
    ("diamond_nautilus_armor", 0), ("diamond_pickaxe", 0), ("diamond_shovel", 0),
    ("diamond_sword", 0), ("diorite", 0), ("dirt", 0), ("dirt_path", 0), ("disc_fragment_5", 0),
    ("dispenser", 1), ("dragon_breath", 0), ("dragon_egg", 3), ("dragon_head", 0),
    ("dragon_wall_head", 0), ("dried_ghast", 0), ("dried_kelp", 0), ("dried_kelp_block", 0),
    ("dripstone_block", 0), ("dropper", 1), ("echo_shard", 0), ("egg", 0), ("elytra", 0),
    ("emerald", 0), ("emerald_block", 0), ("enchanted_book", 0), ("enchanted_golden_apple", 0),
    ("enchanting_table", 1), ("end_crystal", 0), ("end_gateway", 0), ("end_portal", 0),
    ("end_portal_frame", 0), ("end_rod", 0), ("end_stone", 0), ("end_stone_bricks", 0),
    ("ender_chest", 0), ("ender_eye", 0), ("ender_pearl", 0), ("experience_bottle", 0),
    ("exposed_chiseled_copper", 0), ("exposed_copper", 0), ("exposed_copper_bulb", 0),
    ("exposed_copper_grate", 0), ("exposed_cut_copper", 0), ("farmland", 0), ("feather", 0),
    ("fermented_spider_eye", 0), ("fern", 0), ("field_masoned_banner_pattern", 0),
    ("filled_map", 0), ("fire", 0), ("fire_charge", 0), ("firefly_bush", 0),
    ("firework_rocket", 0), ("firework_star", 0), ("fishing_rod", 0), ("fletching_table", 0),
    ("flint", 0), ("flint_and_steel", 0), ("flow_banner_pattern", 0), ("flower_banner_pattern", 0),
    ("flowering_azalea", 0), ("frogspawn", 0), ("frosted_ice", 0), ("furnace", 1),
    ("furnace_minecart", 0), ("ghast_tear", 0), ("gilded_blackstone", 0), ("glass", 0),
    ("glass_bottle", 0), ("glass_pane", 0), ("glistering_melon_slice", 0),
    ("globe_banner_pattern", 0), ("glow_berries", 0), ("glow_ink_sac", 0), ("glow_item_frame", 0),
    ("glow_lichen", 0), ("glowstone", 0), ("glowstone_dust", 0), ("goat_horn", 0),
    ("gold_block", 0), ("gold_ingot", 0), ("gold_nugget", 0), ("golden_apple", 0),
    ("golden_axe", 0), ("golden_boots", 0), ("golden_carrot", 0), ("golden_chestplate", 0),
    ("golden_dandelion", 0), ("golden_helmet", 0), ("golden_hoe", 0), ("golden_horse_armor", 0),
    ("golden_leggings", 0), ("golden_nautilus_armor", 0), ("golden_pickaxe", 0),
    ("golden_shovel", 0), ("golden_sword", 0), ("granite", 0), ("grass_block", 0), ("gravel", 0),
    ("gray_dye", 0), ("gray_stained_glass", 0), ("gray_stained_glass_pane", 0),
    ("gray_terracotta", 0), ("green_dye", 0), ("green_stained_glass", 0),
    ("green_stained_glass_pane", 0), ("green_terracotta", 0), ("grindstone", 0), ("gunpowder", 0),
    ("guster_banner_pattern", 0), ("hanging_roots", 0), ("hard_clay", 0), ("hay_block", 0),
    ("heart_of_the_sea", 0), ("heavy_core", 0), ("heavy_weighted_pressure_plate", 0),
    ("honey_block", 0), ("honey_bottle", 0), ("honeycomb", 0), ("honeycomb_block", 0),
    ("hopper", 1), ("hopper_minecart", 0), ("ice", 0), ("infested_chiseled_stone_bricks", 0),
    ("infested_cobblestone", 0), ("infested_cracked_stone_bricks", 0), ("infested_deepslate", 0),
    ("infested_mossy_stone_bricks", 0), ("infested_stone", 0), ("infested_stone_bricks", 0),
    ("ink_sac", 0), ("iron_axe", 0), ("iron_block", 0), ("iron_boots", 0), ("iron_chestplate", 0),
    ("iron_door", 0), ("iron_helmet", 0), ("iron_hoe", 0), ("iron_horse_armor", 0),
    ("iron_ingot", 0), ("iron_leggings", 0), ("iron_nautilus_armor", 0), ("iron_nugget", 0),
    ("iron_pickaxe", 0), ("iron_shovel", 0), ("iron_sword", 0), ("iron_trapdoor", 0),
    ("item_frame", 0), ("jack_o_lantern", 0), ("jigsaw", 3), ("jukebox", 1), ("kelp", 0),
    ("kelp_plant", 0), ("knowledge_book", 0), ("ladder", 0), ("lapis_block", 0),
    ("lapis_lazuli", 0), ("large_amethyst_bud", 0), ("large_fern", 0), ("lava", 0),
    ("lava_bucket", 0), ("lead", 0), ("leaf_litter", 0), ("leather", 0), ("leather_boots", 0),
    ("leather_chestplate", 0), ("leather_helmet", 0), ("leather_horse_armor", 0),
    ("leather_leggings", 0), ("lectern", 0), ("lever", 1), ("light", 1), ("light_blue_dye", 0),
    ("light_blue_stained_glass", 0), ("light_blue_stained_glass_pane", 0),
    ("light_blue_terracotta", 0), ("light_gray_dye", 0), ("light_gray_stained_glass", 0),
    ("light_gray_stained_glass_pane", 0), ("light_gray_terracotta", 0),
    ("light_weighted_pressure_plate", 0), ("lilac", 0), ("lily_pad", 0), ("lime_dye", 0),
    ("lime_stained_glass", 0), ("lime_stained_glass_pane", 0), ("lime_terracotta", 0),
    ("lingering_potion", 0), ("lodestone", 0), ("loom", 0), ("mace", 0), ("magenta_dye", 0),
    ("magenta_stained_glass", 0), ("magenta_stained_glass_pane", 0), ("magenta_terracotta", 0),
    ("magma_block", 0), ("magma_cream", 0), ("mangrove_roots", 0), ("map", 0),
    ("medium_amethyst_bud", 0), ("melon", 0), ("melon_seeds", 0), ("melon_slice", 0),
    ("melon_stem", 0), ("milk_bucket", 0), ("minecart", 0), ("mojang_banner_pattern", 0),
    ("moss_block", 0), ("moss_carpet", 0), ("mossy_cobblestone", 0), ("mossy_stone_bricks", 0),
    ("moving_piston", 0), ("mud", 0), ("mud_bricks", 0), ("muddy_mangrove_roots", 0),
    ("mushroom_stem", 0), ("mushroom_stew", 0), ("mutton", 0), ("mycelium", 0), ("name_tag", 0),
    ("nautilus_shell", 0), ("nether_brick", 0), ("nether_brick_stairs", 0), ("nether_bricks", 0),
    ("nether_portal", 0), ("nether_quartz_ore", 0), ("nether_sprouts", 0), ("nether_star", 0),
    ("nether_wart", 0), ("nether_wart_block", 0), ("netherite_axe", 0), ("netherite_block", 0),
    ("netherite_boots", 0), ("netherite_chestplate", 0), ("netherite_helmet", 0),
    ("netherite_hoe", 0), ("netherite_horse_armor", 0), ("netherite_ingot", 0),
    ("netherite_leggings", 0), ("netherite_nautilus_armor", 0), ("netherite_pickaxe", 0),
    ("netherite_scrap", 0), ("netherite_shovel", 0), ("netherite_sword", 0), ("netherrack", 0),
    ("note_block", 1), ("observer", 0), ("obsidian", 0), ("ochre_froglight", 0),
    ("ominous_bottle", 0), ("ominous_trial_key", 0), ("orange_dye", 0),
    ("orange_stained_glass", 0), ("orange_stained_glass_pane", 0), ("orange_terracotta", 0),
    ("oxidized_chiseled_copper", 0), ("oxidized_copper", 0), ("oxidized_copper_bulb", 0),
    ("oxidized_copper_grate", 0), ("oxidized_cut_copper", 0), ("packed_ice", 0), ("packed_mud", 0),
    ("painting", 0), ("pale_hanging_moss", 0), ("pale_moss_block", 0), ("pale_moss_carpet", 0),
    ("paper", 0), ("pearlescent_froglight", 0), ("peony", 0), ("phantom_membrane", 0),
    ("piglin_banner_pattern", 0), ("piglin_head", 0), ("piglin_wall_head", 0), ("pink_dye", 0),
    ("pink_petals", 0), ("pink_stained_glass", 0), ("pink_stained_glass_pane", 0),
    ("pink_terracotta", 0), ("piston", 0), ("piston_head", 0), ("pitcher_crop", 0),
    ("pitcher_plant", 0), ("pitcher_pod", 0), ("player_head", 0), ("player_wall_head", 0),
    ("podzol", 0), ("pointed_dripstone", 0), ("poisonous_potato", 0), ("polished_andesite", 0),
    ("polished_basalt", 0), ("polished_blackstone", 0), ("polished_blackstone_bricks", 0),
    ("polished_blackstone_pressure_plate", 0), ("polished_cinnabar", 0), ("polished_deepslate", 0),
    ("polished_diorite", 0), ("polished_granite", 0), ("polished_sulfur", 0), ("polished_tuff", 0),
    ("popped_chorus_fruit", 0), ("porkchop", 0), ("potato", 0), ("potatoes", 0),
    ("potent_sulfur", 0), ("potion", 0), ("powder_snow", 0), ("powder_snow_bucket", 0),
    ("powered_rail", 0), ("prismarine", 0), ("prismarine_bricks", 0), ("prismarine_crystals", 0),
    ("prismarine_shard", 0), ("pufferfish", 0), ("pufferfish_bucket", 0), ("pumpkin", 0),
    ("pumpkin_pie", 0), ("pumpkin_seeds", 0), ("pumpkin_stem", 0), ("purple_dye", 0),
    ("purple_stained_glass", 0), ("purple_stained_glass_pane", 0), ("purple_terracotta", 0),
    ("purpur_block", 0), ("purpur_pillar", 0), ("purpur_slab", 0), ("purpur_stairs", 0),
    ("quartz", 0), ("quartz_block", 0), ("quartz_bricks", 0), ("quartz_pillar", 0),
    ("quartz_stairs", 0), ("rabbit", 0), ("rabbit_foot", 0), ("rabbit_hide", 0),
    ("rabbit_stew", 0), ("rail", 0), ("raw_copper", 0), ("raw_copper_block", 0), ("raw_gold", 0),
    ("raw_gold_block", 0), ("raw_iron", 0), ("raw_iron_block", 0), ("recovery_compass", 0),
    ("red_dye", 0), ("red_mushroom", 0), ("red_mushroom_block", 0), ("red_nether_bricks", 0),
    ("red_sand", 0), ("red_sandstone", 0), ("red_sandstone_stairs", 0), ("red_shrub", 0),
    ("red_stained_glass", 0), ("red_stained_glass_pane", 0), ("red_terracotta", 0),
    ("redstone", 0), ("redstone_block", 0), ("redstone_lamp", 0), ("redstone_torch", 0),
    ("redstone_wall_torch", 0), ("redstone_wire", 1), ("reinforced_deepslate", 0), ("repeater", 1),
    ("repeating_command_block", 1), ("resin_block", 0), ("resin_brick", 0), ("resin_bricks", 0),
    ("resin_clump", 0), ("respawn_anchor", 1), ("rooted_dirt", 0), ("rose_bush", 0),
    ("rotten_flesh", 0), ("saddle", 0), ("salmon", 0), ("salmon_bucket", 0), ("sand", 0),
    ("sandstone", 0), ("sandstone_stairs", 0), ("scaffolding", 0), ("sculk", 0),
    ("sculk_catalyst", 0), ("sculk_sensor", 0), ("sculk_shrieker", 0), ("sculk_vein", 0),
    ("sea_lantern", 0), ("sea_pickle", 0), ("seagrass", 0), ("shears", 0), ("shelf_mushroom", 0),
    ("shield", 0), ("short_dry_grass", 0), ("short_grass", 0), ("shroomlight", 0),
    ("shulker_shell", 0), ("skeleton_skull", 0), ("skeleton_wall_skull", 0),
    ("skull_banner_pattern", 0), ("slime_ball", 0), ("slime_block", 0), ("small_amethyst_bud", 0),
    ("small_dripleaf", 0), ("smithing_table", 0), ("smoker", 1), ("smooth_basalt", 0),
    ("smooth_quartz", 0), ("smooth_red_sandstone", 0), ("smooth_sandstone", 0),
    ("smooth_stone", 0), ("sniffer_egg", 0), ("snow", 0), ("snow_block", 0), ("snowball", 0),
    ("soul_campfire", 1), ("soul_fire", 0), ("soul_sand", 0), ("soul_soil", 0), ("soul_torch", 0),
    ("soul_wall_torch", 0), ("spawner", 1), ("spectral_arrow", 0), ("spider_eye", 0),
    ("splash_potion", 0), ("sponge", 0), ("spore_blossom", 0), ("spyglass", 0),
    ("stained_clay", 0), ("stained_glass", 0), ("stained_glass_pane", 0), ("stick", 0),
    ("sticky_piston", 0), ("stone", 0), ("stone_axe", 0), ("stone_bricks", 0), ("stone_hoe", 0),
    ("stone_pickaxe", 0), ("stone_pressure_plate", 0), ("stone_shovel", 0), ("stone_sword", 0),
    ("stonecutter", 0), ("straw_bed", 1), ("string", 0), ("stripped_bamboo_block", 0),
    ("structure_block", 3), ("structure_void", 0), ("sugar", 0), ("sugar_cane", 0), ("sulfur", 0),
    ("sulfur_bricks", 0), ("sulfur_cube_bucket", 0), ("sunflower", 0), ("suspicious_gravel", 0),
    ("suspicious_sand", 0), ("suspicious_stew", 0), ("sweet_berries", 0), ("sweet_berry_bush", 1),
    ("tadpole_bucket", 0), ("tall_dry_grass", 0), ("tall_grass", 0), ("tall_seagrass", 0),
    ("target", 0), ("terracotta", 0), ("test_block", 1), ("test_instance_block", 1),
    ("tinted_glass", 0), ("tipped_arrow", 0), ("tnt", 1), ("tnt_minecart", 0), ("torch", 0),
    ("torchflower_crop", 0), ("torchflower_seeds", 0), ("totem_of_undying", 0),
    ("trapped_chest", 1), ("trial_key", 0), ("trial_spawner", 0), ("trident", 0), ("tripwire", 0),
    ("tripwire_hook", 0), ("tropical_fish", 0), ("tropical_fish_bucket", 0), ("tuff", 0),
    ("tuff_bricks", 0), ("turtle_egg", 0), ("turtle_helmet", 0), ("turtle_scute", 0),
    ("twisting_vines", 0), ("twisting_vines_plant", 0), ("vault", 1), ("verdant_froglight", 0),
    ("vine", 0), ("void_air", 0), ("wall_torch", 0), ("warped_fungus", 0),
    ("warped_fungus_on_a_stick", 0), ("warped_nylium", 0), ("warped_roots", 0),
    ("warped_wart_block", 0), ("water", 0), ("water_bucket", 0), ("waxed_chiseled_copper", 0),
    ("waxed_copper_block", 0), ("waxed_copper_bulb", 0), ("waxed_copper_grate", 0),
    ("waxed_cut_copper", 0), ("waxed_exposed_chiseled_copper", 0), ("waxed_exposed_copper", 0),
    ("waxed_exposed_copper_bulb", 0), ("waxed_exposed_copper_grate", 0),
    ("waxed_exposed_cut_copper", 0), ("waxed_oxidized_chiseled_copper", 0),
    ("waxed_oxidized_copper", 0), ("waxed_oxidized_copper_bulb", 0),
    ("waxed_oxidized_copper_grate", 0), ("waxed_oxidized_cut_copper", 0),
    ("waxed_weathered_chiseled_copper", 0), ("waxed_weathered_copper", 0),
    ("waxed_weathered_copper_bulb", 0), ("waxed_weathered_copper_grate", 0),
    ("waxed_weathered_cut_copper", 0), ("weathered_chiseled_copper", 0), ("weathered_copper", 0),
    ("weathered_copper_bulb", 0), ("weathered_copper_grate", 0), ("weathered_cut_copper", 0),
    ("weeping_vines", 0), ("weeping_vines_plant", 0), ("wet_sponge", 0), ("wheat", 0),
    ("wheat_seeds", 0), ("white_dye", 0), ("white_stained_glass", 0),
    ("white_stained_glass_pane", 0), ("white_terracotta", 0), ("wildflowers", 0),
    ("wind_charge", 0), ("wither_skeleton_skull", 0), ("wither_skeleton_wall_skull", 0),
    ("wolf_armor", 0), ("wooden_axe", 0), ("wooden_hoe", 0), ("wooden_pickaxe", 0),
    ("wooden_shovel", 0), ("wooden_sword", 0), ("writable_book", 0), ("written_book", 0),
    ("yellow_dye", 0), ("yellow_stained_glass", 0), ("yellow_stained_glass_pane", 0),
    ("yellow_terracotta", 0), ("zombie_head", 0), ("zombie_wall_head", 0),
];

/// Materials whose flags `Materials` fills in by tag (checked after the explicit table).
fn tag_flags(b: &str) -> Option<u8> {
    let right = b.ends_with("_door")
        || b.ends_with("_trapdoor")
        || b.ends_with("shulker_box")
        || b.ends_with("_button")
        || b == "flower_pot"
        || b.starts_with("potted_")
        || b.ends_with("_bed")
        || b.ends_with("_fence_gate")
        || b == "candle"
        || b.ends_with("_candle")
        || b == "candle_cake"
        || b.ends_with("_candle_cake")
        || b.ends_with("cauldron")
        || b.contains("copper_chest")
        || b.ends_with("copper_golem_statue")
        || b.ends_with("_shelf");
    if right {
        return Some(MODIFIED_ON_RIGHT);
    }
    let none = b.ends_with("_concrete")
        || b.ends_with("_concrete_powder")
        || b.ends_with("_glazed_terracotta")
        || b.ends_with("_banner")
        || b.ends_with("_slab")
        || b.ends_with("_planks")
        || (b.ends_with("_carpet") && !b.contains("moss"))
        || b.ends_with("_sapling")
        || b.ends_with("_log")
        || b.ends_with("_wood")
        || (b.ends_with("_stem") && b != "mushroom_stem")
        || b.ends_with("_hyphae")
        || b.ends_with("_leaves")
        || b.ends_with("_stairs")
        || b.ends_with("_wool")
        || b.ends_with("_wall")
        || b.ends_with("_sign")
        || b.ends_with("_hanging_sign")
        || b.ends_with("_fence")
        || b.ends_with("_ore")
        || b.ends_with("_bars")
        || b == "chain"
        || b.ends_with("_chain")
        || b == "lantern"
        || b.ends_with("_lantern")
        || b == "lightning_rod"
        || b.ends_with("_lightning_rod")
        || b == "pointed_dripstone"
        || (b.ends_with("_pressure_plate")
            && ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo", "crimson", "warped", "pale_oak"]
                .iter()
                .any(|w| b.starts_with(w)))
        || matches!(
            b,
            "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip" | "orange_tulip"
                | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley"
                | "wither_rose" | "torchflower" | "open_eyeblossom" | "closed_eyeblossom"
        );
    none.then_some(0)
}

fn material_flags(id: &str) -> Option<u8> {
    let b = path(id);
    match MATERIAL_FLAGS.binary_search_by(|(n, _)| (*n).cmp(b)) {
        Ok(i) => Some(MATERIAL_FLAGS[i].1),
        Err(_) => tag_flags(b),
    }
}

/// `Materials.isBlockModifiedOnClick`: conservative, true for blocks it does not know.
pub fn modified_on_click(block: &str, right_click: bool) -> bool {
    match material_flags(block) {
        None => true,
        Some(f) => {
            (right_click && f & MODIFIED_ON_RIGHT != 0) || (!right_click && f & MODIFIED_ON_LEFT != 0)
        }
    }
}

/// `Materials.isItemAppliedToBlock`: conservative, true for items it does not know.
pub fn item_applied_to_block(item: &str, block: &str) -> bool {
    match material_flags(item) {
        None => true,
        Some(f) => f & MODIFIES_BLOCKS != 0 || tool_applicable(path(item), path(block)),
    }
}

fn is_waxed_copper(b: &str) -> bool {
    b.starts_with("waxed_") && b.contains("copper")
}

fn is_unwaxed_copper(b: &str) -> bool {
    matches!(
        b,
        "copper_block" | "chiseled_copper" | "copper_door" | "copper_trapdoor" | "copper_grate" | "copper_bulb"
            | "copper_bars" | "copper_torch" | "copper_chest" | "copper_chain" | "copper_wall_torch"
            | "copper_lantern" | "copper_golem_statue"
    ) || b.starts_with("exposed_")
        || b.starts_with("weathered_")
        || b.starts_with("oxidized_")
        || b.starts_with("cut_copper")
}

fn is_sign(b: &str) -> bool {
    b.ends_with("_sign") || b.ends_with("_hanging_sign")
}

/// `Materials.isToolApplicable`.
fn tool_applicable(tool: &str, target: &str) -> bool {
    const TIERS: [&str; 7] = ["wooden", "stone", "copper", "iron", "golden", "diamond", "netherite"];
    let tier_of = |suffix: &str| TIERS.iter().any(|t| tool.strip_suffix(suffix).is_some_and(|p| p == *t));
    if tier_of("_hoe") {
        return matches!(target, "grass_block" | "dirt" | "dirt_path" | "rooted_dirt");
    }
    if tier_of("_axe") {
        if is_waxed_copper(target) {
            return true;
        }
        let log = target.ends_with("_log")
            || target.ends_with("_wood")
            || (target.ends_with("_stem") && target != "mushroom_stem")
            || target.ends_with("_hyphae");
        return log || matches!(target, "pumpkin" | "bamboo_block");
    }
    if tier_of("_shovel") {
        return matches!(target, "grass_block" | "campfire" | "soul_campfire");
    }
    match tool {
        "shears" => matches!(target, "pumpkin" | "bee_nest" | "beehive"),
        "black_dye" | "blue_dye" | "brown_dye" | "cyan_dye" | "gray_dye" | "green_dye" | "light_blue_dye"
        | "light_gray_dye" | "lime_dye" | "magenta_dye" | "orange_dye" | "pink_dye" | "purple_dye" | "red_dye"
        | "white_dye" | "yellow_dye" | "glow_ink_sac" | "ink_sac" => is_sign(target),
        "honeycomb" => is_unwaxed_copper(target) || is_sign(target),
        "brush" => matches!(target, "suspicious_gravel" | "suspicious_sand"),
        "written_book" | "writable_book" => target == "lectern",
        _ => false,
    }
}

/// The flags (checked with `build`) that a right click on this block needs, with what the deny
/// message says, in `RegionProtectionListener.onUseBlock` order. `[]` is the plain build check.
pub fn block_use_flags(block: &str) -> (&'static [&'static str], &'static str) {
    let b = path(block);
    if is_considered_building_if_used(b) {
        return (&[], "use that");
    }
    if is_inventory_block(b) {
        return (&["chest-access"], "open that");
    }
    if b.ends_with("anvil") {
        return (&["use-anvil"], "use that");
    }
    if b.ends_with("_bed") || b == "straw_bed" {
        return (&["interact", "sleep"], "sleep");
    }
    if b == "respawn_anchor" {
        return (&["interact", "respawn-anchors"], "use anchors");
    }
    if b == "tnt" {
        return (&["interact", "tnt"], "use explosives");
    }
    if is_use_flag_applicable(b) {
        return (&["interact", "use"], "use that");
    }
    if b == "big_dripleaf" {
        return (&["interact", "use-dripleaf"], "use that");
    }
    (&["interact"], "use that")
}

/// `Materials.isConsideredBuildingIfUsed`.
pub fn is_considered_building_if_used(b: &str) -> bool {
    matches!(b, "repeater" | "comparator" | "cake" | "dragon_egg" | "flower_pot" | "candle_cake")
        || b.starts_with("potted_")
        || b == "candle"
        || b.ends_with("_candle")
        || b.ends_with("_candle_cake")
        || is_sign(b)
}

/// `Materials.isInventoryBlock` (blocks only).
pub fn is_inventory_block(b: &str) -> bool {
    let b = path(b);
    matches!(
        b,
        "chest" | "jukebox" | "dispenser" | "furnace" | "brewing_stand" | "trapped_chest" | "hopper" | "dropper"
            | "barrel" | "blast_furnace" | "smoker" | "chiseled_bookshelf" | "crafter" | "decorated_pot"
    ) || b.ends_with("_shelf")
        || b.contains("copper_chest")
        || b.ends_with("shulker_box")
}

/// `Materials.isUseFlagApplicable`.
fn is_use_flag_applicable(b: &str) -> bool {
    b.ends_with("_button")
        || b.ends_with("_door")
        || b.ends_with("_trapdoor")
        || b.ends_with("_fence_gate")
        || b.ends_with("_pressure_plate")
        || matches!(
            b,
            "lever" | "lectern" | "enchanting_table" | "bell" | "loom" | "cartography_table" | "stonecutter"
                | "grindstone" | "vault"
        )
}

pub fn is_container(b: &str) -> bool {
    is_inventory_block(b) || path(b) == "lectern"
}

/// What an item used on a block does, as protection sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemUse {
    /// Flint and steel, fire charge: the fire it places is checked (`block-place`, `lighter`).
    Lighter,
    /// A minecart or boat: `vehicle-place`.
    Vehicle,
    /// TNT: used on the clicked block and placed next to it.
    Tnt,
    /// Armor stands, end crystals, cushions and spawn eggs: a plain build check where it spawns.
    Spawns,
    /// Water and lava buckets: the bucket hooks check them.
    Bucket,
    /// Anything else: only matters when it is applied to the clicked block.
    Other,
}

pub fn item_use(item: &str) -> ItemUse {
    let i = path(item);
    if i == "flint_and_steel" || i == "fire_charge" {
        return ItemUse::Lighter;
    }
    if i.ends_with("_boat") || i.ends_with("_raft") || i.ends_with("minecart") || i.ends_with("chest_boat") {
        return ItemUse::Vehicle;
    }
    if i == "tnt" {
        return ItemUse::Tnt;
    }
    if i == "armor_stand" || i == "end_crystal" || i.ends_with("_spawn_egg") || i.ends_with("_cushion") {
        return ItemUse::Spawns;
    }
    if i == "water_bucket" || i == "lava_bucket" {
        return ItemUse::Bucket;
    }
    ItemUse::Other
}

/// `Entities.isHostile` (`Enemy`).
pub fn is_hostile(kind: &str) -> bool {
    matches!(
        path(kind),
        "blaze" | "bogged" | "breeze" | "cave_spider" | "creaking" | "creeper" | "drowned" | "elder_guardian"
            | "ender_dragon" | "enderman" | "endermite" | "evoker" | "ghast" | "giant" | "guardian" | "hoglin"
            | "husk" | "illusioner" | "magma_cube" | "phantom" | "piglin" | "piglin_brute" | "pillager"
            | "ravager" | "shulker" | "silverfish" | "skeleton" | "slime" | "spider" | "stray" | "vex"
            | "vindicator" | "warden" | "witch" | "wither" | "wither_skeleton" | "zoglin" | "zombie"
            | "zombie_villager" | "zombified_piglin" | "parched" | "camel_husk"
    )
}

/// `Entities.isAmbient`.
pub fn is_ambient(kind: &str) -> bool {
    path(kind) == "bat"
}

/// Non-hostile creatures (`Entities.isNonHostile`): damage needs `damage-animals`.
pub fn is_animal(kind: &str) -> bool {
    matches!(
        path(kind),
        "cow" | "mooshroom" | "pig" | "sheep" | "chicken" | "rabbit" | "horse" | "donkey" | "mule"
            | "llama" | "trader_llama" | "camel" | "wolf" | "cat" | "ocelot" | "parrot" | "fox"
            | "panda" | "polar_bear" | "goat" | "frog" | "tadpole" | "axolotl" | "turtle" | "bee"
            | "sniffer" | "armadillo" | "strider" | "squid" | "glow_squid" | "dolphin" | "cod"
            | "salmon" | "pufferfish" | "tropical_fish" | "villager" | "wandering_trader"
            | "iron_golem" | "snow_golem" | "allay" | "skeleton_horse" | "zombie_horse"
            | "happy_ghast" | "copper_golem" | "nautilus" | "zombie_nautilus"
    )
}

/// `Entities.isConsideredBuildingIfUsed`: hanging entities, armor stands, end crystals, allays and
/// minecarts with an inventory.
pub fn is_hanging_or_stand(kind: &str) -> bool {
    matches!(
        path(kind),
        "item_frame" | "glow_item_frame" | "painting" | "armor_stand" | "leash_knot" | "end_crystal" | "allay"
            | "chest_minecart" | "hopper_minecart"
    )
}

/// `Entities.isVehicle`: boats and minecarts.
pub fn is_vehicle(kind: &str) -> bool {
    let k = path(kind);
    k.ends_with("_boat") || k.ends_with("_raft") || k.ends_with("minecart") || k == "boat"
}

/// `Entities.isRiddenOnUse` (without the saddle test): right clicking them is a ride, which
/// `mount` checks.
pub fn is_rideable(kind: &str) -> bool {
    matches!(
        path(kind),
        "horse" | "donkey" | "mule" | "llama" | "trader_llama" | "camel" | "skeleton_horse" | "zombie_horse"
            | "happy_ghast" | "nautilus" | "zombie_nautilus"
    ) || is_vehicle(kind)
}

/// Kinds `mob-spawning` and `deny-spawn` apply to: living mobs, not items, projectiles or
/// players.
pub fn is_mob(kind: &str) -> bool {
    let k = path(kind);
    if k == "player" || k == "armor_stand" || is_vehicle(kind) || is_hanging_or_stand(kind) {
        return false;
    }
    !matches!(
        k,
        "item" | "experience_orb" | "arrow" | "spectral_arrow" | "trident" | "snowball" | "egg"
            | "ender_pearl" | "eye_of_ender" | "potion" | "experience_bottle" | "fireball"
            | "small_fireball" | "dragon_fireball" | "wither_skull" | "shulker_bullet"
            | "llama_spit" | "firework_rocket" | "fishing_bobber" | "tnt" | "falling_block"
            | "area_effect_cloud" | "lightning_bolt" | "marker" | "interaction"
            | "block_display" | "item_display" | "text_display" | "evoker_fangs" | "wind_charge"
            | "breeze_wind_charge" | "ominous_item_spawner" | "splash_potion" | "lingering_potion"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(block_use_flags("minecraft:chest"), (&["chest-access"][..], "open that"));
        assert_eq!(block_use_flags("oak_door").0, &["interact", "use"][..]);
        assert_eq!(block_use_flags("minecraft:anvil").0, &["use-anvil"][..]);
        assert_eq!(block_use_flags("minecraft:oak_sign").0, &[][..] as &[&str]);
        assert_eq!(block_use_flags("minecraft:stone").0, &["interact"][..]);
        assert!(modified_on_click("minecraft:chest", true));
        assert!(!modified_on_click("minecraft:stone", true));
        assert!(!modified_on_click("minecraft:oak_planks", true));
        assert!(modified_on_click("minecraft:some_new_block", true));
        assert!(item_applied_to_block("minecraft:diamond_hoe", "minecraft:dirt"));
        assert!(!item_applied_to_block("minecraft:diamond_hoe", "minecraft:stone"));
        assert!(!item_applied_to_block("minecraft:cooked_beef", "minecraft:stone"));
        assert!(item_applied_to_block("minecraft:bone_meal", "minecraft:grass_block"));
        assert_eq!(item_use("minecraft:oak_boat"), ItemUse::Vehicle);
        assert_eq!(item_use("minecraft:flint_and_steel"), ItemUse::Lighter);
        assert!(is_animal("minecraft:cow"));
        assert!(is_hostile("minecraft:zombie"));
        assert!(!is_animal("minecraft:zombie"));
        assert!(is_mob("minecraft:zombie"));
        assert!(!is_mob("minecraft:item"));
        assert!(MATERIAL_FLAGS.windows(2).all(|w| w[0].0 < w[1].0), "the table must be sorted");
    }
}
