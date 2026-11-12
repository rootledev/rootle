//! Search language families shared by facets, highlighting and the filesystem fixture.
//! The strop families/aliases follow strop-core/src/languages.rs (MIT,
//! © 2026 Tarek Nawara); rootle's additional language families are retained.

use std::sync::LazyLock;

struct Language {
    name: &'static str,
    aliases: Vec<&'static str>,
    extensions: Vec<&'static str>,
}

static LANGUAGES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    rootle_stdio::SEARCH_LANGUAGES
        .lines()
        .map(|line| {
            let mut fields = line.split('\t');
            Language {
                name: fields.next().expect("language name"),
                aliases: fields
                    .next()
                    .expect("language aliases")
                    .split(',')
                    .filter(|alias| !alias.is_empty())
                    .collect(),
                extensions: fields
                    .next()
                    .expect("language extensions")
                    .split(',')
                    .collect(),
            }
        })
        .collect()
});

pub fn lang_exts(name: &str) -> Option<&'static [&'static str]> {
    LANGUAGES
        .iter()
        .find(|language| language.name == name || language.aliases.contains(&name))
        .map(|language| language.extensions.as_slice())
}

pub fn ext_lang(extension: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|language| language.extensions.contains(&extension))
        .map(|language| language.name)
}

pub(crate) fn path_ext(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .and_then(|base| base.rsplit_once('.'))
        .map_or_else(String::new, |(_, ext)| ext.to_ascii_lowercase())
}
