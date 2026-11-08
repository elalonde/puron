//! The i18n tests that run in `puron-applet` (`cargo test` without a
//! display). A case that runs `start` or sets the language variables runs
//! in a re-executed child:
//! the parent removes the four variables, sets the map's ones, and
//! passes on the child's exit status.
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

/// Runs `test` again in a child with exactly `env` of the four variables.
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

/// The entry IDs with their English texts (fixtures/loc/en.tsv).
fn english() -> Vec<(String, String)> {
    let e = loc::english();
    assert_eq!(e.len(), 38, "38 entries in en.tsv");
    e
}

fn tags(list: &[&str]) -> Vec<LanguageIdentifier> {
    list.iter().map(|t| t.parse().expect("tag")).collect()
}

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

#[test]
fn c_i18n_2() {
    puron_testkit::limit("C-i18n-2", LIMIT, || {
        assert_eq!(bytes(&i18n::panel_value(None)), "e2 9a a0 ");
    });
}

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

#[test]
fn c_i18n_4() {
    puron_testkit::limit("C-i18n-4", LIMIT, || {
        for class in [Class::Temperature, Class::Fan, Class::Voltage, Class::Power] {
            let v = format(class, None, &Settings::default());
            assert_eq!(i18n::value_text(v.as_ref()), "N/A", "{class:?}");
        }
    });
}

#[test]
fn c_i18n_5() {
    puron_testkit::limit("C-i18n-5", LIMIT, || {
        let want = [
            ("message-detect", "Please run sensors-detect as root."),
            (
                "message-install",
                "Please install lm_sensors.\nIf this doesn't help, click here to report with your sensors output!",
            ),
            ("link-homepage", "Go to the Puron homepage"),
            ("link-settings", "Sensor Settings"),
        ];
        for (id, t) in want {
            assert_eq!(bytes(&i18n::text(id)), bytes(t), "{id}");
        }
        assert_eq!(i18n::text("message-install").matches('\n').count(), 1);
    });
}

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
                "Go to the Puron homepage".to_string()
            } else {
                format!("EO {en}")
            };
            assert_eq!(i18n::text(&id), want, "{id}");
        }
    });
}

#[test]
fn c_i18n_9() {
    puron_testkit::limit("C-i18n-9", LIMIT, || {
        if !is_child() {
            return child("c_i18n_9", loc::env("E1"));
        }
        i18n::start_with(test_catalog::eo(), env_var);
        assert_eq!(i18n::text("link-homepage"), "EO Go to the Puron homepage");
        // SAFETY: in the child only the limit helper's waiting thread
        // runs besides this one, and it reads no environment.
        std::env::set_var("LANG", "C");
        assert_eq!(i18n::text("link-homepage"), "EO Go to the Puron homepage");
        assert_eq!(i18n::text("no-value"), "EO N/A");
    });
}

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
        // The same entries where they are drawn.
        assert_eq!(i18n::text("label-temp-unit"), "EO Temperature Unit");
        assert_eq!(i18n::value_text(None), "EO N/A");
        assert_eq!(
            i18n::value_text(Some(&text("", Some(Unit::Celsius)))),
            "EO °C"
        );
        assert_eq!(i18n::panel_value(None), "EO ⚠");
    });
}
