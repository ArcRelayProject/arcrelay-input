fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);
    prost_build::Config::new()
        .compile_protos(&["proto/arc_input.proto"], &["proto"])
        .expect("compile Arc Input schema");
    println!("cargo:rerun-if-changed=proto/arc_input.proto");
}
