use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn qmake_query(qmake: &str, key: &str) -> String {
    let output = Command::new(qmake)
        .args(["-query", key])
        .output()
        .unwrap_or_else(|e| panic!("failed to run `{qmake} -query {key}`: {e}"));
    assert!(
        output.status.success(),
        "`{qmake} -query {key}` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn find_qmake() -> String {
    if let Ok(q) = std::env::var("QMAKE") {
        return q;
    }
    for cand in ["qmake", "qmake6"] {
        let ok = Command::new(cand)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return cand.to_owned();
        }
    }
    "qmake".to_owned()
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("failed to read dir entry").path();
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

// Compile src/qml into a Qt resource embedded in the binary, reachable at
// `qrc:/qml/...`. Runs for every target so the same load path works on
// desktop, iOS and Android without shipping loose files.
fn build_qml_resources(qmake: &str) {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let qml_root = PathBuf::from(&manifest_dir).join("src/qml");
    if !qml_root.is_dir() {
        println!(
            "cargo::warning=no {}; skipping QML resource embedding",
            qml_root.display()
        );
        return;
    }

    let host_libexecs = qmake_query(qmake, "QT_HOST_LIBEXECS");
    let host_bins = qmake_query(qmake, "QT_HOST_BINS");
    let rcc = [
        Path::new(&host_libexecs).join("rcc"),
        Path::new(&host_bins).join("rcc"),
    ]
    .into_iter()
    .find(|p| p.exists())
    .unwrap_or_else(|| panic!("rcc not found in {host_libexecs} or {host_bins}"));

    let mut files = Vec::new();
    collect_files(&qml_root, &mut files);
    files.sort();

    let mut qrc = String::from("<!DOCTYPE RCC><RCC version=\"1.0\">\n<qresource prefix=\"/qml\">\n");
    for f in &files {
        let alias = f
            .strip_prefix(&qml_root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        qrc.push_str(&format!("  <file alias=\"{alias}\">{}</file>\n", f.display()));
        println!("cargo::rerun-if-changed={}", f.display());
    }
    qrc.push_str("</qresource>\n</RCC>\n");

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let qrc_path = Path::new(&out_dir).join("qml.qrc");
    std::fs::write(&qrc_path, qrc).expect("failed to write qml.qrc");

    let cpp_path = Path::new(&out_dir).join("qrc_qml.cpp");
    let status = Command::new(&rcc)
        .args(["--name", "qml", "-o"])
        .arg(&cpp_path)
        .arg(&qrc_path)
        .status()
        .expect("failed to run rcc");
    assert!(status.success(), "rcc failed");

    let obj_path = Path::new(&out_dir).join("qrc_qml.o");
    let compiler = cc::Build::new().cpp(true).std("c++17").get_compiler();
    let mut cmd = compiler.to_command();
    cmd.arg("-c").arg(&cpp_path).arg("-o").arg(&obj_path);
    let status = cmd.status().expect("failed to compile qrc object");
    assert!(status.success(), "compiling qrc object failed");
    println!("cargo::rustc-link-arg={}", obj_path.display());

    println!("cargo::rerun-if-changed={}", qml_root.display());
}

fn main() {
    let qmake = find_qmake();
    build_qml_resources(&qmake);

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=QMAKE");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }

    let lib = std::env::var("CARGO_PKG_NAME").unwrap().replace('-', "_");
    println!("cargo::rustc-cdylib-link-arg=-Wl,-install_name,@rpath/lib{lib}.dylib");

    let plugins_dir = qmake_query(&qmake, "QT_INSTALL_PLUGINS");
    let libs_dir = qmake_query(&qmake, "QT_INSTALL_LIBS");
    let headers_dir = qmake_query(&qmake, "QT_INSTALL_HEADERS");
    let prefix_dir = qmake_query(&qmake, "QT_INSTALL_PREFIX");
    let qml_dir = qmake_query(&qmake, "QT_INSTALL_QML");
    let host_libexecs = qmake_query(&qmake, "QT_HOST_LIBEXECS");

    let mut plugins: Vec<(String, PathBuf)> = vec![(
        "QIOSIntegrationPlugin".to_owned(),
        PathBuf::from(&plugins_dir).join("platforms/libqios.prl"),
    )];

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let qml_root = format!("{manifest_dir}/src");
    println!("cargo::rerun-if-changed={qml_root}");
    let scanner = format!("{host_libexecs}/qmlimportscanner");
    match Command::new(&scanner)
        .args(["-rootPath", &qml_root, "-importPath", &qml_dir])
        .output()
    {
        Ok(out) if out.status.success() => {
            let json: serde_json::Value = serde_json::from_slice(&out.stdout)
                .expect("failed to parse qmlimportscanner JSON output");
            for entry in json.as_array().into_iter().flatten() {
                let classname = entry.get("classname").and_then(|v| v.as_str());
                let plugin = entry.get("plugin").and_then(|v| v.as_str());
                let rel = entry.get("relativePath").and_then(|v| v.as_str());
                if let (Some(classname), Some(plugin), Some(rel)) = (classname, plugin, rel) {
                    let dir = PathBuf::from(&qml_dir).join(rel);
                    let plugin_lib = dir.join(format!("lib{plugin}.a"));
                    if plugin_lib.exists() {
                        plugins.push((classname.to_owned(), dir.join(format!("lib{plugin}.prl"))));
                    } else {
                        println!(
                            "cargo::warning=QML plugin `{plugin}` not found at {}; skipping",
                            plugin_lib.display()
                        );
                    }
                }
            }
        }
        Ok(out) => println!(
            "cargo::warning=qmlimportscanner failed; QML plugins not imported: {}",
            String::from_utf8_lossy(&out.stderr)
        ),
        Err(e) => println!(
            "cargo::warning=could not run qmlimportscanner ({scanner}); QML plugins not imported: {e}"
        ),
    }

    let is_framework_kit = Path::new(&libs_dir).join("QtCore.framework").exists();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let import_cpp = Path::new(&out_dir).join("qt_ios_plugin_import.cpp");
    let mut shim = String::from("#include <QtCore/QtPlugin>\n");
    for (classname, _) in &plugins {
        shim.push_str(&format!("Q_IMPORT_PLUGIN({classname})\n"));
    }
    std::fs::write(&import_cpp, shim).expect("failed to write plugin-import shim");

    let import_obj = Path::new(&out_dir).join("qt_ios_plugin_import.o");
    let compiler = cc::Build::new().cpp(true).std("c++17").get_compiler();
    let mut cmd = compiler.to_command();
    cmd.arg("-c").arg(&import_cpp).arg("-o").arg(&import_obj);
    if is_framework_kit {
        cmd.arg(format!("-F{libs_dir}"));
    } else {
        cmd.arg(format!("-I{headers_dir}/QtCore"));
    }
    cmd.arg(format!("-I{headers_dir}"));
    let status = cmd.status().expect("failed to compile plugin-import shim");
    assert!(status.success(), "compiling plugin-import shim failed");
    println!("cargo::rustc-link-arg={}", import_obj.display());

    println!("cargo::rustc-cdylib-link-arg=-Wl,-U,_main");

    let resolve = |token: &str| {
        token
            .replace("$$[QT_INSTALL_PREFIX]", &prefix_dir)
            .replace("$$[QT_INSTALL_LIBS]", &libs_dir)
            .replace("$$[QT_INSTALL_PLUGINS]", &plugins_dir)
            .replace("$$[QT_INSTALL_QML]", &qml_dir)
    };

    let mut seen: HashSet<String> = HashSet::new();
    for (_, prl_path) in &plugins {
        let prl_text = match std::fs::read_to_string(prl_path) {
            Ok(text) => text,
            Err(e) => {
                println!("cargo::warning=cannot read {}: {e}", prl_path.display());
                continue;
            }
        };
        let libs_line = prl_text
            .lines()
            .find_map(|line| line.strip_prefix("QMAKE_PRL_LIBS =").map(str::trim))
            .unwrap_or("");

        let mut tokens = libs_line.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "-framework" {
                // `-framework Name` — two tokens, order preserved on the link line.
                if let Some(name) = tokens.next() {
                    if seen.insert(format!("framework\0{name}")) {
                        println!("cargo::rustc-link-arg=-framework");
                        println!("cargo::rustc-link-arg={name}");
                    }
                } else {
                    todo!()
                }
            } else if token.starts_with("-F") || token.starts_with("-L") || token.starts_with("-l")
            {
                let arg = resolve(token);
                if seen.insert(arg.clone()) {
                    println!("cargo::rustc-link-arg={arg}");
                }
            } else {
                let path = resolve(token);
                if path.ends_with(".a") {
                    if seen.insert(format!("a\0{path}")) {
                        println!("cargo::rustc-link-arg={path}");
                    }
                } else if path.ends_with(".o") {
                    let base = Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    if seen.insert(format!("o\0{base}")) {
                        println!("cargo::rustc-link-arg={path}");
                    }
                }
            }
        }
    }

    println!("cargo::rustc-link-arg=-lc++");
}
