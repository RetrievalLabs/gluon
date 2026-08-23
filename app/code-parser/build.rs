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
    let protos = [
        proto_root.join("gluon/db/v1/business_kg.proto"),
        proto_root.join("gluon/db/v1/characterization_tests.proto"),
        proto_root.join("gluon/db/v1/extraction.proto"),
    ];

    for proto in &protos {
        println!("cargo:rerun-if-changed={}", proto.display());
    }
    prost_build::compile_protos(&protos, &[proto_root]).expect("failed to compile protobuf");
}
