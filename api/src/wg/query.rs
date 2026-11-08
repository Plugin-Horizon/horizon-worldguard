//! Flag and build queries over the regions at a point: WorldGuard's `FlagValueCalculator`.
//!
//! - The applicable regions are those containing the point plus all their parents, in normative
//!   order (priority high to low, children before their parents), then the global region.
//! - A region's flag is its own or the nearest parent's; its region group is checked against the
//!   subject's standing in the region the walk is at (parents' lists included).
//! - The walk takes the flag from the highest priority tier that sets it. A region sets aside its
//!   parents; `build` is implicitly set by every region without `passthrough allow`.
//! - `build` without a value comes from membership: a member of every protecting region of the top
//!   priority tier. Everything protected is checked as `build` combined with the specific flags.

use super::flags::{self, FlagValue, Group};
use super::geom::BlockPos;
use super::region::{Region, Subject};
use super::store::World;
use std::collections::HashSet;

/// Who does something.
#[derive(Clone, Debug)]
pub enum Actor<'a> {
    Player(&'a Subject),
    /// A block or entity source (piston, TNT, fire): a member of the regions it is in, and of
    /// their children.
    Source(BlockPos),
    /// No known source: a non-member everywhere.
    Nobody,
}

pub struct Query<'w> {
    pub world: &'w World,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

impl Verdict {
    pub fn allowed(self) -> bool {
        self == Verdict::Allow
    }
}

/// `StateFlag.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum St {
    Allow,
    Deny,
}

/// `StateFlag.combine`: deny beats allow; nothing set stays nothing.
pub fn combine(states: &[Option<St>]) -> Option<St> {
    let mut allowed = false;
    for s in states {
        match s {
            Some(St::Deny) => return Some(St::Deny),
            Some(St::Allow) => allowed = true,
            None => {}
        }
    }
    if allowed { Some(St::Allow) } else { None }
}

const NO_PRIORITY: i64 = i32::MIN as i64;

impl<'w> Query<'w> {
    pub fn new(world: &'w World) -> Self {
        Query { world }
    }

    /// Regions at `p` (no global, no parents), highest priority first, children before parents.
    pub fn sorted_at(&self, p: BlockPos) -> Vec<&'w Region> {
        let rs = self.world.at(p);
        self.sort(rs)
    }

    fn sort(&self, mut rs: Vec<&'w Region>) -> Vec<&'w Region> {
        rs.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| self.depth(b).cmp(&self.depth(a)))
                .then_with(|| a.id.cmp(&b.id))
        });
        rs
    }

    fn depth(&self, r: &Region) -> usize {
        self.world.ancestors(&r.id).len()
    }

    /// The regions of a query: those at `p` and all their parents (`COMPUTE_PARENTS`), sorted,
    /// the global region last.
    fn applicable(&self, p: BlockPos) -> Vec<&'w Region> {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut all: Vec<&'w Region> = Vec::new();
        for r in self.world.at(p) {
            if seen.insert(r.id.as_str()) {
                all.push(r);
            }
            for a in self.world.ancestors(&r.id) {
                if seen.insert(a.id.as_str()) {
                    all.push(a);
                }
            }
        }
        let mut all = self.sort(all);
        if let Some(g) = self.world.global() {
            all.push(g);
        }
        all
    }

    fn priority_of(r: &Region) -> i64 {
        if r.is_global() { NO_PRIORITY } else { r.priority as i64 }
    }

    /// (member, owner) of `actor` in `r`, parents' lists included.
    pub fn standing(&self, r: &Region, actor: &Actor<'_>) -> (bool, bool) {
        match actor {
            Actor::Player(s) => {
                let (mut m, mut o) = r.own_standing(s);
                if !(m && o) {
                    for a in self.world.ancestors(&r.id) {
                        let (am, ao) = a.own_standing(s);
                        m |= am;
                        o |= ao;
                    }
                }
                (m, o)
            }
            Actor::Source(at) => {
                if r.is_global() {
                    // The global region belongs to sources outside every region.
                    return (self.world.at(*at).is_empty(), false);
                }
                // The source is in this region, or in one of its parents.
                let inside = self.world.at(*at).iter().any(|src| {
                    src.id == r.id || self.world.ancestors(&r.id).iter().any(|a| a.id == src.id)
                });
                (inside, false)
            }
            Actor::Nobody => (false, false),
        }
    }

    /// The flag `name` on `r` or inherited from its parents, with the group that applies.
    pub fn inherited(&self, r: &'w Region, name: &str) -> Option<(&'w FlagValue, Group)> {
        let default_group = flags::lookup_exact(name).map_or(Group::All, |d| d.default_group);
        if let Some(v) = r.flag(name) {
            return Some((v, r.group(name).unwrap_or(default_group)));
        }
        for a in self.world.ancestors(&r.id) {
            if let Some(v) = a.flag(name) {
                return Some((v, a.group(name).unwrap_or(default_group)));
            }
        }
        None
    }

    /// `FlagValueCalculator.getEffectiveFlagOf` for one region (`/rg teleport`).
    pub fn effective_flag(&self, r: &'w Region, name: &str, actor: &Actor<'_>) -> Option<FlagValue> {
        self.effective(r, name, actor)
    }

    /// `getEffectiveFlagOf`: the value of `name` for `actor` at region `r` (own or inherited), after
    /// the region group check, with the special cases of the global region.
    fn effective(&self, r: &'w Region, name: &str, actor: &Actor<'_>) -> Option<FlagValue> {
        if r.is_global() {
            if name == "passthrough" {
                let pt = r.flag("passthrough").and_then(FlagValue::as_state);
                let has_people = !r.members.is_empty() || !r.owners.is_empty();
                return if pt == Some(false) || (pt != Some(true) && has_people) {
                    None
                } else {
                    Some(FlagValue::State(true))
                };
            } else if name == "build" {
                // The legacy rule: build cannot be allowed on the global region.
                return match r.flag("build").and_then(FlagValue::as_state) {
                    Some(false) => Some(FlagValue::State(false)),
                    _ => None,
                };
            }
        }
        let has_group = name != "nonplayer-protection-domains";
        let default_group = flags::lookup_exact(name).map_or(Group::All, |d| d.default_group);
        let (m, o) = self.standing(r, actor);
        let mut chain: Vec<&'w Region> = vec![r];
        chain.extend(self.world.ancestors(&r.id));
        for cur in chain {
            if let Some(v) = cur.flag(name) {
                let use_it = if !has_group {
                    true
                } else {
                    let group = cur.group(name).unwrap_or(default_group);
                    match actor {
                        Actor::Nobody => group.contains(false, false),
                        _ => group.contains(m, o),
                    }
                };
                if use_it {
                    return Some(v.clone());
                }
            }
        }
        None
    }

    /// `queryAllValues`: the values of `name` from the deciding priority tier (`accept_one`: the
    /// first only). `name` is a built-in flag or an unknown one (no default, no special rules).
    pub fn all_values_of(&self, p: BlockPos, name: &str, actor: &Actor<'_>, accept_one: bool) -> Vec<FlagValue> {
        let def = flags::lookup_exact(name);
        let is_build = name == "build";
        let list = self.applicable(p);
        let mut min_priority = NO_PRIORITY;
        let mut considered: Vec<FlagValue> = Vec::new();
        let mut ignored: HashSet<&str> = HashSet::new();
        for r in list.iter().copied() {
            let priority = Self::priority_of(r);
            if priority < min_priority {
                break;
            }
            if ignored.contains(r.id.as_str()) {
                continue;
            }
            if let Some(v) = self.effective(r, name, actor) {
                min_priority = priority;
                if accept_one {
                    return vec![v];
                }
                considered.push(v);
            }
            for a in self.world.ancestors(&r.id) {
                ignored.insert(a.id.as_str());
            }
            // `build` is implicitly set on every region where passthrough is not allow.
            if priority != min_priority
                && is_build
                && self.effective(r, "passthrough", actor).and_then(|v| v.as_state()) != Some(true)
            {
                min_priority = priority;
            }
        }
        if is_build && considered.is_empty() {
            match self.membership(&list, actor) {
                Membership::Fail => return Vec::new(),
                Membership::Success => return vec![FlagValue::State(true)],
                Membership::NoRegions => {}
            }
        }
        if considered.is_empty() {
            if let Some(d) = def
                && d.kind == flags::Kind::State
                && d.default_allow == Some(true)
            {
                return vec![FlagValue::State(true)];
            }
            return Vec::new();
        }
        considered
    }

    /// The regions of a query at `p` (those containing it and their parents), as a result set
    /// lists them: highest priority first, no global region.
    pub fn regions_at(&self, p: BlockPos) -> Vec<&'w Region> {
        let mut all = self.applicable(p);
        if all.last().is_some_and(|r| r.is_global()) {
            all.pop();
        }
        all
    }

    /// `getMembership`.
    fn membership(&self, list: &[&'w Region], actor: &Actor<'_>) -> Membership {
        let mut min_priority = NO_PRIORITY;
        let mut result = Membership::NoRegions;
        let mut ignored: HashSet<&str> = HashSet::new();
        for r in list.iter().copied() {
            let priority = Self::priority_of(r);
            if priority < min_priority {
                break;
            }
            if self.effective(r, "passthrough", actor).and_then(|v| v.as_state()) == Some(true) {
                continue;
            }
            if ignored.contains(r.id.as_str()) {
                continue;
            }
            min_priority = priority;
            let (m, o) = self.standing(r, actor);
            if m || o {
                result = Membership::Success;
                for a in self.world.ancestors(&r.id) {
                    ignored.insert(a.id.as_str());
                }
            } else {
                return Membership::Fail;
            }
        }
        result
    }

    /// `queryState` over several flags: deny beats allow, stops at the first deny.
    pub fn query_state(&self, p: BlockPos, names: &[&str], actor: &Actor<'_>) -> Option<St> {
        let mut value: Option<St> = None;
        for n in names {
            let vals = self.all_values_of(p, n, actor, false);
            let states: Vec<Option<St>> = vals
                .iter()
                .map(|v| v.as_state().map(|b| if b { St::Allow } else { St::Deny }))
                .collect();
            value = combine(&[value, combine(&states)]);
            if value == Some(St::Deny) {
                break;
            }
        }
        value
    }

    /// All values of a flag that apply to `actor` at `p` (empty: none).
    pub fn values(&self, p: BlockPos, name: &str, actor: &Actor<'_>) -> Vec<FlagValue> {
        self.all_values_of(p, name, actor, false)
    }

    /// A state flag: Some(true) allow, Some(false) deny, None when nobody sets it (and it has no
    /// default).
    pub fn state(&self, p: BlockPos, name: &str, actor: &Actor<'_>) -> Option<bool> {
        self.query_state(p, &[name], actor).map(|s| s == St::Allow)
    }

    /// `testState` for one flag: allowed only when the state resolves to allow (an unset flag
    /// without a default is not allowed).
    pub fn test_state(&self, p: BlockPos, names: &[&str], actor: &Actor<'_>) -> bool {
        self.query_state(p, names, actor) == Some(St::Allow)
    }

    /// A state flag with its default when unset, as the listeners that only look for `DENY` see
    /// it: false only when the resolved state is deny.
    pub fn allows(&self, p: BlockPos, name: &str, actor: &Actor<'_>) -> bool {
        match self.state(p, name, actor) {
            Some(v) => v,
            None => flags::lookup_exact(name).and_then(|d| d.default_allow).unwrap_or(true),
        }
    }

    /// A non-state flag's value (`queryValue`: the first of the deciding tier).
    pub fn value(&self, p: BlockPos, name: &str, actor: &Actor<'_>) -> Option<FlagValue> {
        self.all_values_of(p, name, actor, true).into_iter().next()
    }

    /// `testBuild`: whether `actor` may do something protected by the flags `specific` at `p`.
    pub fn can_build(&self, p: BlockPos, actor: &Actor<'_>, specific: &[&str]) -> Verdict {
        let allowed = if specific.is_empty() {
            self.test_state(p, &["build"], actor)
        } else {
            let build = match self.query_state(p, &["build"], actor) {
                Some(St::Deny) => None,
                other => other,
            };
            let flags = self.query_state(p, specific, actor);
            combine(&[build, flags]) == Some(St::Allow)
        };
        if allowed { Verdict::Allow } else { Verdict::Deny }
    }

    /// The ids of the regions at `p`, highest priority first (for `/rg info` without id).
    pub fn ids_at(&self, p: BlockPos) -> Vec<String> {
        self.sorted_at(p).iter().map(|r| r.id.clone()).collect()
    }
}

enum Membership {
    NoRegions,
    Fail,
    Success,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::geom::Shape;
    use super::super::region::Region;

    fn subject(name: &str, n: u128) -> Subject {
        Subject {
            uuid: n,
            name: name.into(),
            groups: vec![],
        }
    }

    fn cub(id: &str, a: [i32; 3], b: [i32; 3], pri: i32) -> Region {
        let mut r = Region::new(id, Shape::cuboid(a.into(), b.into()));
        r.priority = pri;
        r
    }

    fn world(rs: Vec<Region>) -> World {
        let mut w = World::new("world");
        for r in rs {
            w.put(r);
        }
        w
    }

    #[test]
    fn membership_and_build() {
        let mut home = cub("home", [0, 0, 0], [10, 10, 10], 0);
        home.members.add_uuid(1);
        let w = world(vec![home]);
        let q = Query::new(&w);
        let a = subject("a", 1);
        let b = subject("b", 2);
        let p = BlockPos::new(5, 5, 5);
        assert!(q.can_build(p, &Actor::Player(&a), &["block-break"]).allowed());
        assert!(!q.can_build(p, &Actor::Player(&b), &["block-break"]).allowed());
        assert!(q.can_build(BlockPos::new(50, 5, 5), &Actor::Player(&b), &[]).allowed());
    }

    #[test]
    fn specific_flag_beats_build_and_deny_beats_allow() {
        let mut r = cub("r", [0, 0, 0], [10, 10, 10], 0);
        r.set_flag("build", Some(FlagValue::State(false)));
        r.set_flag("use", Some(FlagValue::State(true)));
        let w = world(vec![r]);
        let q = Query::new(&w);
        let b = subject("b", 2);
        let p = BlockPos::new(1, 1, 1);
        assert!(q.can_build(p, &Actor::Player(&b), &["use"]).allowed());
        assert!(!q.can_build(p, &Actor::Player(&b), &["block-place"]).allowed());

        let mut x = cub("x", [0, 0, 0], [10, 10, 10], 5);
        x.set_flag("pvp", Some(FlagValue::State(true)));
        let mut y = cub("y", [0, 0, 0], [10, 10, 10], 5);
        y.set_flag("pvp", Some(FlagValue::State(false)));
        let w = world(vec![x, y]);
        assert_eq!(Query::new(&w).state(p, "pvp", &Actor::Nobody), Some(false));
    }

    #[test]
    fn priority_decides() {
        let mut low = cub("low", [0, 0, 0], [10, 10, 10], 0);
        low.set_flag("pvp", Some(FlagValue::State(false)));
        let mut high = cub("high", [0, 0, 0], [5, 10, 5], 10);
        high.set_flag("pvp", Some(FlagValue::State(true)));
        let unset = cub("unset", [0, 0, 0], [10, 10, 10], 20);
        let w = world(vec![low, high, unset]);
        let q = Query::new(&w);
        assert_eq!(q.state(BlockPos::new(1, 1, 1), "pvp", &Actor::Nobody), Some(true));
        assert_eq!(q.state(BlockPos::new(8, 1, 8), "pvp", &Actor::Nobody), Some(false));
        // Membership: only the top tier counts.
        let a = subject("a", 1);
        let mut w = world(vec![]);
        let mut top = cub("top", [0, 0, 0], [10, 10, 10], 10);
        top.members.add_uuid(1);
        w.put(top);
        w.put(cub("below", [0, 0, 0], [10, 10, 10], 0));
        assert!(Query::new(&w).can_build(BlockPos::new(1, 1, 1), &Actor::Player(&a), &[]).allowed());
        // Same priority: a member of all.
        w.get_mut("below").unwrap().priority = 10;
        assert!(!Query::new(&w).can_build(BlockPos::new(1, 1, 1), &Actor::Player(&a), &[]).allowed());
    }

    #[test]
    fn region_groups_and_inheritance() {
        // The documented market/shop example.
        let mut market = cub("market", [0, 0, 0], [100, 100, 100], 0);
        market.set_flag("pvp", Some(FlagValue::State(false)));
        market.set_group("pvp", Some(Group::NonMembers));
        let mut shop = cub("shop1", [0, 0, 0], [10, 10, 10], 0);
        shop.members.add_name("trader");
        shop.parent = Some("market".into());
        let w = world(vec![market, shop]);
        let q = Query::new(&w);
        let trader = subject("Trader", 7);
        let other = subject("other", 8);
        let p = BlockPos::new(5, 5, 5);
        assert_eq!(q.state(p, "pvp", &Actor::Player(&trader)), None);
        assert_eq!(q.state(p, "pvp", &Actor::Player(&other)), Some(false));
        // Without inheritance (spawn + market overlapping), membership elsewhere does not count.
        let mut spawn = cub("spawn", [0, 0, 0], [100, 100, 100], 0);
        spawn.set_flag("pvp", Some(FlagValue::State(false)));
        spawn.set_group("pvp", Some(Group::NonMembers));
        let mut m2 = cub("market", [0, 0, 0], [10, 10, 10], 0);
        m2.members.add_name("trader");
        let w = world(vec![spawn, m2]);
        assert_eq!(Query::new(&w).state(p, "pvp", &Actor::Player(&trader)), Some(false));
    }

    #[test]
    fn entry_defaults_to_non_members() {
        let mut r = cub("vault", [0, 0, 0], [10, 10, 10], 0);
        r.set_flag("entry", Some(FlagValue::State(false)));
        r.owners.add_uuid(1);
        let w = world(vec![r]);
        let q = Query::new(&w);
        let p = BlockPos::new(1, 1, 1);
        assert!(q.allows(p, "entry", &Actor::Player(&subject("o", 1))));
        assert!(!q.allows(p, "entry", &Actor::Player(&subject("x", 2))));
    }

    #[test]
    fn global_region() {
        let mut w = world(vec![]);
        w.global_mut().set_flag("tnt", Some(FlagValue::State(false)));
        let q = Query::new(&w);
        let p = BlockPos::new(1, 1, 1);
        let b = subject("b", 2);
        assert!(q.can_build(p, &Actor::Player(&b), &["block-break"]).allowed());
        assert!(!q.can_build(p, &Actor::Nobody, &["tnt"]).allowed());
        w.global_mut().set_flag("passthrough", Some(FlagValue::State(false)));
        assert!(!Query::new(&w).can_build(p, &Actor::Player(&b), &["block-break"]).allowed());
    }

    #[test]
    fn sources_are_members_of_their_regions() {
        let r = cub("r", [0, 0, 0], [10, 10, 10], 0);
        let w = world(vec![r]);
        let q = Query::new(&w);
        let inside = Actor::Source(BlockPos::new(2, 2, 2));
        let outside = Actor::Source(BlockPos::new(20, 2, 2));
        let p = BlockPos::new(1, 1, 1);
        assert!(q.can_build(p, &inside, &["block-break"]).allowed());
        assert!(!q.can_build(p, &outside, &["block-break"]).allowed());
    }
}
