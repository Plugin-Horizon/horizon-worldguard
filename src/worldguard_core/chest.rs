//! Sign chest protection (`chest-protection.enable`): a `[Lock]` sign under a chest (or beside
//! the chest's column) locks the chest to the names on the sign's lines 2 to 4. The search order
//! follows WorldGuard's `SignChestProtection`.

/// What the protection reads from the world.
pub trait Reader {
    /// The block id at `pos` (`minecraft:chest`), None when unknown or unloaded.
    fn block(&self, pos: [i32; 3]) -> Option<String>;
    /// The four lines of the sign at `pos`, None when there is no sign block entity.
    fn sign_lines(&self, pos: [i32; 3]) -> Option<Vec<String>>;
}

fn short(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

/// `ChestProtection.isChest`.
pub fn is_chest(id: &str) -> bool {
    matches!(short(id), "chest" | "dispenser" | "furnace" | "trapped_chest" | "dropper")
}

/// A standing sign post (`Tag.STANDING_SIGNS`): not a wall or hanging sign.
pub fn is_standing_sign(id: &str) -> bool {
    let s = short(id);
    s.ends_with("_sign") && !s.contains("wall") && !s.contains("hanging")
}

fn off(p: [i32; 3], d: [i32; 3]) -> [i32; 3] {
    [p[0] + d[0], p[1] + d[1], p[2] + d[2]]
}

const SIDES: [[i32; 3]; 4] = [[-1, 0, 0], [1, 0, 0], [0, 0, -1], [0, 0, 1]];

/// `isProtectedSign`: Some(true) when the sign at `pos` is a lock the player is not named on
/// (no player: always protected), Some(false) when the player is named, None when it is no lock.
fn sign_protects(w: &dyn Reader, pos: [i32; 3], player: Option<&str>) -> Option<bool> {
    let block = w.block(pos)?;
    if !short(&block).ends_with("sign") {
        return None;
    }
    let lines = w.sign_lines(pos)?;
    if !lines.first().is_some_and(|l| l.eq_ignore_ascii_case("[Lock]")) {
        return None;
    }
    let Some(name) = player else { return Some(true) };
    Some(!(1..4).any(|i| lines.get(i).is_some_and(|l| l.trim().eq_ignore_ascii_case(name))))
}

fn sign_and_chest(w: &dyn Reader, pos: [i32; 3], player: Option<&str>) -> Option<bool> {
    if !w.block(off(pos, [0, 1, 0])).is_some_and(|b| is_chest(&b)) {
        return None;
    }
    sign_protects(w, pos, player)
}

fn sign_around(w: &dyn Reader, search: [i32; 3], player: Option<&str>) -> bool {
    if sign_protects(w, search, player) == Some(true) {
        return true;
    }
    SIDES.iter().any(|d| sign_and_chest(w, off(search, *d), player) == Some(true))
}

/// `isProtected`: whether the block at `pos` is locked against `player` (None: no one, as a
/// hopper or an explosion).
pub fn is_protected(w: &dyn Reader, pos: [i32; 3], player: Option<&str>) -> bool {
    let Some(block) = w.block(pos) else { return false };
    if is_chest(&block) {
        sign_around(w, off(pos, [0, -1, 0]), player)
    } else if is_standing_sign(&block) {
        sign_and_chest(w, pos, player) == Some(true)
    } else {
        sign_protects(w, off(pos, [0, 1, 0]), player).unwrap_or(false)
    }
}

/// `isProtectedPlacement`: a lock sign may not go beside a chest locked to someone else.
pub fn is_protected_placement(w: &dyn Reader, pos: [i32; 3], player: Option<&str>) -> bool {
    sign_around(w, pos, player)
}

/// `isAdjacentChestProtected`: the block or one of its four neighbours is locked.
pub fn is_adjacent_protected(w: &dyn Reader, pos: [i32; 3], player: Option<&str>) -> bool {
    is_protected(w, pos, player) || SIDES.iter().any(|d| is_protected(w, off(pos, *d), player))
}

/// The checks of `ChestProtectionListener.onSignChange` for a `[Lock]` sign being written:
/// Err(message) means the sign must be removed.
pub fn check_lock_sign(
    w: &dyn Reader,
    pos: [i32; 3],
    player: &str,
    lines: &[String],
    protected_here: bool,
) -> Result<&'static str, &'static str> {
    if protected_here {
        return Err("You do not own the adjacent chest.");
    }
    let here = w.block(pos).unwrap_or_default();
    if !is_standing_sign(&here) {
        return Err("The [Lock] sign must be a sign post, not a wall sign.");
    }
    if !lines.get(1).is_some_and(|l| l.eq_ignore_ascii_case(player)) {
        return Err("The first owner line must be your name.");
    }
    let below = w.block(off(pos, [0, -1, 0])).unwrap_or_default();
    if matches!(short(&below), "tnt" | "sand" | "gravel") || is_standing_sign(&below) {
        return Err("That is not a safe block that you're putting this sign on.");
    }
    Ok("A chest or double chest above is now protected.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        blocks: HashMap<[i32; 3], String>,
        signs: HashMap<[i32; 3], Vec<String>>,
    }

    impl Reader for Fake {
        fn block(&self, pos: [i32; 3]) -> Option<String> {
            self.blocks.get(&pos).cloned()
        }
        fn sign_lines(&self, pos: [i32; 3]) -> Option<Vec<String>> {
            self.signs.get(&pos).cloned()
        }
    }

    fn lock(owner: &str) -> Vec<String> {
        vec!["[Lock]".into(), owner.into(), String::new(), String::new()]
    }

    /// A chest at (0,1,0) on a sign post at (0,0,0).
    fn world() -> Fake {
        let mut w = Fake::default();
        w.blocks.insert([0, 1, 0], "minecraft:chest".into());
        w.blocks.insert([0, 0, 0], "minecraft:oak_sign".into());
        w.signs.insert([0, 0, 0], lock("Alex"));
        w
    }

    #[test]
    fn the_owner_passes_and_others_do_not() {
        let w = world();
        assert!(!is_protected(&w, [0, 1, 0], Some("alex")));
        assert!(is_protected(&w, [0, 1, 0], Some("Sam")));
        assert!(is_protected(&w, [0, 1, 0], None));
    }

    #[test]
    fn a_sign_beside_the_column_locks_the_chest() {
        let mut w = Fake::default();
        w.blocks.insert([0, 1, 0], "minecraft:trapped_chest".into());
        w.blocks.insert([1, 0, 0], "minecraft:oak_sign".into());
        w.blocks.insert([1, 1, 0], "minecraft:chest".into());
        w.signs.insert([1, 0, 0], lock("Alex"));
        assert!(is_protected(&w, [0, 1, 0], Some("Sam")));
        assert!(!is_protected(&w, [0, 1, 0], Some("Alex")));
    }

    #[test]
    fn the_sign_and_the_block_above_it_are_covered() {
        let w = world();
        // Breaking the sign post is locked too.
        assert!(is_protected(&w, [0, 0, 0], Some("Sam")));
        assert!(!is_protected(&w, [0, 0, 0], Some("Alex")));
    }

    #[test]
    fn an_unlocked_chest_is_free() {
        let mut w = world();
        w.signs.insert([0, 0, 0], vec!["hello".into(), String::new(), String::new(), String::new()]);
        assert!(!is_protected(&w, [0, 1, 0], Some("Sam")));
        assert!(!is_protected(&w, [5, 5, 5], Some("Sam")));
    }

    #[test]
    fn placing_next_to_a_foreign_chest_is_refused() {
        let mut w = world();
        w.blocks.insert([1, 0, 0], "minecraft:oak_sign".into());
        // The lock at (0,0,0) under the chest at (0,1,0) is beside the new sign.
        assert!(is_protected_placement(&w, [1, 0, 0], Some("Sam")));
        assert!(!is_protected_placement(&w, [1, 0, 0], Some("Alex")));
    }

    #[test]
    fn lock_sign_rules() {
        let mut w = Fake::default();
        w.blocks.insert([0, 0, 0], "minecraft:oak_sign".into());
        w.blocks.insert([0, -1, 0], "minecraft:stone".into());
        assert_eq!(
            check_lock_sign(&w, [0, 0, 0], "Alex", &lock("Alex"), false),
            Ok("A chest or double chest above is now protected.")
        );
        assert_eq!(
            check_lock_sign(&w, [0, 0, 0], "Alex", &lock("Sam"), false),
            Err("The first owner line must be your name.")
        );
        w.blocks.insert([0, 0, 0], "minecraft:oak_wall_sign".into());
        assert_eq!(
            check_lock_sign(&w, [0, 0, 0], "Alex", &lock("Alex"), false),
            Err("The [Lock] sign must be a sign post, not a wall sign.")
        );
        w.blocks.insert([0, 0, 0], "minecraft:oak_sign".into());
        w.blocks.insert([0, -1, 0], "minecraft:sand".into());
        assert_eq!(
            check_lock_sign(&w, [0, 0, 0], "Alex", &lock("Alex"), false),
            Err("That is not a safe block that you're putting this sign on.")
        );
        assert_eq!(
            check_lock_sign(&w, [0, 0, 0], "Alex", &lock("Alex"), true),
            Err("You do not own the adjacent chest.")
        );
    }
}
