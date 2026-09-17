fn main() {
    println!("cargo:rerun-if-changed=vendor/xatlas/xatlas.cpp");
    println!("cargo:rerun-if-changed=vendor/xatlas/xatlas.h");
    println!("cargo:rerun-if-changed=vendor/xatlas/aias_xatlas.cpp");
    // xatlas 静态库内含 std::thread 调度器。默认动态链 libstdc++-6.dll——该 DLL
    // 按用户 PATH 搜索，Git 等软件的 mingw64\bin 会抢载不匹配版本，调度线程在
    // WaitFor::wait 段错误（0xc0000409/0xc0000005 的 erratic 崩溃源）。这里先于
    // cc 的动态 stdc++ 指令静态链入，符号全部就地解析，后续 -lstdc++ 不再引入
    // DLL 导入。静态库引用的 _MCF_* 由随后链接的 mcfgthread 归档满足。
    if std::env::var("TARGET")
        .unwrap_or_default()
        .contains("windows-gnu")
    {
        println!("cargo:rustc-link-lib=static=stdc++");
    }
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
        // 线程库随工具链变体而异：MCF 变体的 libstdc++/libgcc_eh 引用 mcfgthread，
        // 须显式补链；POSIX/winpthreads 变体由 rustc 的 -l:libpthread.a
        // 覆盖，硬链 mcfgthread 会在没有该库的工具链上直接断链。
        let mut linked = false;
        if let Some(root) = compiler.path().parent().and_then(std::path::Path::parent) {
            let library = root.join("x86_64-w64-mingw32").join("lib");
            if library.is_dir() {
                println!("cargo:rustc-link-search=native={}", library.display());
                if library.join("libmcfgthread.a").is_file() {
                    println!("cargo:rustc-link-lib=static=mcfgthread");
                    // GNU release 链接把 libgcc_eh 放在 Rust/Cargo 库之后；再在
                    // 最终参数处重复一次线程库，满足静态库从左到右的符号解析顺序。
                    println!("cargo:rustc-link-arg=-lmcfgthread");
                    linked = true;
                }
            }
        }
        if !linked {
            // cc 经 PATH 解析时可能只给出相对的可执行名，推不出工具链根；
            // 直接让编译器自报 libmcfgthread.a 的绝对落点，按目录补链。
            // 找不到（POSIX 变体）则保持不链接，维持既有行为。
            if let Ok(output) = std::process::Command::new(compiler.path())
                .arg("--print-file-name=libmcfgthread.a")
                .output()
            {
                let found = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let found = std::path::PathBuf::from(found);
                if found.is_absolute() && found.is_file() {
                    if let Some(dir) = found.parent() {
                        println!("cargo:rustc-link-search=native={}", dir.display());
                    }
                    println!("cargo:rustc-link-lib=static=mcfgthread");
                    println!("cargo:rustc-link-arg=-lmcfgthread");
                }
            }
        }
    }
}
