extern crate protoc_rust;

fn main() {
    protoc_rust::Codegen::new()
        .out_dir("src/api")
        .inputs(["src/api/api.proto"])
        .include("src/api")
        .run()
        .expect("Running protoc failed.");
}
