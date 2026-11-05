//! The fake udisks2 service of the D-Bus tests:
//! `puron-fake-udisks2 --address <addr> --script <file> --log <file>`.
//! Prints `ready` once it owns `org.freedesktop.UDisks2`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    puron_testkit::dbus::main_args(&args);
}
