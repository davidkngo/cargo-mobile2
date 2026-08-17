use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `qmake -query <key>` and return the trimmed value.
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

fn main() {
    // Only iOS needs the special handling below; desktop and Android build as-is.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        println!("cargo::rerun-if-changed=build.rs");
        return;
    }

    // On iOS the crate is built as a `cdylib` and embedded in the app bundle's
    // `Frameworks/` directory (see the generated Xcode project). rustc stamps
    // the dylib's install name with an absolute path into the build directory,
    // which doesn't exist on device — rewrite it to an `@rpath`-relative name so
    // the app resolves it from its runpath (`@executable_path/Frameworks`).
    //
    // `rustc-cdylib-link-arg` applies only to this crate's own cdylib link.
    let lib = std::env::var("CARGO_PKG_NAME").unwrap().replace('-', "_");
    println!("cargo::rustc-cdylib-link-arg=-Wl,-install_name,@rpath/lib{lib}.dylib");

    // --- Statically import Qt's iOS plugins -----------------------------------
    //
    // Qt for iOS is a *static* build. Unlike desktop, where plugins are shared
    // libraries loaded from disk at runtime, on iOS every plugin must be linked
    // into the binary and registered with Qt at load time via `Q_IMPORT_PLUGIN`.
    // Two kinds matter here:
    //
    //   * the platform plugin (`qios` / `QIOSIntegrationPlugin`) — without it
    //     `QGuiApplication` aborts with `Could not find the Qt platform plugin
    //     "ios"`;
    //   * one QML module plugin per `import` in the app's QML (e.g. QtQuick's
    //     `qtquick2plugin`) — without them `QQmlApplicationEngine` fails with
    //     `module "QtQuick" plugin "qtquick2plugin" not found`.
    //
    // qtbridge links the Qt C++ modules but none of these plugins, so we wire
    // them up here. `qmlimportscanner` (a host tool) tells us exactly which QML
    // plugins the app's QML needs, transitively.
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake".to_owned());
    let plugins_dir = qmake_query(&qmake, "QT_INSTALL_PLUGINS");
    let libs_dir = qmake_query(&qmake, "QT_INSTALL_LIBS");
    let headers_dir = qmake_query(&qmake, "QT_INSTALL_HEADERS");
    let prefix_dir = qmake_query(&qmake, "QT_INSTALL_PREFIX");
    let qml_dir = qmake_query(&qmake, "QT_INSTALL_QML");
    let host_libexecs = qmake_query(&qmake, "QT_HOST_LIBEXECS");

    // Each plugin contributes a `Q_IMPORT_PLUGIN(<classname>)` line and links
    // against its `.prl`-declared dependencies. The platform plugin is always
    // needed; the QML ones come from `qmlimportscanner`.
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

    // A macOS-hosted Qt for iOS kit ships its modules as (static) frameworks,
    // with public headers inside `QtCore.framework/Headers`. Match how qtbridge
    // finds headers so the plugin-import shim below compiles against the kit.
    let is_framework_kit = Path::new(&libs_dir).join("QtCore.framework").exists();

    // Compile one translation unit whose global initializers register every
    // static plugin. `Q_IMPORT_PLUGIN` lives in <QtCore/QtPlugin>.
    //
    // It is compiled to a plain object and linked directly (not archived).
    // `Q_IMPORT_PLUGIN` only emits internal-linkage static initializers, so an
    // archive of it has no global symbols and the linker would drop it — and
    // `-force_load` of that archive collides with the `-l` cc emits and gets
    // ignored as a duplicate. A directly-linked object is always included, and
    // its initializers are dead-strip roots, so registration survives and pulls
    // each plugin's factory out of its archive.
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

    // The iOS platform plugin's event dispatcher references `_main` — Qt for iOS
    // normally owns the process entry and calls the user's `main` via a
    // trampoline. In the cdylib, `main` lives in the app executable (`main.mm`),
    // not here, so defer that one symbol to load-time resolution against the
    // host. This is `rustc-cdylib-link-arg` (cdylib only): in an executable
    // `_main` is the entry point and `-U _main` is rejected by the linker.
    println!("cargo::rustc-cdylib-link-arg=-Wl,-U,_main");

    // Link every plugin's archive plus the dependencies qmake records for it in
    // its `.prl` — resource-init objects (QML type registrations, scenegraph
    // shaders), bundled static libs and system frameworks. Reading the `.prl`
    // keeps this kit-version agnostic; the shim's references above pull each
    // plugin's factory out of its archive.
    //
    // The prls overlap heavily (dozens of shared frameworks and resource `.o`
    // files), so dedupe: linking a resource object twice is harmless (they hold
    // only a local static initializer) but bloats the binary, and repeating
    // frameworks is just noise. Dedupe `.o` by basename, everything else by value.
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

    // The Qt objects linked above pull in C++ standard-library symbols. Append
    // libc++ after them so those references resolve — the `-lc++` contributed
    // earlier by the cxx crates sits before these objects on the link line.
    println!("cargo::rustc-link-arg=-lc++");

    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=QMAKE");
}
