use std::{collections::HashSet, fmt};

pub const MAX_GROUP_NAME_CHARS: usize = 120;
pub const MAX_GROUP_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GroupPathError {
    Empty,
    TooLong,
    TooDeep,
    InvalidSegment,
    Reserved,
}

impl fmt::Display for GroupPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Empty => "A group name is required.",
            Self::TooLong => "Group paths are limited to 120 characters.",
            Self::TooDeep => "Group nesting is limited to 8 levels.",
            Self::InvalidSegment => "Group names contain an invalid path segment.",
            Self::Reserved => "UNGROUPED is a reserved group name.",
        };
        formatter.write_str(message)
    }
}

pub fn normalize_group_segment(raw: &str) -> Result<String, GroupPathError> {
    let segment = raw.trim();
    if segment.is_empty() {
        return Err(GroupPathError::Empty);
    }
    if segment.chars().count() > MAX_GROUP_NAME_CHARS {
        return Err(GroupPathError::TooLong);
    }
    if segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
        || segment.chars().any(char::is_control)
    {
        return Err(GroupPathError::InvalidSegment);
    }
    Ok(segment.to_owned())
}

pub fn normalize_group_path(raw: &str) -> Result<String, GroupPathError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(GroupPathError::Empty);
    }
    let segments = raw
        .split('/')
        .map(normalize_group_segment)
        .collect::<Result<Vec<_>, _>>()?;
    if segments.len() > MAX_GROUP_DEPTH {
        return Err(GroupPathError::TooDeep);
    }
    let path = segments.join("/");
    if path.chars().count() > MAX_GROUP_NAME_CHARS {
        return Err(GroupPathError::TooLong);
    }
    if segments[0].eq_ignore_ascii_case("UNGROUPED") {
        return Err(GroupPathError::Reserved);
    }
    Ok(path)
}

pub fn group_key(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join("/")
}

pub fn group_parent_path(path: &str) -> Option<String> {
    path.rsplit_once('/').map(|(parent, _)| parent.to_owned())
}

pub fn group_leaf_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn is_group_within(path: &str, parent: &str) -> bool {
    let path = group_key(path);
    let parent = group_key(parent);
    path == parent
        || path
            .strip_prefix(&parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

pub fn rebase_group_path(path: &str, old_parent: &str, new_parent: &str) -> Option<String> {
    let path_segments = path.split('/').collect::<Vec<_>>();
    let old_segments = old_parent.split('/').collect::<Vec<_>>();
    if path_segments.len() < old_segments.len()
        || !path_segments
            .iter()
            .zip(&old_segments)
            .all(|(path, old)| path.trim().eq_ignore_ascii_case(old.trim()))
    {
        return None;
    }

    let mut rebased = new_parent.split('/').map(str::to_owned).collect::<Vec<_>>();
    rebased.extend(
        path_segments
            .into_iter()
            .skip(old_segments.len())
            .map(str::to_owned),
    );
    Some(rebased.join("/"))
}

pub fn group_rename_conflicts(
    paths: &[String],
    old_parent: &str,
    new_parent: &str,
) -> Result<bool, GroupPathError> {
    let rebased_paths = paths
        .iter()
        .filter(|path| is_group_within(path, old_parent))
        .filter_map(|path| rebase_group_path(path, old_parent, new_parent))
        .map(|path| normalize_group_path(&path).map(|path| group_key(&path)))
        .collect::<Result<HashSet<_>, _>>()?;

    Ok(paths
        .iter()
        .filter(|path| !is_group_within(path, old_parent))
        .any(|path| rebased_paths.contains(&group_key(path))))
}

pub fn group_ancestors(path: &str) -> Vec<String> {
    let segments = path.split('/').collect::<Vec<_>>();
    (1..=segments.len())
        .map(|length| segments[..length].join("/"))
        .collect()
}

pub fn same_group_parent(left: &str, right: &str) -> bool {
    match (group_parent_path(left), group_parent_path(right)) {
        (None, None) => true,
        (Some(left), Some(right)) => group_key(&left) == group_key(&right),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_legacy_and_nested_group_paths() {
        assert_eq!(normalize_group_path(" Production ").unwrap(), "Production");
        assert_eq!(
            normalize_group_path(" Production / Databases ").unwrap(),
            "Production/Databases"
        );
        assert_eq!(
            group_ancestors("Production/Databases/Primary"),
            [
                "Production",
                "Production/Databases",
                "Production/Databases/Primary"
            ]
        );
    }

    #[test]
    fn rejects_ambiguous_or_unbounded_group_paths() {
        assert_eq!(
            normalize_group_path("Production//Databases"),
            Err(GroupPathError::Empty)
        );
        assert_eq!(
            normalize_group_path("Production/../Databases"),
            Err(GroupPathError::InvalidSegment)
        );
        assert_eq!(
            normalize_group_path("UNGROUPED/Child"),
            Err(GroupPathError::Reserved)
        );
        assert_eq!(
            normalize_group_path("a/b/c/d/e/f/g/h/i"),
            Err(GroupPathError::TooDeep)
        );
        assert_eq!(
            normalize_group_path(&"x".repeat(MAX_GROUP_NAME_CHARS + 1)),
            Err(GroupPathError::TooLong)
        );
    }

    #[test]
    fn group_ancestry_uses_segment_boundaries_and_case_insensitive_names() {
        assert!(is_group_within("PRODUCTION/DB", "production"));
        assert!(is_group_within("production", "PRODUCTION"));
        assert!(!is_group_within("Production2/DB", "Production"));
        assert!(!same_group_parent("Production/DB", "Staging/DB"));
        assert!(same_group_parent("Production/DB", "production/Web"));
    }

    #[test]
    fn rebases_only_the_selected_group_subtree() {
        assert_eq!(
            rebase_group_path("Production/Databases/Primary", "production", "Live"),
            Some("Live/Databases/Primary".to_owned())
        );
        assert_eq!(
            rebase_group_path("Production2/Database", "Production", "Live"),
            None
        );
    }

    #[test]
    fn rejects_group_renames_that_merge_existing_subtrees() {
        let paths = [
            "Production".to_owned(),
            "Production/Databases".to_owned(),
            "Live".to_owned(),
            "Live/Databases".to_owned(),
            "Staging".to_owned(),
        ];

        assert!(group_rename_conflicts(&paths, "Production", "Live").unwrap());
        assert!(!group_rename_conflicts(&paths, "Production", "Archive").unwrap());
        assert!(!group_rename_conflicts(&paths, "Production/Databases", "Live/Archive").unwrap());
        assert_eq!(
            group_rename_conflicts(&paths, "Production", &"x".repeat(MAX_GROUP_NAME_CHARS + 1)),
            Err(GroupPathError::TooLong)
        );
    }
}
