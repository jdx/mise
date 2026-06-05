use std::borrow::Cow;

pub(crate) fn append_str_ext(s: &str, ext: &str) -> String {
    if ext.is_empty() {
        return s.to_string();
    }
    if ext.starts_with('.') {
        format!("{s}{ext}")
    } else {
        format!("{s}.{ext}")
    }
}

pub(crate) fn file_ext(filename: &str, version: &str) -> Option<String> {
    let version = version.trim_start_matches(['v', 'V']);
    let filename = file_name_without_version(filename, version);
    filename
        .rsplit_once('.')
        .and_then(|(_, ext)| is_likely_file_extension(ext).then_some(ext.to_string()))
}

pub(crate) fn file_ext_is_empty(filename: &str, version: &str) -> bool {
    file_ext(filename, version).is_none()
}

fn file_name_without_version<'a>(file_name: &'a str, version: &str) -> Cow<'a, str> {
    if version.is_empty() {
        return Cow::Borrowed(file_name);
    }

    let mut stripped = None::<String>;
    let mut anchor = 0;
    let mut search_start = 0;
    while let Some(relative_start) = file_name[search_start..].find(version) {
        let start = search_start + relative_start;
        let end = start + version.len();
        if is_version_boundary_before(file_name, start) && is_version_boundary_after(file_name, end)
        {
            stripped
                .get_or_insert_with(|| String::with_capacity(file_name.len()))
                .push_str(&file_name[anchor..start]);
            anchor = end;
            search_start = end;
        } else {
            search_start = start
                + file_name[start..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
        }
    }

    match stripped {
        Some(mut stripped) => {
            stripped.push_str(&file_name[anchor..]);
            Cow::Owned(stripped)
        }
        None => Cow::Borrowed(file_name),
    }
}

fn is_version_boundary_before(s: &str, index: usize) -> bool {
    let Some((prev_index, prev)) = s[..index].char_indices().next_back() else {
        return true;
    };
    match prev {
        '-' | '_' | '+' | ' ' => true,
        '.' => s[..prev_index]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_digit()),
        'v' | 'V' => s[..prev_index]
            .chars()
            .next_back()
            .is_none_or(|c| matches!(c, '-' | '_' | '+' | ' ' | '.')),
        _ => false,
    }
}

fn is_version_boundary_after(s: &str, index: usize) -> bool {
    let Some(next) = s[index..].chars().next() else {
        return true;
    };
    match next {
        '-' | '_' | '+' | ' ' => true,
        '.' => s[index + next.len_utf8()..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_digit()),
        _ => false,
    }
}

fn is_likely_file_extension(ext: &str) -> bool {
    !ext.is_empty()
        && !ext.chars().all(|c| c.is_ascii_digit())
        && !ext.chars().any(|c| matches!(c, '-' | '_' | '+' | ' '))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_ext_ignores_selected_version_dots() {
        assert_eq!(file_ext("tool_1.0.0", "v1.0.0"), None);
        assert_eq!(file_ext("tool.1.0.0", "v1.0.0"), None);
        assert_eq!(file_ext("x1.8atool_1.8_win", "1.8"), None);
        assert_eq!(file_ext("tool-1.1.1", "1.1"), None);
    }

    #[test]
    fn file_ext_preserves_real_extensions() {
        assert_eq!(file_ext("arq.bat", "1.0.0"), Some("bat".to_string()));
        assert_eq!(file_ext("tool.jar", "1.0.0"), Some("jar".to_string()));
        assert_eq!(
            file_ext("tool_1.0.0.bat", "v1.0.0"),
            Some("bat".to_string())
        );
        assert_eq!(
            file_ext("tool_1.0.0.ps1", "v1.0.0"),
            Some("ps1".to_string())
        );
    }
}
