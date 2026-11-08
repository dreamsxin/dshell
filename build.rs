fn main() {
    println!("cargo:rerun-if-changed=dshell.rc");
    println!("cargo:rerun-if-changed=assets/dshell.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("dshell.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("could not embed the dshell application icon");
    }
}
