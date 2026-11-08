//! Locale fixtures.

/// The four variables that `i18n::start` reads.
pub const VARS: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];

/// One environment map: its name and the variables it sets. A variable
/// not in a map is unset for the call.
pub type EnvSet = (&'static str, &'static [(&'static str, &'static str)]);

/// The eight environment maps E1 to E8.
pub const ENV: [EnvSet; 8] = [
    ("E1", &[("LANG", "eo.UTF-8")]),
    ("E2", &[("LANG", "C"), ("LANGUAGE", "eo")]),
    ("E3", &[("LC_ALL", "en_US.UTF-8"), ("LANG", "eo.UTF-8")]),
    ("E4", &[("LANGUAGE", "fr:eo"), ("LANG", "en_US.UTF-8")]),
    ("E5", &[]),
    ("E6", &[("LC_MESSAGES", "eo"), ("LANG", "en_US.UTF-8")]),
    ("E7", &[("LANG", "C.UTF-8"), ("LANGUAGE", "eo")]),
    ("E8", &[("LANG", "12_ab.UTF-8")]),
];

/// The environment map `name`.
pub fn env(name: &str) -> &'static [(&'static str, &'static str)] {
    ENV.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| *m)
        .expect("X-loc-env map")
}

/// The English texts of `fixtures/loc/en.tsv`, in file order, each
/// `(ID, text)`; the two characters `\n` in a text stand
/// for a line break.
pub fn english() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(crate::fixture("loc", "en.tsv")).expect("read en.tsv");
    text.lines()
        .map(|l| {
            let (id, t) = l.split_once('\t').expect("ID<TAB>text");
            (id.to_string(), t.replace("\\n", "\n"))
        })
        .collect()
}
