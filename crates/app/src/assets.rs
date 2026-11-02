//! Embedded icon assets: serves the few gpui-component icons the app uses.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

/// Asset source for the icons gpui-component loads by path.
pub struct Icons;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        const ICONS: &[(&str, &[u8])] = &[
            $((concat!("icons/", $name, ".svg"),
               include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice()),)*
        ];
    };
}

icons!(
    "asterisk",
    "book-open",
    "case-sensitive",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "close",
    "copy",
    "external-link",
    "eye",
    "eye-off",
    "inbox",
    "loader",
    "minus",
    "plus",
    "replace",
    "search",
    "triangle-alert",
);

impl AssetSource for Icons {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(p, _)| *p == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(p, _)| p.starts_with(path))
            .map(|(p, _)| SharedString::from(*p))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::{IconName, IconNamed};

    macro_rules! used {
        ($($variant:ident),* $(,)?) => {
            /// Every icon gpui-component draws inside components the app mounts, plus
            /// app-side uses, keyed by variant name.
            fn used() -> Vec<(&'static str, IconName)> {
                vec![$((stringify!($variant), IconName::$variant)),*]
            }
        };
    }

    used!(
        Asterisk,
        BookOpen,
        CaseSensitive,
        Check,
        ChevronDown,
        ChevronLeft,
        ChevronRight,
        Close,
        Copy,
        ExternalLink,
        Eye,
        EyeOff,
        Inbox,
        Loader,
        Minus,
        Plus,
        Replace,
        Search,
        TriangleAlert,
    );

    #[test]
    fn every_used_icon_loads_as_svg_bytes() {
        for (_, icon) in used() {
            let path = icon.path();
            let bytes = Icons
                .load(&path)
                .unwrap()
                .unwrap_or_else(|| panic!("{path} missing"));
            assert!(bytes.starts_with(b"<svg"), "{path} is not an svg");
        }
    }

    #[test]
    fn unknown_and_empty_paths_load_as_none() {
        assert!(
            Icons
                .load("icons/definitely-not-an-icon.svg")
                .unwrap()
                .is_none()
        );
        assert!(Icons.load("").unwrap().is_none());
    }

    #[test]
    fn list_returns_exactly_the_embedded_paths_with_the_prefix() {
        assert_eq!(Icons.list("icons/").unwrap().len(), ICONS.len());
        assert!(Icons.list("icons/chevron-d").unwrap().len() == 1);
        assert!(Icons.list("nope/").unwrap().is_empty());
    }

    #[test]
    fn every_icon_variant_named_in_app_sources_resolves() {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|e| e == "rs") {
                    out.push(p);
                }
            }
        }
        let mut files = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut files,
        );
        let mut missing = Vec::new();
        for file in files {
            // This file's own tests name variants as data, not as uses.
            if file.ends_with("assets.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for (i, _) in text.match_indices("IconName::") {
                let variant: String = text[i + "IconName::".len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric())
                    .collect();
                let resolves = used()
                    .into_iter()
                    .find(|(name, _)| *name == variant)
                    .is_some_and(|(_, icon)| matches!(Icons.load(&icon.path()), Ok(Some(_))));
                if !resolves && !missing.contains(&variant) {
                    missing.push(variant);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "icons without an embedded SVG: {missing:?}; copy the SVG into crates/app/assets/icons/, add it to icons! above and to used! in these tests"
        );
    }
}
