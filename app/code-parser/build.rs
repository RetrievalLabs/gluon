use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"),
    );
    let repo_root = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("code-parser lives under app/code-parser");
    let proto_root = repo_root.join("app/package");
    let descriptor_path = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"))
        .join("gluon_descriptor.bin");
    let protos = [
        proto_root.join("gluon/db/v1/business_kg.proto"),
        proto_root.join("gluon/db/v1/characterization_tests.proto"),
        proto_root.join("gluon/db/v1/extraction.proto"),
        proto_root.join("gluon/db/v1/sqlite.proto"),
    ];

    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
    let mut includes = vec![proto_root];
    if let Some(protoc_include) = std::env::var_os("PROTOC_INCLUDE") {
        includes.push(PathBuf::from(protoc_include));
    } else {
        let system_protoc_include = PathBuf::from("/usr/include");
        if system_protoc_include
            .join("google/protobuf/descriptor.proto")
            .exists()
        {
            includes.push(system_protoc_include);
        }
    }
    let mut config = prost_build::Config::new();
    config.file_descriptor_set_path(descriptor_path);
    config
        .compile_protos(&protos, &includes)
        .expect("failed to compile protobuf");
}
