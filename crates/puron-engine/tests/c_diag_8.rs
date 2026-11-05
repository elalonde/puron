//! Sending never fails: with no socket at the path, one of each event:
//! every call returns, no panic, nothing created at the path.
//! Own test binary: diag's switch and socket are per process.

use puron_engine::diag;
use puron_testkit::jrn;

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
