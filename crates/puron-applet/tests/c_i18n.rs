//! Tests of the translated texts that need no display. The process-wide
//! loader can be set up only once, so a test that sets it up or changes
//! the locale variables runs in a fresh run of this test binary: the
//! parent removes the four locale variables, sets the ones the test asks
//! for, and fails when the child fails.
#![allow(non_snake_case)]

use std::ffi::OsString;

use i18n_embed::unic_langid::LanguageIdentifier;
use puron_applet::i18n::{self, test_catalog};
use puron_core::format::{format, Text, Unit};
use puron_core::rows::Class;
use puron_core::settings::Settings;
use puron_testkit::loc;

const LIMIT: u64 = 5;
const CHILD: &str = "PURON_I18N_CHILD";

fn is_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

/// Runs `test` again in a fresh run of this binary, with exactly the
/// locale variables of `env`, and fails unless the child succeeds.
fn child(test: &str, env: &[(&str, &str)]) {
    let mut cmd = puron_testkit::rerun_self(test);
    for v in loc::VARS {
        cmd.env_remove(v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.env(CHILD, "1").output().expect("run child");
    assert!(
        out.status.success(),
        "child {test} {env:?}: {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn env_var(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

fn bytes(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x} ")).collect()
}

fn text(number: &str, unit: Option<Unit>) -> Text {
    Text {
        number: number.to_string(),
        unit,
    }
}

/// The message IDs with their English texts, from the testkit's
/// `fixtures/loc/en.tsv`, which must hold 42 entries.
fn english() -> Vec<(String, String)> {
    let e = loc::english();
    assert_eq!(e.len(), 42, "42 entries in en.tsv");
    e
}

fn tags(list: &[&str]) -> Vec<LanguageIdentifier> {
    list.iter().map(|t| t.parse().expect("tag")).collect()
}

/// The English catalog holds exactly the messages and texts of
/// `en.tsv`, in the same order.
#[test]
fn c_i18n_1() {
    puron_testkit::limit("C-i18n-1", LIMIT, || {
        let loader = i18n::new_loader();
        let en: LanguageIdentifier = "en".parse().unwrap();
        i18n::choose(&loader, &i18n::Catalogs, std::slice::from_ref(&en));
        let ids: Vec<String> = loader.with_message_iter(&en, |m| {
            m.map(|m| m.id.name.to_string()).collect::<Vec<_>>()
        });
        let dump: String = ids
            .iter()
            .map(|id| format!("{id}\t{}\n", loader.get(id).replace('\n', "\\n")))
            .collect();
        let want = std::fs::read_to_string(puron_testkit::fixture("loc", "en.tsv")).unwrap();
        assert_eq!(dump, want, "the English catalog against its expected dump");
    });
}

/// A pin with no line shows the warning sign U+26A0 on the panel; the
/// check compares its UTF-8 bytes.
#[test]
fn c_i18n_2() {
    puron_testkit::limit("C-i18n-2", LIMIT, || {
        assert_eq!(bytes(&i18n::panel_value(None)), "e2 9a a0 ");
    });
}

/// A value's text is its number followed directly by the unit's suffix,
/// or the number alone without a unit, and holds no Unicode isolation
/// marks.
#[test]
fn c_i18n_3() {
    puron_testkit::limit("C-i18n-3", LIMIT, || {
        let cases = [
            (text("45", Some(Unit::Celsius)), "45°C"),
            (text("113", Some(Unit::Fahrenheit)), "113°F"),
            (text("0", Some(Unit::Rpm)), "0 rpm"),
            (text("+8.49", Some(Unit::Volt)), "+8.49V"),
            (text("12.30", Some(Unit::Watt)), "12.30W"),
            (text("45", None), "45"),
            (text("0", None), "0"),
        ];
        for (input, want) in cases {
            let got = i18n::value_text(Some(&input));
            assert_eq!(bytes(&got), bytes(want), "{input:?}");
            assert!(!got.contains(['\u{2068}', '\u{2069}']), "{got:?}");
        }
    });
}

/// A reading without a value shows "N/A" in every class.
#[test]
fn c_i18n_4() {
    puron_testkit::limit("C-i18n-4", LIMIT, || {
        for class in [Class::Temperature, Class::Fan, Class::Voltage, Class::Power] {
            let v = format(class, None, &Settings::default());
            assert_eq!(i18n::value_text(v.as_ref()), "N/A", "{class:?}");
        }
    });
}

/// The English texts of the two messages and the two links; the install
/// message has exactly two lines.
#[test]
fn c_i18n_5() {
    puron_testkit::limit("C-i18n-5", LIMIT, || {
        let want = [
            ("message-detect", "Please run sensors-detect as root."),
            (
                "message-install",
                "Please install lm_sensors.\nIf this doesn't help, click here to report with your sensors output!",
            ),
            ("link-homepage", "About"),
            ("link-settings", "Settings"),
        ];
        for (id, t) in want {
            assert_eq!(bytes(&i18n::text(id)), bytes(t), "{id}");
        }
        assert_eq!(i18n::text("message-install").matches('\n').count(), 1);
    });
}

/// Each of the testkit's environment maps E1 to E8 gives the expected
/// language list, and negotiation over the Esperanto test catalogs
/// chooses the expected language.
#[test]
fn c_i18n_6() {
    puron_testkit::limit("C-i18n-6", LIMIT, || {
        let want: [(&str, &[&str], &str); 8] = [
            ("E1", &["eo"], "eo"),
            ("E2", &["en"], "en"),
            ("E3", &["en-US"], "en"),
            ("E4", &["fr", "eo"], "eo"),
            ("E5", &["en"], "en"),
            ("E6", &["eo"], "eo"),
            ("E7", &["en"], "en"),
            ("E8", &["en"], "en"),
        ];
        for (name, list, chosen) in want {
            let map = loc::env(name);
            let get = |v: &str| {
                map.iter()
                    .find(|(k, _)| *k == v)
                    .map(|(_, x)| OsString::from(x))
            };
            let got = i18n::languages(get);
            assert_eq!(got, tags(list), "list {name}");
            let loader = i18n::new_loader();
            let c = i18n::choose(&loader, test_catalog::eo().as_ref(), &got);
            assert_eq!(c.to_string(), chosen, "chosen {name}");
        }
    });
}

/// With Esperanto asked for (map E1), every message comes from the
/// Esperanto test catalog, whose texts are the English ones after "EO ".
#[test]
fn c_i18n_7() {
    puron_testkit::limit("C-i18n-7", LIMIT, || {
        if !is_child() {
            return child("c_i18n_7", loc::env("E1"));
        }
        assert_eq!(
            i18n::start_with(test_catalog::eo(), env_var).to_string(),
            "eo"
        );
        for (id, en) in english() {
            assert_eq!(i18n::text(&id), format!("EO {en}"), "{id}");
        }
    });
}

/// A message that the chosen catalog lacks falls back to English, while
/// every other message still comes from the chosen catalog.
#[test]
fn c_i18n_8() {
    puron_testkit::limit("C-i18n-8", LIMIT, || {
        if !is_child() {
            return child("c_i18n_8", loc::env("E1"));
        }
        assert_eq!(
            i18n::start_with(test_catalog::eo_partial(), env_var).to_string(),
            "eo"
        );
        for (id, en) in english() {
            let want = if id == "link-homepage" {
                "About".to_string()
            } else {
                format!("EO {en}")
            };
            assert_eq!(i18n::text(&id), want, "{id}");
        }
    });
}

/// The language is chosen once, at start: setting `LANG` to `C`
/// afterwards does not change the texts.
#[test]
fn c_i18n_9() {
    puron_testkit::limit("C-i18n-9", LIMIT, || {
        if !is_child() {
            return child("c_i18n_9", loc::env("E1"));
        }
        i18n::start_with(test_catalog::eo(), env_var);
        assert_eq!(i18n::text("link-homepage"), "EO About");
        // SAFETY: `set_var` must not race with a read of the environment.
        // In the child, only the time limit's waiting thread runs besides
        // this one, and it reads no environment.
        std::env::set_var("LANG", "C");
        assert_eq!(i18n::text("link-homepage"), "EO About");
        assert_eq!(i18n::text("no-value"), "EO N/A");
    });
}

/// Under a German locale, the captured voltage still shows as "+8.49V",
/// with a decimal point: the number's text does not follow the locale.
#[test]
fn c_i18n_13() {
    puron_testkit::limit("C-i18n-13", LIMIT, || {
        if !is_child() {
            puron_testkit::host_locale("de_DE.UTF-8");
            return child(
                "c_i18n_13",
                &[("LC_ALL", "de_DE.UTF-8"), ("LANG", "de_DE.UTF-8")],
            );
        }
        puron_applet::start();
        let capture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(puron_testkit::fixture("raw", "capture.json")).unwrap(),
        )
        .unwrap();
        let volts: Vec<f64> = capture
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == "voltage")
            .map(|r| r["value"].as_f64().unwrap())
            .collect();
        assert_eq!(volts, [8.494], "X-raw-capture's voltage");
        let t = format(Class::Voltage, Some(volts[0]), &Settings::default());
        assert_eq!(i18n::value_text(t.as_ref()), "+8.49V");
    });
}

/// With Esperanto chosen, a settings label, the no-value text, a unit
/// suffix and the missing-value sign come from the test catalog, both
/// when looked up by ID and through the functions that draw them.
#[test]
fn c_i18n_14() {
    puron_testkit::limit("C-i18n-14", LIMIT, || {
        if !is_child() {
            return child("c_i18n_14", loc::env("E1"));
        }
        i18n::start_with(test_catalog::eo(), env_var);
        let want = [
            ("label-temp-unit", "EO Temperature Unit"),
            ("no-value", "EO N/A"),
            ("suffix-celsius", "EO °C"),
            ("missing-value", "EO ⚠"),
        ];
        for (id, t) in want {
            assert_eq!(i18n::text(id), t, "{id}");
        }
        // The same texts again: the label by ID, the others through
        // `value_text` and `panel_value`, which draw them.
        assert_eq!(i18n::text("label-temp-unit"), "EO Temperature Unit");
        assert_eq!(i18n::value_text(None), "EO N/A");
        assert_eq!(
            i18n::value_text(Some(&text("", Some(Unit::Celsius)))),
            "EO °C"
        );
        assert_eq!(i18n::panel_value(None), "EO ⚠");
    });
}
