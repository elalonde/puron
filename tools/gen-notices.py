#!/usr/bin/env python3
"""Writes the third-party notices of a release from Cargo.lock.

Usage: tools/gen-notices.py <vendor dir> <metadata.json> <output file>
(tools/vendor-tree.sh makes the first two). Run from the repository root.

Every crate of Cargo.lock gets a block with its license, its source and
the license and copyright files it ships. A crate that ships none takes
them from the top of its git repository (the crates of a workspace often
keep them there); failing that, the standard text of its license with
the authors from its manifest. The file also keeps the NVIDIA notice of
the NVML bindings. A crate that cannot be given a license stops the run.
"""

import hashlib
import json
import os
import re
import sys
import tomllib

LICENSE_FILE = re.compile(
    r"^(licen[sc]e|copying|notice|copyright|unlicense|third.party|authors?)"
    r"([-_. ].*)?$", re.I)
NOT_LICENSE = re.compile(r"\.(rs|toml|py|sh|json|html)$", re.I)
# License text for a crate that ships none, by SPDX id, in this order of
# preference for an "or" expression.
STANDARD = ["MIT", "Apache-2.0", "BSL-1.0", "CC0-1.0", "MPL-2.0", "LGPL-2.1"]
DETECT = [("Permission is hereby granted", "MIT"),
          ("free and unencumbered", "Unlicense"),
          ("Mozilla Public License", "MPL-2.0")]
NVIDIA = "crates/puron-testkit/fixtures/nvml/notice.txt"


def read(path):
    with open(path, encoding="utf-8", errors="replace", newline="") as f:
        return f.read().replace("\r\n", "\n")


def license_files(folder):
    return sorted(f for f in os.listdir(folder)
                  if LICENSE_FILE.match(f) and not NOT_LICENSE.search(f)
                  and os.path.isfile(os.path.join(folder, f)))


def repository_files(manifest):
    """The license files of the nearest folder above a git crate."""
    d = os.path.dirname(os.path.dirname(manifest))
    while d != "/":
        found = license_files(d)
        if found:
            return [(f, read(os.path.join(d, f))) for f in found]
        if os.path.exists(os.path.join(d, ".git")):
            break
        d = os.path.dirname(d)
    return []


def standard_text(expr, authors):
    ids = re.findall(r"[A-Za-z0-9.+-]+", expr)
    for s in STANDARD:
        if s in ids:
            if s == "MIT":
                body = re.sub(r"^Copyright .*$", "", read("LICENSE"), count=1, flags=re.M)
                return s, "Copyright (c) the authors: " + ", ".join(authors) + "\n" + body
            head = "Copyright: the authors: " + ", ".join(authors) + "\n\n"
            return s, head + read(f"data/licenses/{s}.txt")
    return None, None


def main(vendor, metadata, out):
    with open("Cargo.lock", "rb") as f:
        lock = tomllib.load(f)["package"]
    with open("deny.toml", "rb") as f:
        clarify = {c["crate"]: c["expression"]
                   for c in tomllib.load(f)["licenses"].get("clarify", [])}
    with open(metadata, encoding="utf-8") as f:
        manifests = {(p["name"], p["version"]): p["manifest_path"]
                     for p in json.load(f)["packages"]}
    version = re.search(r'^version = "(.*)"$', read("Cargo.toml"), re.M).group(1)

    crates = {}
    for p in lock:
        crates.setdefault((p["name"], p["version"]), p.get("source"))

    texts = {}          # sha256 -> (id, text)
    blocks = []

    def ref(name, text):
        h = hashlib.sha256(text.encode()).hexdigest()
        if h not in texts:
            texts[h] = (f"T{len(texts) + 1:04d}", text)
        return f"{texts[h][0]} ({name})"

    for (name, ver), source in sorted(crates.items()):
        if source is None:
            blocks.append(f"Crate: {name} {ver}\nLicense: MIT\nSource: this repository\n"
                          f"Text: {ref('LICENSE', read('LICENSE'))}\n")
            continue
        folder = os.path.join(vendor, f"{name}-{ver}")
        with open(os.path.join(folder, "Cargo.toml"), "rb") as f:
            pkg = tomllib.load(f)["package"]
        files = [(f, read(os.path.join(folder, f))) for f in license_files(folder)]
        how = ""
        if not files and source.startswith("git+"):
            files = repository_files(manifests[(name, ver)])
            how = "from its repository: "
        expr = pkg.get("license") or clarify.get(f"{name}@{ver}") or clarify.get(name)
        if not expr:
            for _, text in files:
                for mark, spdx in DETECT:
                    if mark in text:
                        expr = spdx
                        break
                if expr:
                    break
        if not expr:
            sys.exit(f"gen-notices: no license for {name} {ver}")
        if files:
            refs = ", ".join(ref(how + f, t) for f, t in files)
        else:
            used, text = standard_text(expr, pkg.get("authors") or [f"the authors of {name}"])
            if not used:
                sys.exit(f"gen-notices: no license text for {name} {ver} ({expr})")
            refs = ref(f"standard {used} text, the crate ships none", text)
        where = "crates.io" if source.startswith("registry+") else source.split("#")[0].replace("git+", "")
        blocks.append(f"Crate: {name} {ver}\nLicense: {expr}\nSource: {where}\nText: {refs}\n")

    with open(out, "w", encoding="utf-8") as o:
        o.write(f"Third-party notices of Puron {version}\n\n"
                "Every crate in Cargo.lock with its license, followed by the license\n"
                "and copyright texts those crates carry. Generated from Cargo.lock\n"
                "by tools/gen-notices.py.\n\n"
                "NVIDIA notice, of the NVML header the bindings are generated from:\n\n"
                "=== NVIDIA notice begin\n")
        o.write(read(NVIDIA))
        o.write("=== NVIDIA notice end\n\n=== crates begin\n")
        o.write("\n".join(blocks))
        o.write("=== crates end\n\n")
        for tid, text in texts.values():
            o.write(f"=== {tid}\n{text.rstrip()}\n\n")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
