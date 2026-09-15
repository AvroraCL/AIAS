fn main() {
    println!("cargo:rerun-if-changed=vendor/xatlas/xatlas.cpp");
    println!("cargo:rerun-if-changed=vendor/xatlas/xatlas.h");
    println!("cargo:rerun-if-changed=vendor/xatlas/aias_xatlas.cpp");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .debug(false)
        .define("NDEBUG", None)
        .warnings(false)
        .flag_if_supported("-std=c++11")
        .file("vendor/xatlas/xatlas.cpp")
        .file("vendor/xatlas/aias_xatlas.cpp");
    let compiler = build.get_compiler();
    build.compile("aias_xatlas");
    if std::env::var("TARGET")
        .unwrap_or_default()
        .contains("windows-gnu")
    {
        if let Some(root) = compiler.path().parent().and_then(std::path::Path::parent) {
            let library = root.join("x86_64-w64-mingw32").join("lib");
            if library.is_dir() {
                println!("cargo:rustc-link-search=native={}", library.display());
            }
        }
        println!("cargo:rustc-link-lib=static=mcfgthread");
        // GNU release 链接把 libgcc_eh 放在 Rust/Cargo 库之后；再在最终参数处
        // 重复一次线程库，满足静态库从左到右的符号解析顺序。
        println!("cargo:rustc-link-arg=-lmcfgthread");
    }
}
