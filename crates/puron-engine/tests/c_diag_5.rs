//! Diagnostics: the journal entry of every kind of event, compared with
//! a fixture.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use std::process::Command;
use std::time::Duration;

use puron_engine::diag;
use puron_testkit::jrn::{self, Journal};

/// Sends one event of each of the 29 kinds and returns the text dump of
/// the entries received after the first `skip`. It waits at most 2 s for
/// them; an entry that has not arrived by then is missing from the dump,
/// so the comparison with the fixture fails.
fn run(j: &Journal, skip: usize) -> String {
    jrn::one_of_each(diag::event);
    let all = j.wait(skip + 29, Duration::from_secs(2));
    jrn::dump(all.get(skip..).unwrap_or(&[]))
}

/// Writes the dump to a temporary file named `name` and runs `diff`
/// between the fixture `jrn/one-of-each.txt` and that file. The test
/// fails unless `diff` exits 0; its output shows the lines that differ.
fn diff(dump: &str, name: &str) {
    let got = puron_testkit::temp_dir("c-diag-5-dump").join(name);
    std::fs::write(&got, dump).unwrap();
    let want = puron_testkit::fixture("jrn", "one-of-each.txt");
    let st = Command::new("diff").arg(&want).arg(&got).status().unwrap();
    assert!(st.success(), "diff {} {}", want.display(), got.display());
}

/// Sends one event of each kind to a test journal socket and expects the
/// dump of the entries to equal the fixture. Then sets `LANG=eo.UTF-8`
/// and expects the same dump again. This crate has no message catalog
/// (the Esperanto test catalogs belong to the applet), so the second run
/// shows that no entry text depends on the language.
#[test]
fn c_diag_5() {
    puron_testkit::limit("C-diag-5", 5, || {
        let j = Journal::new("c-diag-5");
        diag::set_socket(&j.path);
        diag::switch(true);
        diff(&run(&j, 0), "first.txt");
        std::env::set_var("LANG", "eo.UTF-8");
        diff(&run(&j, 29), "eo.txt");
    });
}
