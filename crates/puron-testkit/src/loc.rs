//! Locale fixtures: the environments under which the language choice is
//! tested, and the English texts the user interface must show.

/// The four locale variables that the applet's `i18n::start` reads.
/// A test that sets an environment from `ENV` unsets the others.
pub const VARS: [&str; 4] = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"];

/// A named locale environment: its name and the variables it sets, as
/// `(variable, value)`. A variable of `VARS` that a map does not name is
/// unset when the map is applied.
pub type EnvSet = (&'static str, &'static [(&'static str, &'static str)]);

/// The locale environments of the language tests, named `E1` to `E8`.
/// Among them: `LANGUAGE` under a `C` locale (E2, E7), a `LANGUAGE`
/// list of two languages (E4), no variable set (E5), and a `LANG` whose
/// name is not a language (E8). The applet's test `c_i18n_6` lists the
/// language each one must give.
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

/// The variables of the environment `name` of `ENV`; panics if there is
/// none.
pub fn env(name: &str) -> &'static [(&'static str, &'static str)] {
    ENV.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, m)| *m)
        .expect("X-loc-env map")
}

/// The English texts of `fixtures/loc/en.tsv`, in file order, each as
/// `(message ID, text)`. A line of the file is the ID, a tab and the
/// text; the two characters `\n` in a text stand for a line break.
pub fn english() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(crate::fixture("loc", "en.tsv")).expect("read en.tsv");
    text.lines()
        .map(|l| {
            let (id, t) = l.split_once('\t').expect("ID<TAB>text");
            (id.to_string(), t.replace("\\n", "\n"))
        })
        .collect()
}
