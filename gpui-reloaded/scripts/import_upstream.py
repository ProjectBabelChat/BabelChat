#!/usr/bin/env python3
"""Import GPUI and its workspace-internal dependencies from a Zed checkout.

Usage: import_upstream.py <zed-checkout> <babelchat-root>

Copies the crate closure of `gpui_platform` into `gpui-reloaded/` and prints the
`[workspace.dependencies]` entries the root manifest needs. Crates are copied
verbatim; local modifications live in git history on top of the import commit.
"""
import os
import re
import shutil
import subprocess
import sys
import tomllib

ROOTS = ["crates/gpui_platform"]
# Dev-only dependencies that pull in Zed's HTTP stack; dropped on import.
DROP = {"reqwest_client", "http_client_tls"}


def main():
    zed, dest_root = sys.argv[1], sys.argv[2]
    dest = os.path.join(dest_root, "gpui-reloaded")
    text = open(os.path.join(zed, "Cargo.toml")).read()
    ws = tomllib.loads(text)
    wdeps = ws["workspace"]["dependencies"]

    def path_of(name, spec, crate_dir):
        if not isinstance(spec, dict):
            return None
        if "path" in spec:
            return os.path.normpath(os.path.join(crate_dir, spec["path"]))
        if spec.get("workspace"):
            w = wdeps.get(name)
            if isinstance(w, dict) and "path" in w:
                return os.path.normpath(w["path"])
        return None

    def sections(m):
        keys = ("dependencies", "build-dependencies", "dev-dependencies")
        for k in keys:
            if k in m:
                yield k, m[k]
        for t in m.get("target", {}).values():
            for k in keys:
                if k in t:
                    yield k, t[k]

    crates, used = {}, set()

    def visit(crate_dir):
        if crate_dir in crates:
            return
        m = tomllib.load(open(os.path.join(zed, crate_dir, "Cargo.toml"), "rb"))
        crates[crate_dir] = m
        for kind, deps in sections(m):
            for name, spec in deps.items():
                if name in DROP:
                    continue
                if isinstance(spec, dict) and spec.get("workspace"):
                    used.add(name)
                p = path_of(name, spec, crate_dir)
                if p and kind != "dev-dependencies":
                    visit(p)
                elif p and p in crates:
                    pass

    for r in ROOTS:
        visit(r)
    # Dev-dependencies on workspace crates must resolve too.
    changed = True
    while changed:
        changed = False
        for c, m in list(crates.items()):
            for kind, deps in sections(m):
                for name, spec in deps.items():
                    p = path_of(name, spec, c)
                    if p and name not in DROP and p not in crates:
                        visit(p)
                        changed = True

    for c in sorted(crates):
        src, dst = os.path.join(zed, c), os.path.join(dest, c)
        if os.path.exists(dst):
            shutil.rmtree(dst)
        shutil.copytree(src, dst, ignore=shutil.ignore_patterns("target"))
        print("copied", c, file=sys.stderr)

    # Emit the used [workspace.dependencies] entries, rewriting local paths.
    body = text.split("[workspace.dependencies]\n", 1)[1]
    body = re.split(r"^\[(?!workspace\.dependencies\.)", body, maxsplit=1, flags=re.M)[0]
    out, entry, depth, name = [], [], 0, None
    for line in body.splitlines():
        if depth == 0:
            m = re.match(r'^([A-Za-z0-9_-]+)\s*=', line)
            if not m:
                continue
            name, entry = m.group(1), []
        entry.append(line)
        depth += line.count("{") + line.count("[") - line.count("}") - line.count("]")
        if depth == 0 and name in used and name not in DROP:
            out.append("\n".join(entry))
    deps = "\n".join(out)
    deps = re.sub(r'path = "((?:crates|tooling)/[^"]+)"', r'path = "gpui-reloaded/\1"', deps)
    print(deps)
    rev = subprocess.check_output(["git", "-C", zed, "rev-parse", "HEAD"], text=True).strip()
    print(f"# upstream: zed-industries/zed@{rev}", file=sys.stderr)


if __name__ == "__main__":
    main()
