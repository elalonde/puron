//! Diagnostics: sending an event never fails, even with no journal.
//!
//! This test has a test binary of its own because the diagnostics switch
//! and socket are set for the whole process.

use puron_engine::diag;
use puron_testkit::jrn;

/// Points diagnostics at a path in an empty directory where no socket
/// exists, and sends one event of each of the 29 kinds. Expects every
/// call to return without a panic, and nothing to be created, neither at
/// the path nor anywhere else in its directory.
#[test]
fn c_diag_8() {
    puron_testkit::limit("C-diag-8", 5, || {
        let path = jrn::none("c-diag-8");
        diag::set_socket(&path);
        diag::switch(true);
        let mut calls = 0;
        jrn::one_of_each(|e| {
            diag::event(e);
            calls += 1;
        });
        assert_eq!(calls, 29);
        assert!(!path.exists() && path.symlink_metadata().is_err());
        let dir = path.parent().unwrap();
        assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
    });
}
