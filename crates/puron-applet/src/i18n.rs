//! Translation catalogs and lookup. One source, compiled into
//! `puron-applet` and `puron-settings`. Each crate's `i18n.toml` names
//! the catalog directory `i18n/` of the workspace root, domain `puron`,
//! fallback language `en`.

use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{I18nAssets, LanguageLoader};
use puron_core::format::{Text as ValueText, Unit};

/// Catalog domain.
pub const DOMAIN: &str = "puron";
/// Fallback language.
pub const FALLBACK: &str = "en";

/// The catalogs of `i18n/<language>/puron.ftl`, embedded at build time.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../i18n/"]
pub struct Catalogs;

/// Assets a loader reads its catalogs from.
pub type Assets = Box<dyn I18nAssets + Send + Sync + 'static>;

struct State {
    loader: FluentLanguageLoader,
    #[allow(dead_code)]
    assets: Assets,
}

static STATE: OnceLock<State> = OnceLock::new();

fn english() -> LanguageIdentifier {
    FALLBACK.parse().expect("fallback tag")
}

/// A loader for domain `puron` with no catalog loaded yet.
pub fn new_loader() -> FluentLanguageLoader {
    FluentLanguageLoader::new(DOMAIN, english())
}

/// A value of the environment as a language tag: the part from
/// `.` or `@` dropped, `_` written as `-`; `None` when it is not a tag.
fn tag(value: &OsStr) -> Option<LanguageIdentifier> {
    let v = value.to_str()?;
    let v = v.split(['.', '@']).next().unwrap_or("");
    v.replace('_', "-").parse().ok()
}

/// The language list for `start`, from the four locale variables as `get`
/// gives them.
pub fn languages(get: impl Fn(&str) -> Option<OsString>) -> Vec<LanguageIdentifier> {
    let set = |name: &str| get(name).filter(|v| !v.is_empty());
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|name| set(name));
    let Some(locale) = locale else {
        return vec![english()];
    };
    let c = locale
        .to_str()
        .is_some_and(|l| l == "C" || l == "POSIX" || l.starts_with("C."));
    if c {
        return vec![english()];
    }
    let mut values: Vec<OsString> = set("LANGUAGE")
        .map(|v| {
            v.to_string_lossy()
                .split(':')
                .filter(|e| !e.is_empty())
                .map(OsString::from)
                .collect()
        })
        .unwrap_or_default();
    if values.is_empty() {
        values.push(locale);
    }
    let list: Vec<LanguageIdentifier> = values.iter().filter_map(|v| tag(v)).collect();
    if list.is_empty() {
        vec![english()]
    } else {
        list
    }
}

/// Negotiates `list` over the catalogs of `assets` and loads the result
/// into `loader`; returns the chosen language (English when none
/// matches).
pub fn choose(
    loader: &FluentLanguageLoader,
    assets: &dyn I18nAssets,
    list: &[LanguageIdentifier],
) -> LanguageIdentifier {
    let chosen = i18n_embed::select(loader, assets, list).unwrap_or_default();
    match chosen.first() {
        Some(l) => l.clone(),
        None => {
            let _ = i18n_embed::select(loader, assets, &[english()]);
            english()
        }
    }
}

/// Start with the given catalogs and environment. Runs once per
/// process; a later call changes nothing. Returns the
/// chosen language.
pub fn start_with(assets: Assets, get: impl Fn(&str) -> Option<OsString>) -> LanguageIdentifier {
    let mut chosen = None;
    let state = STATE.get_or_init(|| {
        let loader = new_loader();
        chosen = Some(choose(&loader, assets.as_ref(), &languages(get)));
        State { loader, assets }
    });
    chosen.unwrap_or_else(|| state.loader.current_language())
}

/// Start with the shipped catalogs and the process environment.
pub fn start() -> LanguageIdentifier {
    start_with(Box::new(Catalogs), |name| std::env::var_os(name))
}

/// The loader of this process; English alone when `start` has not run.
pub fn loader() -> &'static FluentLanguageLoader {
    &STATE
        .get_or_init(|| {
            let loader = new_loader();
            choose(&loader, &Catalogs, &[english()]);
            State {
                loader,
                assets: Box::new(Catalogs),
            }
        })
        .loader
}

/// `fl!` over the process loader; the ID is checked against the English
/// file at build time.
macro_rules! fl {
    ($id:literal) => {
        i18n_embed_fl::fl!($crate::i18n::loader(), $id)
    };
}

/// The entry's text in the chosen language, else English.
pub fn text(id: &str) -> String {
    loader().get(id)
}

/// The suffix entry of a unit.
fn suffix(unit: Unit) -> String {
    match unit {
        Unit::Celsius => fl!("suffix-celsius"),
        Unit::Fahrenheit => fl!("suffix-fahrenheit"),
        Unit::Rpm => fl!("suffix-rpm"),
        Unit::Volt => fl!("suffix-volt"),
        Unit::Watt => fl!("suffix-watt"),
    }
}

/// Value text: the number text as `core.format` gives it, then
/// the unit's suffix (none: nothing added); "no value" (`None`) is
/// `no-value`. The number is joined in code, never a catalog argument.
pub fn value_text(value: Option<&ValueText>) -> String {
    match value {
        None => fl!("no-value"),
        Some(t) => {
            let mut s = t.number.clone();
            if let Some(unit) = t.unit {
                s.push_str(&suffix(unit));
            }
            s
        }
    }
}

/// Panel value: `None` is *missing* (`missing-value`), else
/// Value text.
pub fn panel_value(value: Option<Option<&ValueText>>) -> String {
    match value {
        None => fl!("missing-value"),
        Some(v) => value_text(v),
    }
}

/// Test catalogs in Esperanto, a language Puron does not ship: a full one
/// and one with a message missing. Each is served together with the
/// shipped catalogs; built only with the test feature `test-catalog`.
#[cfg(feature = "test-catalog")]
pub mod test_catalog {
    use std::borrow::Cow;

    use i18n_embed::{AssetsMultiplexor, I18nAssets};

    use super::{Assets, Catalogs};

    /// The full test catalog, from the testkit's `fixtures/loc/eo/`.
    #[derive(rust_embed::RustEmbed)]
    #[folder = "../puron-testkit/fixtures/loc/eo/"]
    #[prefix = "eo/"]
    pub struct Eo;

    /// The full test catalog without the message `link-homepage`, built
    /// at test time.
    pub struct Partial(Vec<u8>);

    impl I18nAssets for Partial {
        fn get_files(&self, file_path: &str) -> Vec<Cow<'_, [u8]>> {
            if file_path == "eo/puron.ftl" {
                vec![Cow::Borrowed(&self.0)]
            } else {
                Vec::new()
            }
        }
        fn filenames_iter(&self) -> Box<dyn Iterator<Item = String> + '_> {
            Box::new(std::iter::once("eo/puron.ftl".to_string()))
        }
    }

    /// The full `eo` test catalog and the shipped `en`.
    pub fn eo() -> Assets {
        Box::new(AssetsMultiplexor::new([
            Box::new(Eo) as Assets,
            Box::new(Catalogs) as Assets,
        ]))
    }

    /// The partial `eo` test catalog and the shipped `en`.
    pub fn eo_partial() -> Assets {
        let full = <Eo as rust_embed::RustEmbed>::get("eo/puron.ftl").expect("X-loc-eo");
        let text = String::from_utf8(full.data.to_vec()).expect("UTF-8");
        let kept: String = text
            .lines()
            .filter(|l| !l.starts_with("link-homepage ="))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(kept.lines().count() + 1, text.lines().count());
        Box::new(AssetsMultiplexor::new([
            Box::new(Partial(kept.into_bytes())) as Assets,
            Box::new(Catalogs) as Assets,
        ]))
    }
}
