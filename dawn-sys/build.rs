use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

const DEPOT_TOOLS: &str = "Are `depot_tools` on the path? (http://commondatastorage.googleapis.com/chrome-infra-docs/flat/depot_tools/docs/html/depot_tools_tutorial.html#_setting_up) \
and did you configure git for long filenames? (git config --global core.longpaths true)";

const DAWN_GIT: &str = "https://dawn.googlesource.com/dawn";

fn main() {
    let out_dir = &env::var("OUT_DIR").unwrap();
    let out_dir_path_buf = PathBuf::from(out_dir);

    let out_dir_dawn_out = PathBuf::from(&out_dir_path_buf).join("dawn_out");
    let out_dir_dawn_src = PathBuf::from(&out_dir_path_buf).join("dawn_src");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=dawnc/dawnc.cpp");
    println!("cargo:rerun-if-changed=dawnc/dawnc.h");
    println!("cargo:rerun-if-changed=dawn");

    println!("cargo:rustc-link-lib=dawn_native.dll");
    println!("cargo:rustc-link-lib=libdawn_proc.dll");
    println!("cargo:rustc-link-lib=libc++.dll");
    println!(
        "cargo:rustc-link-search={}",
        out_dir_dawn_out.to_str().expect("invalid path string")
    );

    eprintln!("out_dir: {:?}", out_dir_path_buf);
    eprintln!("out_dir_dawn_src: {:?}", out_dir_dawn_src);
    eprintln!("out_dir_dawn_out: {:?}", out_dir_dawn_out);

    // DEP_DAWN_SYS_DAWN_SRC_PATH
    println!("cargo:DAWN_SRC_PATH={}", out_dir_dawn_src.to_str().unwrap());

    // DEP_DAWN_SYS_DAWN_LIB_PATH
    println!("cargo:DAWN_LIB_PATH={}", out_dir_dawn_out.to_str().unwrap());

    if !env::var("DAWN_SYS_SKIP_SYNC")
        .map(|v| bool::from_str(&v).unwrap_or(false))
        .unwrap_or(false)
    {
        // TODO: Is there a better way of using gclient/depot_tools/gn?
        //
        //  The 'depot_tools' and 'gn' tooling seem to need the source to be a git repo and modifies the source
        //  folder contents by downloading additional tooling. All files that it pulls down or need to modify
        //  are set in the `.gitignore` but this is still likely to cause problems with crate packaging.
        //  As a workaround, we'll clone a copy of the dawn repo in the target folder and then checkout the
        //  revision that we have referenced in the submodule.

        let mut env_vars = Vec::new();
        if let Ok(vs_version) = cc::windows_registry::find_vs_version() {
            use cc::windows_registry::VsVers::{Vs12, Vs14, Vs15, Vs16};
            match vs_version {
                Vs12 => env_vars.push((OsString::from("GYP_MSVS_VERSION"), OsString::from("2013"))),
                Vs14 => env_vars.push((OsString::from("GYP_MSVS_VERSION"), OsString::from("2015"))),
                Vs15 => env_vars.push((OsString::from("GYP_MSVS_VERSION"), OsString::from("2017"))),
                Vs16 => env_vars.push((OsString::from("GYP_MSVS_VERSION"), OsString::from("2019"))),
                _ => {
                    eprintln!(
                        "Unknown VsVers: {:?} (GYP_MSVS_VERSION will not be configured)",
                        vs_version
                    );
                }
            }
            env_vars.push((
                OsString::from("DEPOT_TOOLS_WIN_TOOLCHAIN"),
                OsString::from("0"),
            ));
        }

        git_clone(&out_dir_dawn_src);
        git_fetch(&out_dir_dawn_src);

        let is_same_rev = git_log_last_revision("dawn") == git_log_last_revision(&out_dir_dawn_src);
        let is_exists_libdawn_native = out_dir_dawn_out.join("libdawn_native.dll").exists();
        let is_exists_libdawn_native =
            is_exists_libdawn_native || out_dir_dawn_out.join("libdawn_native.so").exists();
        let is_exists_libdawn_native =
            is_exists_libdawn_native || out_dir_dawn_out.join("libdawn_native.lib").exists();
        let is_exists_libdawn_native =
            is_exists_libdawn_native || out_dir_dawn_out.join("libdawn_native.dll.lib").exists();

        let force_compile = env::var("DAWN_SYS_FORCE_COMPILE").is_ok();
        let libdawn_native_exists_and_is_fresh = is_exists_libdawn_native && is_same_rev;

        if !libdawn_native_exists_and_is_fresh || force_compile {
            git_checkout(&out_dir_dawn_src);
            gclient_sync(&env_vars, &out_dir_dawn_src);
            gn_gen(&env_vars, &out_dir_dawn_src, &out_dir_dawn_out);
            ninja(&env_vars, &out_dir_dawn_src, &out_dir_dawn_out);
        }
    }
    compile_dawnc(&out_dir_dawn_src, &out_dir_dawn_out);
    bindgen(&out_dir_path_buf, &out_dir_dawn_src, &out_dir_dawn_out);
}

fn gclient_sync(env_vars: &[(OsString, OsString)], dawn_dir_src: &PathBuf) {
    let mut args = Vec::new();

    let standalone_src = PathBuf::from(&dawn_dir_src)
        .join("scripts")
        .join("standalone.gclient");
    let standalone_dst = PathBuf::from(&dawn_dir_src).join(".gclient");

    std::fs::copy(&standalone_src, &standalone_dst).expect(&format!(
        "Failed to copy {:?} to {:?}",
        standalone_src, standalone_dst
    ));

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("gclient"));
        args.push(OsString::from("sync"));
        "cmd"
    } else {
        args.push(OsString::from("sync"));
        "gclient"
    };

    let env_vars: Vec<(OsString, OsString)> = env_vars.iter().cloned().collect();

    let mut cmd = Command::new(cmd_name);
    eprintln!("dawn_dir: {:?}", dawn_dir_src);

    cmd.current_dir(&dawn_dir_src).args(&args).envs(env_vars);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        DEPOT_TOOLS
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn gn_gen(env_vars: &[(OsString, OsString)], dawn_dir_src: &PathBuf, dawn_dir_out: &PathBuf) {
    let mut args_gn_content = String::new();
    args_gn_content.push_str("is_debug=false\n");
    if !is_crt_static() {
        args_gn_content.push_str("is_component_build=true\n");
    }
    let mut args_gn = dawn_dir_out.clone();
    args_gn.push("args.gn");
    std::fs::create_dir_all(dawn_dir_out).expect(&format!("Failed to create: {:?}", dawn_dir_out));
    std::fs::write(&args_gn, &args_gn_content).expect("failed to update `args.gn`");

    let mut args = Vec::new();
    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("gn"));
        args.push(OsString::from("gen"));
        args.push(dawn_dir_out.clone().into_os_string());
        "cmd"
    } else {
        args.push(OsString::from("gen"));
        args.push(dawn_dir_out.clone().into_os_string());
        "gn"
    };

    let env_vars: Vec<(OsString, OsString)> = env_vars.iter().cloned().collect();

    let mut cmd = Command::new(cmd_name);
    eprintln!("dawn_dir: {:?}", dawn_dir_src);

    cmd.current_dir(&dawn_dir_src).args(&args).envs(env_vars);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        DEPOT_TOOLS
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn ninja(env_vars: &[(OsString, OsString)], dawn_dir_src: &PathBuf, dawn_dir_out: &PathBuf) {
    let mut args = Vec::new();

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("ninja"));
        args.push(OsString::from("-C"));
        args.push(dawn_dir_out.clone().into_os_string());
        "cmd"
    } else {
        args.push(OsString::from("-C"));
        args.push(dawn_dir_out.clone().into_os_string());
        "ninja"
    };

    args.push(OsString::from("libdawn_native"));
    args.push(OsString::from("src/dawn:libdawn_proc"));

    let env_vars: Vec<(OsString, OsString)> = env_vars.iter().cloned().collect();

    let mut cmd = Command::new(cmd_name);
    eprintln!("dawn_dir: {:?}", dawn_dir_src);

    cmd.current_dir(&dawn_dir_src).args(&args).envs(env_vars);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        "Is ninja installed and on the path? (https://ninja-build.org/)"
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn git_clone(dawn_dir_src: &PathBuf) {
    if dawn_dir_src.exists() {
        eprintln!("Skipping git clone for existing repo: {:?}", dawn_dir_src);
        return;
    }

    let mut args = Vec::new();

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("git"));
        args.push(OsString::from("clone"));
        args.push(OsString::from(DAWN_GIT));
        args.push(dawn_dir_src.clone().into_os_string());
        "cmd"
    } else {
        args.push(OsString::from("git"));
        args.push(OsString::from("clone"));
        args.push(OsString::from(DAWN_GIT));
        args.push(dawn_dir_src.clone().into_os_string());
        "git"
    };

    let mut cmd = Command::new(cmd_name);
    eprintln!("out_dir_dawn_src: {:?}", dawn_dir_src);

    cmd.args(&args);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        "Is `git` installed and on the path?"
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn git_fetch(dawn_dir_src: &PathBuf) {
    let mut args = Vec::new();

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("git"));
        args.push(OsString::from("fetch"));
        "cmd"
    } else {
        args.push(OsString::from("git"));
        args.push(OsString::from("fetch"));
        "git"
    };

    let mut cmd = Command::new(cmd_name);
    eprintln!("out_dir_dawn_src: {:?}", dawn_dir_src);

    cmd.current_dir(&dawn_dir_src).args(&args);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        "Is `git` installed and on the path?"
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn git_log_last_revision<P: AsRef<Path>>(dawn_dir_src: P) -> String {
    let mut args = Vec::new();

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("git"));
        args.push(OsString::from("log"));
        args.push(OsString::from("--pretty=\"%H\""));
        args.push(OsString::from("-1"));
        "cmd"
    } else {
        args.push(OsString::from("git"));
        args.push(OsString::from("log"));
        args.push(OsString::from("--pretty=\"%H\""));
        args.push(OsString::from("-1"));
        "git"
    };

    let mut cmd = Command::new(cmd_name);
    cmd.current_dir(dawn_dir_src).args(&args);

    // let err_msg = format!(
    //     "Failed to run: `{} {}`. {}",
    //     cmd_name,
    //     args.iter()
    //         .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
    //         .collect::<Vec<_>>()
    //         .join(" "),
    //     "Is `git` installed and on the path?"
    // );

    // let output = cmd.output().expect(&err_msg);
    // let rev = String::from_utf8(output.stdout).unwrap();
    // let rev = rev.trim().trim_matches('"');
    //
    // rev.to_string()

    if let Ok(output) = cmd.output() {
        let rev = String::from_utf8(output.stdout).unwrap();
        let rev = rev.trim().trim_matches('"');
        rev.to_string()
    } else {
        // The dawn folder won't be a git submodule when compiling from the crates.io package
        String::new()
    }
}

fn git_checkout(dawn_dir_src: &PathBuf) {
    let rev = git_log_last_revision("dawn");

    let mut args = Vec::new();

    let cmd_name = if cfg!(windows) {
        args.push(OsString::from("/C"));
        args.push(OsString::from("git"));
        args.push(OsString::from("checkout"));
        args.push(OsString::from(&rev));
        "cmd"
    } else {
        args.push(OsString::from("git"));
        args.push(OsString::from("checkout"));
        args.push(OsString::from(&rev));
        "git"
    };

    let mut cmd = Command::new(cmd_name);
    eprintln!("out_dir_dawn_src: {:?}", dawn_dir_src);

    cmd.current_dir(&dawn_dir_src).args(&args);

    let err_msg = format!(
        "Failed to run: `{} {}`. {}",
        cmd_name,
        args.iter()
            .map(|s| std::ffi::OsStr::to_string_lossy(s).to_owned())
            .collect::<Vec<_>>()
            .join(" "),
        "Is `git` installed and on the path?"
    );

    let mut spawned = cmd.spawn().expect(&err_msg);

    let exit_status = spawned.wait().expect(&err_msg);
    if !exit_status.success() {
        eprintln!("{}", err_msg);
        std::process::exit(1);
    }
}

fn compile_dawnc(dawn_dir_src: &PathBuf, dawn_dir_out: &PathBuf) {
    #[cfg(target_env = "msvc")]
    let clang_path = dawn_dir_src.join("third_party/llvm-build/Release+Asserts/bin/clang-cl");

    let mut build = cc::Build::new();
    build.define("WGPU_SHARED_LIBRARY", None);
    build.define("WGPU_IMPLEMENTATION", None);
    build.define("_WIN32", None);

    build.file("dawnc/dawnc.cpp");
    build.compiler(&clang_path);
    build.no_default_flags(true);

    if cfg!(target_env = "msvc") {
        if is_crt_static() {
            build.flag("/MT");
        } else {
            build.flag("/MD");
        }
        build.flag("-showIncludes");
        build.flag("-Zc:twoPhase");
        build.flag("-Zc:sizedDealloc-");
        //build.flag("-X");
        build.flag("-TP");

        #[cfg(target_pointer_width = "64")]
        build.flag("-m64");

        //build.define("_LIBCPP_ABI_VERSION", Some("1"));
        build.define("_LIBCPP_ABI_UNSTABLE", None);
        build.define("_LIBCPP_ENABLE_NODISCARD", None);
        build.define("_LIBCPP_NO_AUTO_LINK", None);
        build.define("__STD_C", None);
    }

    build.include(dawn_dir_src.join("src"));
    build.include(dawn_dir_src.join("src").join("include"));
    build.include(
        dawn_dir_src
            .join("buildtools")
            .join("third_party")
            .join("libc++")
            .join("trunk")
            .join("include"),
    );

    build.include(dawn_dir_src.join("third_party/khronos"));

    build.include(dawn_dir_out.join("gen").join("src"));
    build.include(dawn_dir_out.join("gen").join("src").join("include"));
    build.include(dawn_dir_out.join("gen").join("src").join("include"));

    build.compile("dawnc");
}

// fn is_debug() -> bool {
//     if cfg!(target_feature="debug_assertions") {
//         true
//     } else {
//         false
//     }
// }

fn is_crt_static() -> bool {
    if cfg!(target_feature = "crt-static") {
        true
    } else {
        false
    }
}

#[cfg(not(feature = "bindgen"))]
fn bindgen(_out_dir: &PathBuf, _out_dir_dawn_src: &PathBuf, _out_dir_dawn_out: &PathBuf) {
    // do nothing
}

#[cfg(feature = "bindgen")]
fn bindgen(out_dir: &PathBuf, out_dir_dawn_src: &PathBuf, out_dir_dawn_out: &PathBuf) {
    let header = out_dir_dawn_out.join("gen/src/include/dawn/webgpu.h");
    let output = bindgen::builder()
        .header(header.to_str().unwrap())
        .blacklist_type("DawnProcTable.*")
        .whitelist_type("WGPU.*")
        .whitelist_var("WGPU.*")
        .whitelist_function("wgpu.*")
        .ctypes_prefix("libc")
        .use_core()
        .impl_debug(true)
        .impl_partialeq(true)
        .prepend_enum_name(false)
        //.layout_tests(false)
        .clang_args(&[
            "-I",
            &format!("{}/gen/src/include", out_dir_dawn_out.to_str().unwrap()),
        ])
        .clang_args(&[
            "-I",
            &format!("{}/include", out_dir_dawn_src.to_str().unwrap()),
        ])
        .generate()
        .expect("bindgen failed");

    output
        .write_to_file(&out_dir.join("webgpu.rs"))
        .expect("failed to write webgpu.rs");

    let header = out_dir_dawn_out.join("gen/src/include/dawn/dawn_proc_table.h");
    let output = bindgen::builder()
        .header(header.to_str().unwrap())
        .whitelist_type("DawnProcTable.*")
        .blacklist_type("WGPU.*")
        .blacklist_function("wgpu.*")
        .ctypes_prefix("libc")
        .use_core()
        .impl_debug(true)
        .impl_partialeq(true)
        .prepend_enum_name(false)
        .whitelist_recursively(false)
        .raw_line("use crate::webgpu::*;")
        .clang_args(&[
            "-I",
            &format!("{}/gen/src/include", out_dir_dawn_out.to_str().unwrap()),
        ])
        .clang_args(&[
            "-I",
            &format!("{}/include", out_dir_dawn_src.to_str().unwrap()),
        ])
        .generate()
        .expect("bindgen failed");

    output
        .write_to_file(&out_dir.join("dawn_proc_table.rs"))
        .expect("failed to write dawn_proc_table.rs");

    let header = out_dir_dawn_src.join("src/include/dawn/dawn_wsi.h");
    let output = bindgen::builder()
        .clang_args(&["-x", "c++"])
        .header(header.to_str().unwrap())
        .whitelist_type("Dawn.*")
        .blacklist_type("WGPU.*")
        .blacklist_function("wgpu.*")
        .ctypes_prefix("libc")
        .use_core()
        .impl_debug(true)
        .impl_partialeq(true)
        .prepend_enum_name(false)
        .whitelist_recursively(false)
        .raw_line("use crate::webgpu::*;")
        .clang_args(&[
            "-I",
            &format!("{}/gen/src/include", out_dir_dawn_out.to_str().unwrap()),
        ])
        .clang_args(&[
            "-I",
            &format!("{}/include", out_dir_dawn_src.to_str().unwrap()),
        ])
        .clang_args(&[
            "-I",
            &format!("{}/third_party/khronos", out_dir_dawn_src.to_str().unwrap()),
        ])
        .generate()
        .expect("bindgen failed");

    output
        .write_to_file(&out_dir.join("dawn_wsi.rs"))
        .expect("failed to write dawn_wsi.rs");
}
