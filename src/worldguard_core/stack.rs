//! `/stack`: WorldGuard's `stackPlayerInventory` (items of the same kind are merged into the
//! earlier stacks, up to their maximum size). The caller says which stacks can merge by giving
//! them the same `group` and applies the counts that come back.

/// One non-empty inventory stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub slot: u32,
    /// Stacks with the same group are the same item with the same components.
    pub group: usize,
    pub count: u32,
    /// The item's maximum stack size.
    pub max: u32,
}

/// The slots whose count changes, with the new count (0: the slot is emptied). `entries` must be
/// in slot order.
pub fn compact(entries: &[Entry]) -> Vec<(u32, u32)> {
    let mut counts: Vec<u32> = entries.iter().map(|e| e.count).collect();
    for i in 0..entries.len() {
        let item = entries[i];
        // Stacks of one (tools, armour) never merge.
        if counts[i] == 0 || item.max <= 1 || counts[i] >= item.max {
            continue;
        }
        let mut needed = item.max - counts[i];
        for j in (i + 1)..entries.len() {
            if counts[j] == 0 || entries[j].group != item.group {
                continue;
            }
            if counts[j] > needed {
                counts[i] = item.max;
                counts[j] -= needed;
                break;
            }
            counts[i] += counts[j];
            counts[j] = 0;
            needed = item.max - counts[i];
        }
    }
    entries
        .iter()
        .zip(&counts)
        .filter(|(e, n)| e.count != **n)
        .map(|(e, n)| (e.slot, *n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(slot: u32, group: usize, count: u32, max: u32) -> Entry {
        Entry { slot, group, count, max }
    }

    #[test]
    fn merges_into_the_earlier_stack() {
        let got = compact(&[e(0, 0, 40, 64), e(3, 0, 30, 64), e(5, 1, 10, 64), e(9, 0, 20, 64)]);
        // 40 + 30 = 70: slot 0 fills to 64, slot 3 keeps 6, then slot 3 takes slot 9: 6 + 20 = 26.
        assert_eq!(got, vec![(0, 64), (3, 26), (9, 0)]);
    }

    #[test]
    fn small_stacks_empty_into_the_first() {
        assert_eq!(compact(&[e(1, 0, 10, 64), e(2, 0, 5, 64)]), vec![(1, 15), (2, 0)]);
    }

    #[test]
    fn unstackable_and_full_stacks_stay() {
        assert_eq!(compact(&[e(0, 0, 1, 1), e(1, 0, 1, 1)]), vec![]);
        assert_eq!(compact(&[e(0, 0, 64, 64), e(1, 0, 3, 64)]), vec![]);
    }

    #[test]
    fn other_groups_are_left_alone() {
        assert_eq!(compact(&[e(0, 0, 10, 16), e(1, 1, 10, 16)]), vec![]);
    }
}
