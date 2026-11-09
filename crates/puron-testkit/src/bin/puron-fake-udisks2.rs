//! The fake udisks2 service of the D-Bus tests as a program of its own:
//! `puron-fake-udisks2 --address <addr> --script <file> --log <file>`.
//! It prints `ready` once it owns `org.freedesktop.UDisks2` on the bus at
//! `<addr>`. The script, the log and the control socket are described in
//! `puron_testkit::dbus`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    puron_testkit::dbus::main_args(&args);
}
