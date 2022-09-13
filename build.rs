extern crate protoc_rust;

fn main() {
    protoc_rust::Codegen::new()
        .out_dir("src/protocol")
        .inputs(&["src/protocol/index.proto"])
        .include("src/protocol")
        .run()
        .expect("Running protoc failed.");
}