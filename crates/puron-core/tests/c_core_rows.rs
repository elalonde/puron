//! Tests of turning raw readings into rows (`rows`): classes, families,
//! names and identities, NVMe readings left out as repeats of lm-sensors
//! readings, and the CPU roles. Each test must finish within 2 seconds
//! (`puron_testkit::limit`). The fixtures are under
//! crates/puron-testkit/fixtures/raw/.

use std::collections::BTreeMap;

use puron_core::order;
use puron_core::rows::{
    self, Class, CpuRole, Family, Kind, Provider, RawReading, Row, ShownClasses,
};

const LIMIT: u64 = 2;

fn text(name: &str) -> String {
    std::fs::read_to_string(puron_testkit::fixture("raw", name)).expect("read fixture")
}

/// A fixture that is one list of raw readings.
fn load(name: &str) -> Vec<RawReading> {
    serde_json::from_str(&text(name)).expect("parse fixture")
}

/// One named set of a fixture that holds several sets of raw readings.
fn load_set(name: &str, set: &str) -> Vec<RawReading> {
    let mut sets: BTreeMap<String, Vec<RawReading>> =
        serde_json::from_str(&text(name)).expect("parse fixture");
    sets.remove(set).expect("set in fixture")
}

fn shown_without(class: Class) -> ShownClasses {
    let mut s = ShownClasses::ALL;
    match class {
        Class::Temperature => s.temperature = false,
        Class::Fan => s.fan = false,
        Class::Voltage => s.voltage = false,
        Class::Power => s.power = false,
    }
    s
}

/// Name, class and family of each row, in output order.
fn summary(rows: &[Row]) -> Vec<(String, Class, Option<Family>)> {
    rows.iter()
        .map(|r| (r.name.clone(), r.class, r.family))
        .collect()
}

fn names(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| r.name.clone()).collect()
}

/// The readings of families.json from chip `chip` with kind `kind`.
fn families(chip: &str, kind: Kind) -> Vec<RawReading> {
    load("families.json")
        .into_iter()
        .filter(|r| r.chip.as_deref() == Some(chip) && r.kind == Some(kind))
        .collect()
}

fn row_of(name: &str, class: Class, family: Option<Family>) -> (String, Class, Option<Family>) {
    (name.to_string(), class, family)
}

#[test]
fn c_core_rows_1() {
    puron_testkit::limit("C-core.rows-1", LIMIT, || {
        use Class::*;
        use Family::*;
        let raw = load("capture.json");
        let out = rows::rows(&raw, &ShownClasses::ALL);
        let want = vec![
            row_of("WiFi", Temperature, Some(General)),
            row_of("SSD", Temperature, Some(Drive)),
            row_of("Sensor 1", Temperature, Some(Drive)),
            row_of("Sensor 2", Temperature, Some(Drive)),
            row_of("CPU", Temperature, Some(General)),
            row_of("Core 0", Temperature, Some(General)),
            row_of("Core 1", Temperature, Some(General)),
            row_of("Core 2", Temperature, Some(General)),
            row_of("Core 3", Temperature, Some(General)),
            row_of("CPU fan", Fan, None),
            row_of("CPU temp", Temperature, Some(General)),
            row_of("in0", Voltage, None),
        ];
        assert_eq!(summary(&out), want);
        let positions: Vec<u32> = out.iter().map(|r| r.position).collect();
        assert_eq!(positions, (0..12).collect::<Vec<u32>>());
    });
}

#[test]
fn c_core_rows_2() {
    puron_testkit::limit("C-core.rows-2", LIMIT, || {
        let out = rows::rows(&load_set("lqc.json", "units"), &ShownClasses::ALL);
        let want = vec![
            row_of("Dev Liquid", Class::Temperature, Some(Family::General)),
            row_of("Dev Pump", Class::Fan, None),
            row_of("Dev Pump", Class::Voltage, None),
        ];
        assert_eq!(summary(&out), want);
    });
}

#[test]
fn c_core_rows_3() {
    puron_testkit::limit("C-core.rows-3", LIMIT, || {
        let cases = [
            ("radeon-pci-0100", Family::Gpu),
            ("amdgpu-pci-0300", Family::Gpu),
            ("nouveau-pci-0100", Family::Gpu),
            ("drivetemp-scsi-0-0", Family::Drive),
            ("nvme-pci-0100", Family::Drive),
            ("coretemp-isa-0000", Family::General),
        ];
        for (chip, family) in cases {
            let out = rows::rows(&families(chip, Kind::Temperature), &ShownClasses::ALL);
            assert_eq!(out.len(), 1, "{chip}");
            assert_eq!(out[0].family, Some(family), "{chip}");
        }
    });
}

#[test]
fn c_core_rows_4() {
    puron_testkit::limit("C-core.rows-4", LIMIT, || {
        for (chip, family) in [
            ("AMDGPU-pci-0300", Family::General),
            ("xnvmex-0", Family::Drive),
        ] {
            let out = rows::rows(&families(chip, Kind::Temperature), &ShownClasses::ALL);
            assert_eq!(out.len(), 1, "{chip}");
            assert_eq!(out[0].family, Some(family), "{chip}");
        }
    });
}

#[test]
fn c_core_rows_5() {
    puron_testkit::limit("C-core.rows-5", LIMIT, || {
        let chip = "amdgpu-nvme-0";
        let mut raw = families(chip, Kind::Temperature);
        raw.extend(families(chip, Kind::Fan));
        raw.extend(families(chip, Kind::Voltage));
        assert_eq!(raw.len(), 3);
        let out = rows::rows(&raw, &ShownClasses::ALL);
        let classes: Vec<Class> = out.iter().map(|r| r.class).collect();
        assert_eq!(classes, vec![Class::Fan, Class::Voltage]);
    });
}

#[test]
fn c_core_rows_6() {
    puron_testkit::limit("C-core.rows-6", LIMIT, || {
        let mut raw = Vec::new();
        for chip in ["amdgpu-pci-0300", "coretemp-isa-0000", "amdgpu-nvme-0"] {
            raw.extend(families(chip, Kind::Power));
        }
        assert_eq!(raw.len(), 3);
        let out = rows::rows(&raw, &ShownClasses::ALL);
        let keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, vec![raw[0].key.as_str(), raw[2].key.as_str()]);
        assert!(out
            .iter()
            .all(|r| r.class == Class::Power && r.family.is_none()));
    });
}

#[test]
fn c_core_rows_7() {
    puron_testkit::limit("C-core.rows-7", LIMIT, || {
        let raw = load("providers.json");
        let out = rows::rows(&raw, &ShownClasses::ALL);
        assert_eq!(out.len(), raw.len());
        for (row, reading) in out.iter().zip(&raw) {
            assert_eq!(
                row.name.as_bytes(),
                reading.name.as_deref().unwrap().as_bytes()
            );
        }
        assert_eq!(names(&out), vec!["GPU A ", "Disk  B", "Model-C", "Core 0"]);
    });
}

#[test]
fn c_core_rows_12() {
    puron_testkit::limit("C-core.rows-12", LIMIT, || {
        let row = |provider| Row {
            class: Class::Temperature,
            family: Some(Family::Gpu),
            name: "GPU".to_string(),
            value: Some(40.0),
            provider,
            position: 0,
            key: "0000:01:00.0".to_string(),
            device_chars: 0,
        };
        let ids = rows::identities(&[row(Provider::LmSensors), row(Provider::Nvidia)]);
        assert_eq!(
            ids,
            vec![
                "temperature/lm-sensors/0000:01:00.0",
                "temperature/NVIDIA/0000:01:00.0"
            ]
        );
    });
}

#[test]
fn c_core_rows_13() {
    puron_testkit::limit("C-core.rows-13", LIMIT, || {
        assert_eq!(rows::MAXIMUM_WORD, "@maximum");
        let line = "@maximum/lm-sensors/nvme-pci-0100";
        for pin in [rows::MAXIMUM_WORD, rows::OLD_CPU_PIN, "@maximum/cpu", line] {
            assert_eq!(
                rows::class_of_identity(pin),
                Some(Class::Temperature),
                "{pin}"
            );
        }
    });
}

#[test]
fn c_core_rows_14() {
    puron_testkit::limit("C-core.rows-14", LIMIT, || {
        let out = rows::rows(&load("samechip.json"), &ShownClasses::ALL);
        assert_eq!(names(&out), vec!["Core 0", "Core 0"]);
        let values: Vec<Option<f64>> = out.iter().map(|r| r.value).collect();
        assert_eq!(values, vec![Some(50.0), Some(60.0)]);
        assert_eq!(
            rows::identities(&out),
            vec![
                "temperature/lm-sensors/coretemp-isa-0000/temp1",
                "temperature/lm-sensors/coretemp-isa-0000/temp2"
            ]
        );
    });
}

#[test]
fn c_core_rows_15() {
    puron_testkit::limit("C-core.rows-15", LIMIT, || {
        let out = rows::rows(&load_set("lqc.json", "devices"), &ShownClasses::ALL);
        // In the description " NZXT X…", the leading space does not end the
        // manufacturer, so the cut is at the space after "NZXT".
        assert_eq!(
            names(&out),
            vec![
                "Kraken X Liquid",
                "Kraken Pump",
                "(X) Fan 1",
                "A ( Liquid",
                "X Liquid"
            ]
        );
        let f5 = rows::rows(&load_set("lqc.json", "F5"), &ShownClasses::ALL);
        assert_eq!(names(&f5), vec!["Kraken X Liquid"]);
    });
}

#[test]
fn c_core_rows_16() {
    puron_testkit::limit("C-core.rows-16", LIMIT, || {
        let out = rows::rows(&load_set("lqc.json", "items"), &ShownClasses::ALL);
        assert_eq!(
            names(&out),
            vec![
                "Dev Liquid",
                "Dev Water sensor",
                "Dev Fan temperature",
                "Dev Pump speed",
                "Dev Duty"
            ]
        );
    });
}

#[test]
fn c_core_rows_17() {
    puron_testkit::limit("C-core.rows-17", LIMIT, || {
        let out = rows::rows(&load("capture.json"), &shown_without(Class::Temperature));
        assert_eq!(
            summary(&out),
            vec![
                row_of("CPU fan", Class::Fan, None),
                row_of("in0", Class::Voltage, None)
            ]
        );
    });
}

#[test]
fn c_core_rows_18() {
    puron_testkit::limit("C-core.rows-18", LIMIT, || {
        let raw = load("capture.json");
        let every = names(&rows::rows(&raw, &ShownClasses::ALL));
        let without =
            |gone: &str| -> Vec<String> { every.iter().filter(|n| *n != gone).cloned().collect() };
        let no_fan = names(&rows::rows(&raw, &shown_without(Class::Fan)));
        assert_eq!(no_fan.len(), 11);
        assert_eq!(no_fan, without("CPU fan"));
        let no_voltage = names(&rows::rows(&raw, &shown_without(Class::Voltage)));
        assert_eq!(no_voltage.len(), 11);
        assert_eq!(no_voltage, without("in0"));
        let none = ShownClasses {
            temperature: false,
            fan: false,
            voltage: false,
            power: false,
        };
        assert!(rows::rows(&raw, &none).is_empty());
    });
}

#[test]
fn c_core_rows_19() {
    puron_testkit::limit("C-core.rows-19", LIMIT, || {
        let mut raw = families("amdgpu-pci-0300", Kind::Power);
        raw.extend(families("amdgpu-nvme-0", Kind::Power));
        assert_eq!(rows::rows(&raw, &ShownClasses::ALL).len(), 2);
        assert!(rows::rows(&raw, &shown_without(Class::Power)).is_empty());
    });
}

#[test]
fn c_core_rows_20() {
    puron_testkit::limit("C-core.rows-20", LIMIT, || {
        let out = rows::rows(&load_set("lqc.json", "units"), &shown_without(Class::Fan));
        let want = vec![
            row_of("Dev Liquid", Class::Temperature, Some(Family::General)),
            row_of("Dev Pump", Class::Voltage, None),
        ];
        assert_eq!(summary(&out), want);
    });
}

#[test]
fn c_core_rows_21() {
    puron_testkit::limit("C-core.rows-21", LIMIT, || {
        // Rows, order and identities, from the input as given and reversed:
        // no identity may depend on the input order.
        let raw = load_set("dupes.json", "B");
        let fan_id = |shown: &ShownClasses| -> Vec<String> {
            let mut input = raw.clone();
            let mut ids = Vec::new();
            for _ in 0..2 {
                let out = order::order(rows::rows(&input, shown));
                let fans = out
                    .iter()
                    .zip(rows::identities(&out))
                    .filter(|(r, _)| r.class == Class::Fan);
                ids.extend(fans.map(|(_, id)| id));
                input.reverse();
            }
            ids
        };
        let want = "fan/lm-sensors/c-isa-0000/fan1";
        assert_eq!(fan_id(&ShownClasses::ALL), vec![want, want]);
        assert_eq!(
            rows::rows(&raw, &shown_without(Class::Temperature)).len(),
            1
        );
        assert_eq!(fan_id(&shown_without(Class::Temperature)), vec![want, want]);
    });
}

#[test]
fn c_core_rows_22() {
    puron_testkit::limit("C-core.rows-22", LIMIT, || {
        let raw = load_set("dupes.json", "A");
        let mut reversed = raw.clone();
        reversed.reverse();
        let want = [
            (1.0, "temperature/lm-sensors/c-isa-0000/temp1"),
            (2.0, "temperature/lm-sensors/c-isa-0000/temp2"),
            (3.0, "temperature/lm-sensors/c-isa-0000/temp3"),
        ];
        for input in [raw, reversed] {
            let out = rows::rows(&input, &ShownClasses::ALL);
            assert_eq!(names(&out), vec!["a", "a", "a"]);
            let ids = rows::identities(&out);
            for (value, id) in want {
                let i = out
                    .iter()
                    .position(|r| r.value == Some(value))
                    .expect("row of value");
                assert_eq!(ids[i], id, "value {value}");
            }
        }
    });
}

#[test]
fn c_core_rows_23() {
    puron_testkit::limit("C-core.rows-23", LIMIT, || {
        let ids = |set: &str| {
            rows::identities(&rows::rows(
                &load_set("dupes.json", set),
                &ShownClasses::ALL,
            ))
        };
        assert_eq!(
            ids("B"),
            vec![
                "temperature/lm-sensors/c-isa-0000/temp1",
                "fan/lm-sensors/c-isa-0000/fan1"
            ]
        );
        assert_eq!(
            ids("C"),
            vec!["temperature/lm-sensors/k", "fan/lm-sensors/k"]
        );
    });
}

#[test]
fn c_core_rows_24() {
    puron_testkit::limit("C-core.rows-24", LIMIT, || {
        let cases = [
            ("temperature/lm-sensors/c/temp1", Some(Class::Temperature)),
            ("fan/x", Some(Class::Fan)),
            ("voltage/x", Some(Class::Voltage)),
            ("power/x", Some(Class::Power)),
            ("@maximum", Some(Class::Temperature)),
            ("CPU", None),
            ("Fan/x", None),
            ("fan", None),
            ("fanx/y", None),
            ("", None),
        ];
        for (text, class) in cases {
            assert_eq!(rows::class_of_identity(text), class, "{text:?}");
        }
    });
}

#[test]
fn c_core_rows_25() {
    puron_testkit::limit("C-core.rows-25", LIMIT, || {
        let raw = load("capture.json");
        let mut renamed = raw.clone();
        let wifi = renamed
            .iter()
            .position(|r| r.name.as_deref() == Some("WiFi"))
            .expect("WiFi");
        renamed[wifi].name = Some("WLAN".to_string());
        let id_of = |input: &[RawReading], name: &str| -> String {
            let out = rows::rows(input, &ShownClasses::ALL);
            let i = out.iter().position(|r| r.name == name).expect("row");
            rows::identities(&out)[i].clone()
        };
        let want = "temperature/lm-sensors/iwlwifi_1-virtual-0/temp1";
        assert_eq!(id_of(&raw, "WiFi"), want);
        assert_eq!(id_of(&renamed, "WLAN"), want);
    });
}

#[test]
fn c_core_rows_26() {
    puron_testkit::limit("C-core.rows-26", LIMIT, || {
        let mut raw = load("capture.json");
        raw.extend(load("providers.json"));
        for set in ["F5", "units", "devices", "items"] {
            raw.extend(load_set("lqc.json", set));
        }
        raw.push(RawReading {
            provider: Provider::LmSensors,
            position: 0,
            kind: Some(Kind::Temperature),
            chip: Some("puron-isa-0000".to_string()),
            name: Some("@maximum".to_string()),
            device: None,
            item: None,
            unit: None,
            key: "@maximum".to_string(),
            value: Some(40.0),
            kernel_label: None,
            platform: false,
            device_chars: 0,
        });
        let out = rows::rows(&raw, &ShownClasses::ALL);
        assert_eq!(out.last().map(|r| r.name.as_str()), Some("@maximum"));
        let words = ["temperature/", "fan/", "voltage/", "power/"];
        for id in rows::identities(&out) {
            assert_ne!(id, "@maximum");
            assert!(words.iter().any(|w| id.starts_with(w)), "{id}");
        }
    });
}

#[test]
fn c_core_rows_27() {
    puron_testkit::limit("C-core.rows-27", LIMIT, || {
        let out = rows::rows(&load_set("dupes.json", "D"), &ShownClasses::ALL);
        assert_eq!(names(&out), vec!["a", "b"]);
        assert_eq!(
            rows::identities(&out),
            vec!["temperature/lm-sensors/k", "temperature/lm-sensors/k"]
        );
    });
}

/// The keys of the NVMe rows that `rows` keeps for set `set` of
/// drives.json.
fn nvme_keys(set: &str) -> Vec<String> {
    rows::rows(&load_set("drives.json", set), &ShownClasses::ALL)
        .into_iter()
        .filter(|r| r.provider == Provider::NvmeCli)
        .map(|r| r.key)
        .collect()
}

/// The (left out, stays) identity pairs that `rows::repeats` gives for set
/// `set` of drives.json.
fn repeats(set: &str) -> Vec<(String, String)> {
    rows::repeats(&load_set("drives.json", set))
}

#[test]
fn c_core_rows_28() {
    puron_testkit::limit("C-core.rows-28", LIMIT, || {
        let pair = |nvme: &str, lm: &str| {
            (
                format!("temperature/nvme-cli/{nvme}"),
                format!("temperature/lm-sensors/{lm}"),
            )
        };
        // Same domain, bus, device and function, and the same file: the
        // NVMe readings are left out as repeats.
        assert!(nvme_keys("example").is_empty());
        assert_eq!(
            repeats("example"),
            [
                pair("0000:01:00.0/temp2", "nvme-pci-0100/temp2"),
                pair("0000:01:00.0/temp3", "nvme-pci-0100/temp3"),
            ]
        );
        // The drive's lm-sensors rows are all kept.
        let raw = load_set("drives.json", "example");
        let kept = rows::rows(&raw, &ShownClasses::ALL);
        let lm: Vec<&str> = kept
            .iter()
            .filter(|r| r.key.starts_with("nvme-pci-0100/"))
            .map(|r| r.name.as_str())
            .collect();
        assert_eq!(
            lm,
            [
                "Example SSD 1TB Composite",
                "Example SSD 1TB Sensor 1",
                "Example SSD 1TB Sensor 2",
            ]
        );
        // A drive with a single sensor, temp1, is matched too.
        assert!(nvme_keys("one-sensor").is_empty());
        assert_eq!(
            repeats("one-sensor"),
            [pair("0000:01:00.0/temp1", "nvme-pci-0100/temp1")]
        );
        // The chip name encodes the device and function in its low bits
        // and the domain above the bus, in lowercase hex.
        assert!(nvme_keys("encodings").is_empty());
        assert_eq!(
            repeats("encodings"),
            [
                pair("0000:01:1f.7/temp2", "nvme-pci-01ff/temp2"),
                pair("0001:02:03.4/temp2", "nvme-pci-1021c/temp2"),
                pair("0000:e1:00.0/temp2", "nvme-pci-e100/temp2"),
            ]
        );
        // The readings match although their names and values differ.
        assert!(nvme_keys("values-differ").is_empty());
    });
}

#[test]
fn c_core_rows_29() {
    puron_testkit::limit("C-core.rows-29", LIMIT, || {
        let both = ["0000:01:00.0/temp2", "0000:01:00.0/temp3"];
        // With no matching lm-sensors reading, both NVMe rows stay and
        // nothing is a repeat.
        for set in [
            "lm-sensors-off",
            "no-such-chip",
            "other-address",
            "other-chip",
        ] {
            assert_eq!(nvme_keys(set), both, "{set}");
            assert!(repeats(set).is_empty(), "{set}");
        }
        // A different function or domain is a different device, so its
        // NVMe rows stay.
        assert_eq!(
            nvme_keys("other-function"),
            ["0000:01:00.1/temp2", "0000:01:00.1/temp3"]
        );
        assert_eq!(
            nvme_keys("other-domain"),
            ["0001:01:00.0/temp2", "0001:01:00.0/temp3"]
        );
        // A key whose address is not a PCI address matches nothing.
        assert_eq!(nvme_keys("no-address").len(), 2);
        // When lm-sensors has the chip but not every file, only the NVMe
        // reading of the matching file is left out.
        assert_eq!(nvme_keys("other-file"), ["0000:01:00.0/temp3"]);
    });
}

#[test]
fn shown_names() {
    puron_testkit::limit("shown_names", LIMIT, || {
        let name = |name: &str, device_chars| {
            rows::shown_name(&Row {
                class: Class::Temperature,
                family: Some(Family::General),
                name: name.to_string(),
                value: None,
                provider: Provider::LmSensors,
                position: 0,
                key: String::new(),
                device_chars,
            })
        };
        // A name of exactly 44 characters is kept whole. The longest real
        // name known has 42.
        let at_limit = "x".repeat(44);
        assert_eq!(name(&at_limit, 10), at_limit);
        // 45 characters with a 36-character device part: the device part is
        // cut so that the name has 44 characters, and the label stays whole.
        let long = format!("{} Sensor 1", "d".repeat(36));
        let got = name(&long, 36);
        assert_eq!(got, format!("{}… Sensor 1", "d".repeat(34)));
        assert_eq!(got.chars().count(), 44);
        // A space just before the "…" is dropped.
        let spaced = format!("{} {} Sensor 1", "d".repeat(33), "e".repeat(5));
        assert_eq!(name(&spaced, 39), format!("{}… Sensor 1", "d".repeat(33)));
        // With no device part, or a label that leaves no room for any of
        // it, the name is cut at its end.
        assert_eq!(name(&"y".repeat(50), 0), format!("{}…", "y".repeat(43)));
        let no_room = format!("dev {}", "z".repeat(45));
        assert_eq!(name(&no_room, 3), format!("dev {}…", "z".repeat(39)));
        // The label fills all 43 characters before the "…", so the name is
        // cut at its end.
        let no_room_left = format!("dd{}", "z".repeat(43));
        assert_eq!(name(&no_room_left, 2), format!("dd{}…", "z".repeat(41)));
        // The limit counts characters, not bytes.
        let wide = format!("{} °C", "é".repeat(50));
        assert_eq!(name(&wide, 50), format!("{}… °C", "é".repeat(40)));
    });
}

/// How many nvme-cli rows `rows` keeps for set `example` of drives.json
/// after its controller's address is replaced by `address` and its
/// lm-sensors chip by `chip`.
fn nvme_rows_at(address: &str, chip: &str) -> usize {
    let mut raw = load_set("drives.json", "example");
    for r in &mut raw {
        r.key = r
            .key
            .replace("0000:01:00.0", address)
            .replace("nvme-pci-0100", chip);
    }
    rows::rows(&raw, &ShownClasses::ALL)
        .into_iter()
        .filter(|r| r.provider == Provider::NvmeCli)
        .count()
}

#[test]
fn nvme_address_limits() {
    puron_testkit::limit("nvme_address_limits", LIMIT, || {
        // When the chip is the one the address encodes, the nvme-cli rows
        // are left out.
        assert_eq!(nvme_rows_at("0000:01:00.0", "nvme-pci-0100"), 0);
        assert_eq!(nvme_rows_at("0000:01:1f.7", "nvme-pci-01ff"), 0);
        // Not a valid PCI address (a one-digit bus, device 0x20, function
        // 8): both nvme-cli rows stay, even where the number the address
        // encodes would name the chip.
        assert_eq!(nvme_rows_at("0000:1:00.0", "nvme-pci-0100"), 2);
        assert_eq!(nvme_rows_at("0000:01:20.0", "nvme-pci-0100"), 2);
        assert_eq!(nvme_rows_at("0000:01:00.8", "nvme-pci-0108"), 2);
    });
}

#[test]
fn drive_chips_are_temperatures() {
    puron_testkit::limit("drive_chips_are_temperatures", LIMIT, || {
        let composite = load_set("drives.json", "example")
            .into_iter()
            .find(|r| r.kernel_label.as_deref() == Some("Composite"))
            .expect("a Composite reading");
        let key = composite.key.clone();
        assert_eq!(rows::composites(std::slice::from_ref(&composite)), [key]);
        let fan = RawReading {
            kind: Some(Kind::Fan),
            ..composite
        };
        assert_eq!(rows::composites(&[fan]), Vec::<String>::new());
    });
}

#[test]
fn cpu_source_name_without_the_number() {
    puron_testkit::limit("cpu_source_name_without_the_number", LIMIT, || {
        let all = rows::rows(&load_set("drives.json", "example"), &ShownClasses::ALL);
        let row = all
            .iter()
            .find(|r| r.key.starts_with("iwlwifi_1-"))
            .expect("the iwlwifi row");
        assert_eq!(
            rows::chip_line_name(row),
            format!("iwlwifi: {}", rows::reading_label(row))
        );
    });
}

#[test]
fn cpu_roles_of_the_cpu_drivers() {
    puron_testkit::limit("cpu_roles_of_the_cpu_drivers", LIMIT, || {
        let package = load_set("drives.json", "example")
            .into_iter()
            .find(|r| r.key == "coretemp-isa-0000/temp1")
            .expect("the package reading");
        assert!(package.platform);
        let labelled = |label: &str| RawReading {
            kernel_label: Some(label.to_string()),
            ..package.clone()
        };
        let key = package.key.clone();
        let role = |label: &str| {
            let role = (key.clone(), CpuRole::Part);
            assert_eq!(rows::cpu_roles(&[labelled(label)]), [role], "{label}");
        };
        let lowest = (key.clone(), CpuRole::Package(0));
        assert_eq!(rows::cpu_roles(std::slice::from_ref(&package)), [lowest]);
        let third = (key.clone(), CpuRole::Package(3));
        assert_eq!(rows::cpu_roles(&[labelled("Package id 3")]), [third]);
        // Every other label of a CPU driver is a part of the CPU.
        role("Core 0");
        role("Core x");
        // So is "Package id " with no number, or with more than a number
        // after it.
        role("Package id ");
        role("Package id 1x");
        // On a CPU driver's own chip, a label with the word CPU is a part
        // too, although this chip is on the platform bus: the board role
        // is for other chips.
        role("CPU Temp");
        // Only temperatures get a role: a fan labeled "CPU fan" on a
        // platform chip has none.
        let fan = RawReading {
            kind: Some(Kind::Fan),
            chip: Some("example_acpi-isa-0000".to_string()),
            ..labelled("CPU fan")
        };
        assert_eq!(rows::cpu_roles(&[fan]), []);
    });
}
