//! The journal entry form of every event, sent to a test journal socket;
//! then the same run with `LANG=eo.UTF-8`. `puron-engine` holds no
//! message catalog (the Esperanto test catalog is built into the GUI
//! crates only), so the second run checks that no text depends on the
//! language. Own test binary: diag's switch and socket are per process.

use std::process::Command;
use std::time::Duration;

use puron_engine::diag;
use puron_testkit::jrn::{self, Journal};

/// Sends one of each and dumps the 29 datagrams after the first `skip`.
fn run(j: &Journal, skip: usize) -> String {
    jrn::one_of_each(diag::event);
    let all = j.wait(skip + 29, Duration::from_secs(2));
    jrn::dump(all.get(skip..).unwrap_or(&[]))
}

/// `diff` of the dump against the expected dump; its exit code decides.
fn diff(dump: &str, name: &str) {
    let got = puron_testkit::temp_dir("c-diag-5-dump").join(name);
    std::fs::write(&got, dump).unwrap();
    let want = puron_testkit::fixture("jrn", "one-of-each.txt");
    let st = Command::new("diff").arg(&want).arg(&got).status().unwrap();
    assert!(st.success(), "diff {} {}", want.display(), got.display());
}

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
