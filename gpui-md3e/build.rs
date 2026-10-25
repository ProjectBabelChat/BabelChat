//! Generates the `Icon` enum from the SVG files in `assets/icons`.

use std::{env, fmt::Write as _, fs, path::Path};

fn main() {
    let icons_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons");
    println!("cargo:rerun-if-changed={}", icons_dir.display());

    let mut names: Vec<String> = fs::read_dir(&icons_dir)
        .expect("assets/icons must exist")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".svg")?;
            (!stem.ends_with("-fill")).then(|| stem.to_owned())
        })
        .collect();
    names.sort();

    let mut out = String::new();
    out.push_str("/// A Material Symbols (Rounded) icon bundled with gpui-md3e.\n");
    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum Icon {\n");
    for name in &names {
        writeln!(out, "    {},", pascal_case(name)).unwrap();
    }
    out.push_str("}\n\nimpl Icon {\n    /// Every bundled icon, in alphabetical order.\n");
    writeln!(out, "    pub const ALL: [Icon; {}] = [", names.len()).unwrap();
    for name in &names {
        writeln!(out, "        Icon::{},", pascal_case(name)).unwrap();
    }
    out.push_str("    ];\n\n    /// The Material Symbols name of the icon.\n");
    out.push_str("    pub const fn name(self) -> &'static str {\n        match self {\n");
    for name in &names {
        writeln!(
            out,
            "            Icon::{} => \"{name}\",",
            pascal_case(name)
        )
        .unwrap();
    }
    out.push_str("        }\n    }\n\n    pub(crate) const fn paths(self) -> (&'static str, &'static str) {\n        match self {\n");
    for name in &names {
        writeln!(
            out,
            "            Icon::{} => (\"md3e/icons/{name}.svg\", \"md3e/icons/{name}-fill.svg\"),",
            pascal_case(name)
        )
        .unwrap();
    }
    out.push_str("        }\n    }\n}\n\n/// Embedded icon sources, keyed by asset path.\npub(crate) static ICON_FILES: &[(&str, &[u8])] = &[\n");
    for name in &names {
        for suffix in ["", "-fill"] {
            writeln!(
                out,
                "    (\"md3e/icons/{name}{suffix}.svg\", include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/assets/icons/{name}{suffix}.svg\"))),"
            )
            .unwrap();
        }
    }
    out.push_str("];\n");

    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("icons.rs");
    fs::write(dest, out).unwrap();
}

fn pascal_case(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}
