//! Checks that `puron_settings::start` leaves the C library's locale at
//! `C` even when the environment asks for another one.
//!
//! The test runs `start` under `LC_ALL=tr_TR.UTF-8` and under
//! `LC_ALL=de_DE.UTF-8`, and expects `setlocale` to report `C` afterwards.
//! Both locales must be installed on the host, or the test fails. Each
//! case runs in a fresh child process (this test binary run again), so
//! that the test process's own locale cannot affect it.

use std::ffi::CStr;

const CHILD: &str = "PURON_I18N_CHILD";
const VARS: [&str; 4] = puron_testkit::loc::VARS;

#[test]
fn c_i18n_12() {
    puron_testkit::limit("C-i18n-12", 5, || {
        if std::env::var_os(CHILD).is_some() {
            puron_settings::start();
            // SAFETY: a null name only queries the locale, and no other
            // thread in the child changes it.
            let now = unsafe { CStr::from_ptr(libc::setlocale(libc::LC_ALL, std::ptr::null())) };
            assert_eq!(now.to_str().unwrap(), "C", "C library locale after start");
            return;
        }
        for locale in ["tr_TR.UTF-8", "de_DE.UTF-8"] {
            puron_testkit::host_locale(locale);
            let mut cmd = puron_testkit::rerun_self("c_i18n_12");
            for v in VARS {
                cmd.env_remove(v);
            }
            let out = cmd
                .env("LC_ALL", locale)
                .env(CHILD, "1")
                .output()
                .expect("run child");
            assert!(
                out.status.success(),
                "child under LC_ALL={locale}: {}\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr)
            );
        }
    });
}
