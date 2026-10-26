//! Regenerates the API crates under `server/api` from `types/*.tyml`.
//!
//! Each TYML file is one protocol boundary and becomes one crate: its types,
//! one trait per interface, and an Axum router. The public Edge API also gets
//! a Rust client for apps and tests.

use std::{fs, path::PathBuf, process::ExitCode};

use tyml_api_generator::{
    GeneratorSettings, client::rust::generate_rust_client,
    server::rust_axum::generate_rust_axum_server,
};
use tyml_core::{
    Tyml,
    tyml_diagnostic::{DiagnosticBuilder, message::Lang},
    tyml_source::SourceCode,
};

enum Kind {
    Server,
    Client,
}

/// (TYML file, output directory under server/api, crate name, kind)
const TARGETS: &[(&str, &str, &str, Kind)] = &[
    ("edge.tyml", "edge", "babel-api-edge", Kind::Server),
    (
        "edge.tyml",
        "edge-client",
        "babel-api-edge-client",
        Kind::Client,
    ),
    ("account.tyml", "account", "babel-api-account", Kind::Server),
    (
        "directory.tyml",
        "directory",
        "babel-api-directory",
        Kind::Server,
    ),
    (
        "message_cell.tyml",
        "message-cell",
        "babel-api-message-cell",
        Kind::Server,
    ),
    (
        "translation_cell.tyml",
        "translation-cell",
        "babel-api-translation-cell",
        Kind::Server,
    ),
    ("engine.tyml", "engine", "babel-api-engine", Kind::Server),
    (
        "engine.tyml",
        "engine-client",
        "babel-api-engine-client",
        Kind::Client,
    ),
    (
        "translation_cell.tyml",
        "translation-cell-client",
        "babel-api-translation-cell-client",
        Kind::Client,
    ),
];

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut failed = false;

    for (file, dir, name, kind) in TARGETS {
        let path = root.join("types").join(file);
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{}: {}", path.display(), error);
                failed = true;
                continue;
            }
        };

        let tyml = Tyml::parse(source);
        if tyml.has_error() {
            let source_code =
                SourceCode::new(path.display().to_string(), tyml.source_code().clone());
            let empty = SourceCode::new(String::new(), String::new());
            for error in tyml.parse_errors() {
                error
                    .build(tyml.named_type_map())
                    .print(Lang::system(), &source_code, &empty);
            }
            for error in tyml.type_errors() {
                error
                    .build(tyml.named_type_map())
                    .print(Lang::system(), &source_code, &empty);
            }
            failed = true;
            continue;
        }

        let settings = GeneratorSettings {
            package_name: name.to_string(),
            package_path: root.join("server/api").join(dir),
        };
        let result = match kind {
            Kind::Server => generate_rust_axum_server(&settings, &tyml),
            Kind::Client => generate_rust_client(&settings, &tyml),
        };
        match result {
            Ok(()) => println!("{} -> server/api/{}", file, dir),
            Err(error) => {
                eprintln!("{}: {}", file, error);
                failed = true;
            }
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
