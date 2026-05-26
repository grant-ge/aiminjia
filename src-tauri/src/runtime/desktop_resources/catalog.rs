use std::{cmp::Ordering, collections::BTreeMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceDisplay {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tagline: String,
    #[serde(default)]
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceItem {
    pub resource_type: String,
    pub resource_id: String,
    pub version: String,
    pub scope: String,
    pub display: DesktopResourceDisplay,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub manifest_sha256: String,
    #[serde(default)]
    pub manifest_size: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopResourceIndex {
    #[serde(default)]
    pub resources: BTreeMap<String, DesktopResourceItem>,
}

pub fn resource_key(item: &DesktopResourceItem) -> String {
    format!("{}:{}:{}", item.resource_type, item.resource_id, item.scope)
}

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');

    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (left_part, right_part) => {
                let left_value = left_part.map(parse_version_part).unwrap_or_default();
                let right_value = right_part.map(parse_version_part).unwrap_or_default();

                match left_value.cmp(&right_value) {
                    Ordering::Equal if left_part.is_none() || right_part.is_none() => {
                        if left_part.is_none() {
                            return compare_remaining_parts(right_parts, Ordering::Less);
                        }
                        return compare_remaining_parts(left_parts, Ordering::Greater);
                    }
                    Ordering::Equal => {}
                    ordering => return ordering,
                }
            }
        }
    }
}

fn parse_version_part(part: &str) -> u64 {
    part.chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>()
        .parse::<u64>()
        .unwrap_or_default()
}

fn compare_remaining_parts<'a>(
    mut parts: impl Iterator<Item = &'a str>,
    non_zero_ordering: Ordering,
) -> Ordering {
    if parts.any(|part| parse_version_part(part) > 0) {
        non_zero_ordering
    } else {
        Ordering::Equal
    }
}

pub fn select_newer(
    current: DesktopResourceItem,
    incoming: DesktopResourceItem,
) -> DesktopResourceItem {
    if compare_versions(&current.version, &incoming.version) == Ordering::Less {
        incoming
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(
        resource_type: &str,
        resource_id: &str,
        scope: &str,
        version: &str,
    ) -> DesktopResourceItem {
        DesktopResourceItem {
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            scope: scope.to_string(),
            version: version.to_string(),
            display: DesktopResourceDisplay {
                name: resource_id.to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn resource_key_includes_type_id_and_scope() {
        let item = item("skill", "daily-assistant", "tenant", "1.0.0");

        assert_eq!(resource_key(&item), "skill:daily-assistant:tenant");
    }

    #[test]
    fn select_newer_uses_numeric_version_segments() {
        let current = item("skill", "daily-assistant", "tenant", "1.2.0");
        let incoming = item("skill", "daily-assistant", "tenant", "1.10.0");

        let selected = select_newer(current, incoming);

        assert_eq!(selected.version, "1.10.0");
    }
}
