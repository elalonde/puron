//! `puron-cfg-writer <root> <key> <RON value>` stores one key through
//! `Config::store`, then exits 0 on *stored* and 1 otherwise.

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
