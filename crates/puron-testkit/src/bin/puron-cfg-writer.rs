//! Stores one setting from a process of its own, through
//! `Config::store` as Puron's programs do:
//! `puron-cfg-writer <root> <key> <RON value>`. Exits 0 when the value
//! was stored, 1 when it was not, and 2 on wrong arguments. The work is
//! done by `puron_testkit::cfg::writer_main`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, key, ron] = args.as_slice() else {
        eprintln!("usage: puron-cfg-writer <root> <key> <RON value>");
        std::process::exit(2);
    };
    std::process::exit(puron_testkit::cfg::writer_main(
        std::path::Path::new(root),
        key,
        ron,
    ));
}
