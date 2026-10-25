use std::path::Path;

/// True when the path's extension is one of `allowed`, ignoring ASCII case.
pub fn has_extension(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            allowed
                .iter()
                .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        })
}
