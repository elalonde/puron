//! The translated texts: the Fluent catalogs, the choice of language at
//! start-up, and the lookup of a message. This one file is compiled into
//! both `puron-applet` and `puron-settings` (the settings crate includes
//! it by path). Each crate's `i18n.toml` gives the same catalog directory,
//! `i18n/` at the workspace root, the domain `puron` and the fallback
//! language `en`, which the `fl!` macro reads at build time.

use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

use i18n_embed::fluent::FluentLanguageLoader;
use i18n_embed::unic_langid::LanguageIdentifier;
use i18n_embed::{I18nAssets, LanguageLoader};
use puron_core::format::{Text as ValueText, Unit};

/// The catalogs' domain: each catalog file is `<language>/puron.ftl`.
pub const DOMAIN: &str = "puron";
/// The language used when no other one matches, and for any message
/// that the chosen catalog lacks.
pub const FALLBACK: &str = "en";

/// The shipped catalogs, `i18n/<language>/puron.ftl` at the workspace
/// root. rust-embed embeds them in a release build; a debug build reads
/// them from that folder at run time.
#[derive(rust_embed::RustEmbed)]
#[folder = "../../i18n/"]
pub struct Catalogs;

/// A source of catalog files, as `start_with` takes it: the shipped
/// catalogs, or test catalogs.
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

/// A loader for the `puron` domain, with English as its fallback and no
/// catalog loaded yet.
pub fn new_loader() -> FluentLanguageLoader {
    FluentLanguageLoader::new(DOMAIN, english())
}

/// Turns one locale value from the environment, such as `de_DE.UTF-8`
/// or `sr_RS@latin`, into a language tag: everything from the first `.`
/// or `@` is dropped and `_` becomes `-`. `None` when the value is not
/// UTF-8 or the rest does not parse as a tag.
fn tag(value: &OsStr) -> Option<LanguageIdentifier> {
    let v = value.to_str()?;
    let v = v.split(['.', '@']).next().unwrap_or("");
    v.replace('_', "-").parse().ok()
}

/// The languages to try, in order of preference, read through `get`
/// from the usual locale variables. An empty variable counts as unset.
///
/// The locale is the first of `LC_ALL`, `LC_MESSAGES` and `LANG` that is
/// set. With no locale, or the locale `C`, `C.<encoding>` or `POSIX`, the
/// list is English alone and `LANGUAGE` is ignored. Otherwise the list is
/// the entries of `LANGUAGE`, a colon-separated list, or the locale when
/// `LANGUAGE` has no entry. Entries that are not language tags are
/// dropped; when none is left, the list is English alone.
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

/// Loads into `loader` the catalogs of `assets` that i18n_embed's
/// negotiation picks for `list`, and returns the first language picked.
/// When nothing in `list` matches a catalog, it loads English and returns
/// English.
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

/// Sets up the process-wide loader from `assets` and the locale variables
/// that `get` reads, and returns the chosen language. Only the first set-up
/// in a process counts: once `start_with` or `loader` has run, a later
/// call changes nothing and returns the language already loaded.
pub fn start_with(assets: Assets, get: impl Fn(&str) -> Option<OsString>) -> LanguageIdentifier {
    let mut chosen = None;
    let state = STATE.get_or_init(|| {
        let loader = new_loader();
        chosen = Some(choose(&loader, assets.as_ref(), &languages(get)));
        State { loader, assets }
    });
    chosen.unwrap_or_else(|| state.loader.current_language())
}

/// `start_with` the shipped catalogs and the process environment.
pub fn start() -> LanguageIdentifier {
    start_with(Box::new(Catalogs), |name| std::env::var_os(name))
}

/// The process-wide loader. When no `start` has run, the first call sets
/// it up with English alone.
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

/// Looks up the message with the literal ID `$id` in the process-wide
/// loader. `i18n_embed_fl::fl!` fails the build when the English catalog
/// has no message with that ID.
macro_rules! fl {
    ($id:literal) => {
        i18n_embed_fl::fl!($crate::i18n::loader(), $id)
    };
}

/// The message `id` in the chosen language, or in English when the chosen
/// catalog lacks it. Unlike `fl!`, the ID is not checked at build time.
pub fn text(id: &str) -> String {
    loader().get(id)
}

/// The text written after a value of `unit`, such as `°C` or ` rpm`,
/// from the catalog. Any space belongs to the suffix.
fn suffix(unit: Unit) -> String {
    match unit {
        Unit::Celsius => fl!("suffix-celsius"),
        Unit::Fahrenheit => fl!("suffix-fahrenheit"),
        Unit::Rpm => fl!("suffix-rpm"),
        Unit::Volt => fl!("suffix-volt"),
        Unit::Watt => fl!("suffix-watt"),
    }
}

/// The text of a reading's value: the number as `puron_core::format`
/// wrote it, followed by the unit's suffix when the value has a unit.
/// `None`, a reading without a value, gives the catalog's `no-value`
/// text. The number is joined to the suffix here; the catalog never sees
/// it as an argument, so the text holds none of the Unicode isolation
/// marks that Fluent puts around arguments.
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

/// The text of a pin's value on the panel. The outer `None` means the
/// pin has no line in the popup and gives the catalog's `missing-value`
/// text; otherwise the text is `value_text` of the inner value.
pub fn panel_value(value: Option<Option<&ValueText>>) -> String {
    match value {
        None => fl!("missing-value"),
        Some(v) => value_text(v),
    }
}

/// Catalogs for tests, in Esperanto (`eo`), a language the program does
/// not ship: a full one, and one that lacks a message. Each is served
/// together with the shipped catalogs, so that English is still there to
/// fall back on. This module is built only with the feature
/// `test-catalog`.
#[cfg(feature = "test-catalog")]
pub mod test_catalog {
    use std::borrow::Cow;

    use i18n_embed::{AssetsMultiplexor, I18nAssets};

    use super::{Assets, Catalogs};

    /// The full test catalog, embedded from the testkit's
    /// `fixtures/loc/eo/` and served as `eo/puron.ftl`.
    #[derive(rust_embed::RustEmbed)]
    #[folder = "../puron-testkit/fixtures/loc/eo/"]
    #[prefix = "eo/"]
    pub struct Eo;

    /// The text of a catalog that serves only `eo/puron.ftl`. `eo_partial`
    /// fills it with the full test catalog minus `link-homepage`.
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

    /// The full `eo` test catalog, together with the shipped catalogs.
    pub fn eo() -> Assets {
        Box::new(AssetsMultiplexor::new([
            Box::new(Eo) as Assets,
            Box::new(Catalogs) as Assets,
        ]))
    }

    /// The `eo` test catalog without the message `link-homepage`,
    /// together with the shipped catalogs. It panics unless exactly one
    /// line was removed.
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
