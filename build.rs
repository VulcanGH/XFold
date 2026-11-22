use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=ui/xfold.ico");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let script = out.join("xfold.rc");
    fs::write(&script, "1 ICON \"ui/xfold.ico\"\n").expect("write XFold icon resource");

    let target = env::var("TARGET").unwrap();
    let (resource, status) = if target.ends_with("-windows-gnu") {
        let resource = out.join("xfold-icon.o");
        let status = Command::new("x86_64-w64-mingw32-windres")
            .arg(&script)
            .arg(&resource)
            .status();
        (resource, status)
    } else if target.ends_with("-windows-msvc") {
        let resource = out.join("xfold-icon.res");
        let status = Command::new("rc")
            .arg("/fo")
            .arg(&resource)
            .arg(&script)
            .status();
        (resource, status)
    } else {
        return;
    };
    assert!(
        status.expect("run the Windows resource compiler").success(),
        "compile the XFold icon resource"
    );
    println!("cargo:rustc-link-arg={}", resource.display());
}
