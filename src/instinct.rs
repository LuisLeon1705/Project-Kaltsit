//! Minecraft Instinct Handler: sub-millisecond survival reflexes and emergency protocols.
//!
//! Evaluates telemetry tick-by-tick (20 Hz) on CPU with zero GPU/LLM latency,
//! executing critical survival cascades (drowning, lava, creeper explosions, hunger).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PotionKind {
    WaterBreathing,
    FireResistance,
}

#[derive(Clone, Debug, PartialEq)]
pub enum InstinctAction {
    /// Ascend towards open air/surface while submerged.
    SwimUp,
    /// Drink an emergency potion immediately.
    DrinkPotion(PotionKind),
    /// Place a door on an adjacent solid block to create an instant 2-block air pocket.
    PlaceDoor,
    /// Dig an emergency pocket of air in the adjacent wall or ceiling.
    DigAirPocket,
    /// Place and retrieve water bucket to extinguish burning on solid ground.
    ExtinguishWithWater,
    /// Rush out of lava towards the nearest solid ledge.
    EscapeLava,
    /// Eat food from hotbar/inventory to regenerate health and sprint.
    EatFood,
    /// Sprint away from an ignited creeper or imminent explosion.
    FleeCreeper { distance: f32 },
    /// Raise shield or strike approaching hostile mob.
    DefendOrAttack { threat_distance: f32 },
    /// Pathfind towards the player when falling behind.
    FollowPlayer { distance: f32 },
    /// Stand by or observe peacefully.
    Idle,
}

impl InstinctAction {
    pub fn name(&self) -> &'static str {
        match self {
            InstinctAction::SwimUp => "SwimUp",
            InstinctAction::DrinkPotion(_) => "DrinkPotion",
            InstinctAction::PlaceDoor => "PlaceDoor",
            InstinctAction::DigAirPocket => "DigAirPocket",
            InstinctAction::ExtinguishWithWater => "ExtinguishWithWater",
            InstinctAction::EscapeLava => "EscapeLava",
            InstinctAction::EatFood => "EatFood",
            InstinctAction::FleeCreeper { .. } => "FleeCreeper",
            InstinctAction::DefendOrAttack { .. } => "DefendOrAttack",
            InstinctAction::FollowPlayer { .. } => "FollowPlayer",
            InstinctAction::Idle => "Idle",
        }
    }
}

/// Telemetry state fed into the instinct evaluator every tick (20 Hz).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Telemetry {
    /// Health (0.0 to 20.0).
    pub health: f32,
    /// Food level (0.0 to 20.0).
    pub food: f32,
    /// Oxygen level (0.0 to 300 ticks, or normalized 0.0 to 1.0).
    pub oxygen_ratio: f32,
    /// Submerged in water.
    pub in_water: bool,
    /// Submerged or in contact with lava.
    pub in_lava: bool,
    /// Burning on fire.
    pub on_fire: bool,
    /// Overhead path to open air is blocked (e.g. underwater cave, sunken ship).
    pub ceiling_blocked: bool,
    /// Distance to open air directly above (if reachable).
    pub surface_distance: Option<f32>,
    /// Inventory has a Water Breathing potion ready.
    pub has_water_breathing_potion: bool,
    /// Inventory has a Fire Resistance potion ready.
    pub has_fire_resistance_potion: bool,
    /// Inventory has a door ready to create an air pocket.
    pub has_door: bool,
    /// Inventory has a water bucket ready.
    pub has_water_bucket: bool,
    /// Adjacent solid block can be mined to make an air chamber.
    pub can_dig_air_pocket: bool,
    /// Distance to the player (in blocks).
    pub player_distance: f32,
    /// Distance to the closest hostile mob (in blocks).
    pub nearest_mob_distance: Option<f32>,
    /// Closest hostile is an ignited creeper about to explode.
    pub creeper_primed: bool,
    /// Inventory has edible food.
    pub has_food: bool,
    /// Nether dimension (where water cannot be placed).
    pub in_nether: bool,
}

/// Evaluates immediate survival priorities in sub-millisecond CPU time (< 0.005 ms).
pub fn evaluate_instinct(t: &Telemetry) -> InstinctAction {
    // ── 1. CRITICAL EMERGENCY: LAVA & FIRE ──
    if t.in_lava {
        if t.has_fire_resistance_potion {
            return InstinctAction::DrinkPotion(PotionKind::FireResistance);
        }
        return InstinctAction::EscapeLava;
    }

    if t.on_fire && !t.in_nether && t.has_water_bucket {
        return InstinctAction::ExtinguishWithWater;
    }

    // ── 2. CRITICAL EMERGENCY: DROWNING / WATER SUFFOCATION ──
    if t.in_water && t.oxygen_ratio < 0.40 {
        // If surface is open and close, swim up
        if !t.ceiling_blocked && t.surface_distance.unwrap_or(0.0) <= 12.0 {
            return InstinctAction::SwimUp;
        }

        // Trapped under ceiling or too deep: use survival tricks
        if t.has_water_breathing_potion {
            return InstinctAction::DrinkPotion(PotionKind::WaterBreathing);
        }

        // Classic door air-pocket technique
        if t.has_door {
            return InstinctAction::PlaceDoor;
        }

        // Dig a pocket in the adjacent wall or ceiling
        if t.can_dig_air_pocket {
            return InstinctAction::DigAirPocket;
        }

        // Fallback: try swimming up even if ceiling seems blocked
        return InstinctAction::SwimUp;
    }

    // ── 3. IMMINENT EXPLOSION: CREEPER ──
    if t.creeper_primed {
        return InstinctAction::FleeCreeper {
            distance: t.nearest_mob_distance.unwrap_or(3.0),
        };
    }

    // ── 4. IMMEDIATE COMBAT THREAT ──
    if let Some(dist) = t.nearest_mob_distance {
        if dist <= 3.5 {
            return InstinctAction::DefendOrAttack { threat_distance: dist };
        }
    }

    // ── 5. HUNGER & HEALTH REGENERATION ──
    if t.food <= 14.0 && t.has_food && t.nearest_mob_distance.map_or(true, |d| d > 5.0) {
        return InstinctAction::EatFood;
    }

    // ── 6. COMPANION LOCOMOTION (FOLLOW PLAYER) ──
    if t.player_distance > 6.0 {
        return InstinctAction::FollowPlayer {
            distance: t.player_distance,
        };
    }

    InstinctAction::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lava_drinks_fire_resistance_or_escapes() {
        let with_potion = Telemetry {
            in_lava: true,
            has_fire_resistance_potion: true,
            ..Default::default()
        };
        assert_eq!(
            evaluate_instinct(&with_potion),
            InstinctAction::DrinkPotion(PotionKind::FireResistance)
        );

        let without_potion = Telemetry {
            in_lava: true,
            has_fire_resistance_potion: false,
            ..Default::default()
        };
        assert_eq!(evaluate_instinct(&without_potion), InstinctAction::EscapeLava);
    }

    #[test]
    fn fire_on_ground_extinguishes_with_water_outside_nether() {
        let on_fire_overworld = Telemetry {
            on_fire: true,
            has_water_bucket: true,
            in_nether: false,
            ..Default::default()
        };
        assert_eq!(
            evaluate_instinct(&on_fire_overworld),
            InstinctAction::ExtinguishWithWater
        );

        let on_fire_nether = Telemetry {
            on_fire: true,
            has_water_bucket: true,
            in_nether: true,
            ..Default::default()
        };
        assert_ne!(
            evaluate_instinct(&on_fire_nether),
            InstinctAction::ExtinguishWithWater
        );
    }

    #[test]
    fn drowning_cascade_open_surface_swims_up() {
        let t = Telemetry {
            in_water: true,
            oxygen_ratio: 0.25,
            ceiling_blocked: false,
            surface_distance: Some(4.0),
            has_door: true,
            ..Default::default()
        };
        assert_eq!(evaluate_instinct(&t), InstinctAction::SwimUp);
    }

    #[test]
    fn drowning_cascade_blocked_ceiling_drinks_potion_or_uses_door_or_digs() {
        // 1. With potion
        let t_potion = Telemetry {
            in_water: true,
            oxygen_ratio: 0.20,
            ceiling_blocked: true,
            has_water_breathing_potion: true,
            has_door: true,
            can_dig_air_pocket: true,
            ..Default::default()
        };
        assert_eq!(
            evaluate_instinct(&t_potion),
            InstinctAction::DrinkPotion(PotionKind::WaterBreathing)
        );

        // 2. Without potion, with door
        let t_door = Telemetry {
            in_water: true,
            oxygen_ratio: 0.20,
            ceiling_blocked: true,
            has_water_breathing_potion: false,
            has_door: true,
            can_dig_air_pocket: true,
            ..Default::default()
        };
        assert_eq!(evaluate_instinct(&t_door), InstinctAction::PlaceDoor);

        // 3. Without door, can dig
        let t_dig = Telemetry {
            in_water: true,
            oxygen_ratio: 0.20,
            ceiling_blocked: true,
            has_water_breathing_potion: false,
            has_door: false,
            can_dig_air_pocket: true,
            ..Default::default()
        };
        assert_eq!(evaluate_instinct(&t_dig), InstinctAction::DigAirPocket);
    }

    #[test]
    fn primed_creeper_triggers_immediate_flee() {
        let t = Telemetry {
            creeper_primed: true,
            nearest_mob_distance: Some(3.2),
            ..Default::default()
        };
        assert_eq!(
            evaluate_instinct(&t),
            InstinctAction::FleeCreeper { distance: 3.2 }
        );
    }

    #[test]
    fn hunger_eats_food_when_safe() {
        let t = Telemetry {
            food: 12.0,
            has_food: true,
            nearest_mob_distance: Some(15.0),
            player_distance: 2.0,
            ..Default::default()
        };
        assert_eq!(evaluate_instinct(&t), InstinctAction::EatFood);
    }

    #[test]
    fn follows_player_when_distant() {
        let t = Telemetry {
            food: 20.0,
            player_distance: 10.5,
            ..Default::default()
        };
        assert_eq!(
            evaluate_instinct(&t),
            InstinctAction::FollowPlayer { distance: 10.5 }
        );
    }
}
