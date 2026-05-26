use std::cmp::Ordering;

use super::catalog::{compare_versions, resource_key, DesktopResourceIndex, DesktopResourceItem};

pub fn merge_catalog_items(items: Vec<DesktopResourceItem>) -> DesktopResourceIndex {
    let mut index = DesktopResourceIndex::default();

    for item in items {
        let key = resource_key(&item);
        match index.resources.get_mut(&key) {
            Some(current)
                if compare_versions(&current.version, &item.version) == Ordering::Less =>
            {
                *current = item;
            }
            Some(_) => {}
            None => {
                index.resources.insert(key, item);
            }
        }
    }

    index
}
