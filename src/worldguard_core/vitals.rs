//! The `heal-*` and `feed-*` flags: WorldGuard's `HealFlag` and `FeedFlag` session handlers, run
//! once a tick for each online player. The caller reads health and food, asks `step`, and applies
//! the changes it returns.

use super::flags::FlagValue;
use super::guard::{Guard, Player};
use super::query::{Actor, Query};
use std::collections::HashMap;

/// A change to apply to a player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    /// Set the health to this value.
    Health(f32),
    /// Set the food level, and the saturation to the same value.
    Food(i32),
}

/// When each player was last healed and fed, on the caller's clock (milliseconds).
#[derive(Default)]
pub struct Vitals {
    last_heal: HashMap<u128, u64>,
    last_feed: HashMap<u128, u64>,
}

fn number(v: Option<FlagValue>) -> Option<f64> {
    match v? {
        FlagValue::Int(i) => Some(i as f64),
        FlagValue::Double(d) => Some(d),
        _ => None,
    }
}

impl Vitals {
    pub fn forget(&mut self, uuid: u128) {
        self.last_heal.remove(&uuid);
        self.last_feed.remove(&uuid);
    }

    /// What the flags at the player's feet change right now. `now_ms` only has to grow.
    pub fn step(&mut self, g: &Guard, p: &Player, health: f32, max_health: f32, food: i32, now_ms: u64) -> Vec<Change> {
        let mut out = Vec::new();
        if !g.cfg.enable {
            return out;
        }
        let Some(w) = g.world(&p.dimension) else { return out };
        let q = Query::new(w);
        let at = p.block();
        let me = Actor::Player(&p.subject);
        // Neither flag hurts the invincible, or players who are not in survival or adventure.
        let spared = q.state(at, "invincible", &me) == Some(true) || (p.game_mode != 0 && p.game_mode != 2);

        let heal_amount = number(q.value(at, "heal-amount", &me));
        let heal_delay = number(q.value(at, "heal-delay", &me));
        let min_health = number(q.value(at, "heal-min-health", &me));
        let max_flag = number(q.value(at, "heal-max-health", &me));
        if health > 0.0
            && let (Some(amount), Some(delay)) = (heal_amount, heal_delay)
            && amount != 0.0
            && delay >= 0.0
            && !(amount < 0.0 && spared)
        {
            let cap = f64::from(max_health);
            let min = min_health.unwrap_or(0.0).min(cap);
            let max = max_flag.unwrap_or(cap).min(cap);
            let now_health = f64::from(health);
            if !(now_health >= max && amount > 0.0) {
                let last = self.last_heal.get(&p.subject.uuid).copied();
                if delay <= 0.0 {
                    out.push(Change::Health((if amount > 0.0 { max } else { min }) as f32));
                    self.last_heal.insert(p.subject.uuid, now_ms);
                } else if last.is_none_or(|l| now_ms.saturating_sub(l) as f64 > delay * 1000.0) {
                    out.push(Change::Health(max.min(min.max(now_health + amount)) as f32));
                    self.last_heal.insert(p.subject.uuid, now_ms);
                }
            }
        }

        let feed_amount = number(q.value(at, "feed-amount", &me));
        let feed_delay = number(q.value(at, "feed-delay", &me));
        let min_food = number(q.value(at, "feed-min-hunger", &me));
        let max_food = number(q.value(at, "feed-max-hunger", &me));
        if let (Some(amount), Some(delay)) = (feed_amount, feed_delay)
            && amount != 0.0
            && delay >= 0.0
            && !(amount < 0.0 && spared)
        {
            let min = min_food.unwrap_or(0.0).min(20.0) as i32;
            let max = max_food.unwrap_or(20.0).min(20.0) as i32;
            if !(food >= max && amount > 0.0) {
                let last = self.last_feed.get(&p.subject.uuid).copied();
                if delay <= 0.0 {
                    out.push(Change::Food(if amount > 0.0 { max } else { min }));
                    self.last_feed.insert(p.subject.uuid, now_ms);
                } else if last.is_none_or(|l| now_ms.saturating_sub(l) as f64 > delay * 1000.0) {
                    out.push(Change::Food(max.min(min.max(food + amount as i32))));
                    self.last_feed.insert(p.subject.uuid, now_ms);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::config::Config;
    use super::super::geom::{BlockPos, Shape};
    use super::super::region::{Region, Subject};

    fn player(game_mode: i32) -> Player {
        Player {
            subject: Subject { uuid: 7, name: "p7".into(), groups: vec![] },
            op_level: 0,
            dimension: "minecraft:overworld".into(),
            pos: [5.5, 5.0, 5.5],
            yaw: 0.0,
            pitch: 0.0,
            game_mode,
            bypass_perm: false,
        }
    }

    fn guard(flags: &[(&str, FlagValue)]) -> Guard {
        let mut g = Guard::new(Config::default());
        let mut r = Region::new("spa", Shape::cuboid(BlockPos::new(0, 0, 0), BlockPos::new(10, 10, 10)));
        for (n, v) in flags {
            r.set_flag(n, Some(v.clone()));
        }
        g.world_mut("minecraft:overworld").put(r);
        g
    }

    #[test]
    fn heal_waits_for_its_delay() {
        let g = guard(&[("heal-amount", FlagValue::Int(2)), ("heal-delay", FlagValue::Int(1))]);
        let p = player(0);
        let mut v = Vitals::default();
        assert_eq!(v.step(&g, &p, 10.0, 20.0, 20, 2000), vec![Change::Health(12.0)]);
        assert_eq!(v.step(&g, &p, 12.0, 20.0, 20, 2500), vec![]);
        assert_eq!(v.step(&g, &p, 12.0, 20.0, 20, 3100), vec![Change::Health(14.0)]);
        assert_eq!(v.step(&g, &p, 19.0, 20.0, 20, 9000), vec![Change::Health(20.0)]);
        assert_eq!(v.step(&g, &p, 20.0, 20.0, 20, 12000), vec![]);
    }

    #[test]
    fn heal_without_delay_fills_up() {
        let g = guard(&[
            ("heal-amount", FlagValue::Int(1)),
            ("heal-delay", FlagValue::Int(0)),
            ("heal-max-health", FlagValue::Double(15.0)),
        ]);
        let mut v = Vitals::default();
        assert_eq!(v.step(&g, &player(0), 3.0, 20.0, 20, 0), vec![Change::Health(15.0)]);
        assert_eq!(v.step(&g, &player(0), 15.0, 20.0, 20, 50), vec![]);
    }

    #[test]
    fn damage_spares_creative_players_and_stops_at_the_minimum() {
        let g = guard(&[
            ("heal-amount", FlagValue::Int(-3)),
            ("heal-delay", FlagValue::Int(1)),
            ("heal-min-health", FlagValue::Double(4.0)),
        ]);
        let mut v = Vitals::default();
        assert_eq!(v.step(&g, &player(1), 20.0, 20.0, 20, 5000), vec![]);
        assert_eq!(v.step(&g, &player(0), 20.0, 20.0, 20, 5000), vec![Change::Health(17.0)]);
        assert_eq!(v.step(&g, &player(0), 5.0, 20.0, 20, 9000), vec![Change::Health(4.0)]);
    }

    #[test]
    fn feed_sets_food_and_respects_the_bounds() {
        let g = guard(&[
            ("feed-amount", FlagValue::Int(4)),
            ("feed-delay", FlagValue::Int(2)),
            ("feed-max-hunger", FlagValue::Int(16)),
        ]);
        let p = player(0);
        let mut v = Vitals::default();
        assert_eq!(v.step(&g, &p, 20.0, 20.0, 10, 3000), vec![Change::Food(14)]);
        assert_eq!(v.step(&g, &p, 20.0, 20.0, 14, 4000), vec![]);
        assert_eq!(v.step(&g, &p, 20.0, 20.0, 14, 5100), vec![Change::Food(16)]);
        assert_eq!(v.step(&g, &p, 20.0, 20.0, 16, 9000), vec![]);
    }

    #[test]
    fn nothing_without_the_flags_or_outside_the_region() {
        let g = guard(&[("heal-amount", FlagValue::Int(2))]);
        let mut v = Vitals::default();
        assert_eq!(v.step(&g, &player(0), 5.0, 20.0, 5, 9000), vec![], "heal-delay is missing");
        let mut outside = player(0);
        outside.pos = [50.5, 5.0, 5.5];
        let g = guard(&[("heal-amount", FlagValue::Int(2)), ("heal-delay", FlagValue::Int(0))]);
        assert_eq!(v.step(&g, &outside, 5.0, 20.0, 5, 9000), vec![]);
    }
}
