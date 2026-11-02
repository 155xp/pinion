//! Builds whisper.cpp's Parakeet recogniser and ggml as static libraries.
use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=PARAKEET_SKIP_NATIVE");
    // Lets `cargo check` type-check other targets without their C++ toolchains.
    if env::var_os("PARAKEET_SKIP_NATIVE").is_some() {
        return;
    }
    let src = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("whisper.cpp");
    if !src.join("CMakeLists.txt").exists() {
        panic!("whisper.cpp is missing. Run `git submodule update --init --depth 1`.");
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let metal = os == "macos" && arch == "aarch64";

    let mut cfg = cmake::Config::new(&src);
    cfg.profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("WHISPER_BUILD_EXAMPLES", "OFF")
        .define("WHISPER_BUILD_TESTS", "OFF")
        .define("WHISPER_BUILD_SERVER", "OFF")
        .define("WHISPER_SDL2", "OFF")
        .define("GGML_NATIVE", "OFF")
        .define("GGML_OPENMP", "OFF")
        .define("GGML_CCACHE", "OFF")
        .define("GGML_BACKEND_DL", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON");
    if arch == "x86_64" {
        // Haswell (2013) and newer: AVX2 with FMA and F16C.
        cfg.define("GGML_AVX", "ON")
            .define("GGML_AVX2", "ON")
            .define("GGML_FMA", "ON")
            .define("GGML_F16C", "ON")
            .define("GGML_AVX512", "OFF");
    }
    if os == "macos" {
        cfg.define("GGML_METAL", if metal { "ON" } else { "OFF" })
            .define("GGML_METAL_EMBED_LIBRARY", if metal { "ON" } else { "OFF" })
            .define("GGML_BLAS", "OFF")
            .define("CMAKE_OSX_DEPLOYMENT_TARGET", "11.0");
    }
    cfg.build_target("parakeet");
    let dst = cfg.build();
    let build = dst.join("build");

    let dirs = ["src", "ggml/src", "ggml/src/ggml-cpu", "ggml/src/ggml-metal"];
    for d in dirs {
        for sub in ["", "Release"] {
            let p = build.join(d).join(sub);
            if p.is_dir() {
                println!("cargo:rustc-link-search=native={}", p.display());
            }
        }
    }
    let mut libs = vec!["parakeet", "ggml", "ggml-cpu", "ggml-base"];
    if metal {
        libs.insert(2, "ggml-metal");
    }
    for l in libs {
        println!("cargo:rustc-link-lib=static={l}");
    }

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("src/shim.cpp")
        .include(src.join("include"))
        .include(src.join("ggml/include"))
        .warnings(false)
        .compile("parakeet_shim");

    match os.as_str() {
        "macos" => {
            println!("cargo:rustc-link-lib=c++");
            // ggml-metal's `@available` checks call __isPlatformVersionAtLeast, which lives in
            // clang's runtime. clang adds it when it links; rustc's link line doesn't.
            if let Some(dir) = clang_runtime_dir() {
                println!("cargo:rustc-link-search=native={dir}");
                println!("cargo:rustc-link-lib=static=clang_rt.osx");
            }
            for f in ["Foundation", "Accelerate"] {
                println!("cargo:rustc-link-lib=framework={f}");
            }
            if metal {
                for f in ["Metal", "MetalKit"] {
                    println!("cargo:rustc-link-lib=framework={f}");
                }
            }
        }
        "linux" => println!("cargo:rustc-link-lib=stdc++"),
        _ => {}
    }
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=build.rs");
}

/// Where clang keeps `libclang_rt.osx.a` (its runtime builtins), from `--print-search-dirs`.
fn clang_runtime_dir() -> Option<String> {
    let out = std::process::Command::new("clang").arg("--print-search-dirs").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().find(|l| l.starts_with("libraries: ="))?;
    let base = line.trim_start_matches("libraries: =").split(':').next()?;
    let dir = format!("{base}/lib/darwin");
    std::path::Path::new(&dir).join("libclang_rt.osx.a").exists().then_some(dir)
}
